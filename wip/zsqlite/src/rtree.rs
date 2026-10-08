//! `ext/rtree/rtree.c` (parte 1): as estruturas do r-tree, os nós em memória, a fila de prioridade
//! da busca, as restrições, o algoritmo R*-tree (escolha de folha, divisão, remoção e reinserção)
//! e as rotinas de arredondamento. A tabela virtual em si (os métodos de `sqlite3_module`), a
//! inicialização, as funções SQL `rtreenode`/`rtreedepth`/`rtreecheck` e as APIs de MATCH estão em
//! [`crate::rtree2`], reexportado no fim deste módulo (`rtree_init` é o `sqlite3RtreeInit`).
//!
//! `SQLITE_ENABLE_GEOPOLY` está desligado no Debian 13: o `geopoly.c` não é portado.
//!
//! Desvios do C, decorrentes do modelo v2 (CONVENTIONS.md):
//!
//! - Os nós (`RtreeNode`) vivem num arenal em `Rtree.nodes` e se referem por [`NodeId`]; o `pParent`
//!   e o `pNext` das cadeias de hash e da lista `pDeleted` são `Option<NodeId>`. A contagem de
//!   referências (`nRef`) continua sendo a do C, porque ela decide quando o nó é gravado.
//! - O `sqlite3_blob` de leitura de nós (`pNodeBlob`) vira um comando preparado
//!   `SELECT data FROM "db"."x_node" WHERE nodeno=?1`: este porte ainda não tem o `vdbeblob.c`
//!   (`sqlite3_blob_open`/`_reopen`/`_read`). Os erros são os mesmos que o C converte em
//!   `SQLITE_CORRUPT_VTAB` (linha ausente, coluna que não é BLOB nem TEXT, tabela inexistente,
//!   tamanho diferente de `iNodeSize`), e o comando é reiniciado a cada leitura, então nenhuma
//!   transação de leitura fica presa entre duas chamadas.
//! - `pUser`/`xDelUser` de `sqlite3_rtree_geometry` viram um `Option<Box<dyn Any>>`: o destrutor é
//!   o `Drop` do valor. `anQueue` de `sqlite3_rtree_query_info` é uma cópia da fila do cursor,
//!   tirada antes de cada chamada do callback (o cursor não muda enquanto ele roda).
//! - O `Rtree.bCorrupt` e o `RTREE_IS_CORRUPT` só existem sob `SQLITE_DEBUG` e somem. Sem a
//!   falta de memória das alocações.

use std::any::Any;
use std::rc::Rc;

use crate::connection::{Connection, StmtId};
use crate::consts::{
    SQLITE_BLOB, SQLITE_CORRUPT_VTAB, SQLITE_DONE, SQLITE_ERROR, SQLITE_NOMEM, SQLITE_OK,
    SQLITE_PREPARE_NO_VTAB, SQLITE_PREPARE_PERSISTENT, SQLITE_ROW, SQLITE_TEXT,
};
use crate::mem::Mem;
use crate::prepare::prepare_v3;
use crate::printf::{mprintf, PrintfArg};
use crate::vdbeapi::{
    bind_blob, bind_int64, bind_null, column_blob, column_int64, column_type, finalize, reset,
    step, value_double,
};

pub use crate::rtree2::*;

/// O r-tree pode ter de 1 a `RTREE_MAX_DIMENSIONS` dimensões.
pub(crate) const RTREE_MAX_DIMENSIONS: usize = 5;

/// Número máximo de colunas auxiliares.
pub(crate) const RTREE_MAX_AUX_COLUMN: i32 = 100;

/// Tamanho da tabela hash `Rtree.aHash`, que nunca tem muitas entradas, por isso o número de
/// baldes é fixo.
const HASHSIZE: usize = 97;

/// Estimativa de linhas quando não há entrada em `sqlite_stat1`.
pub(crate) const RTREE_DEFAULT_ROWEST: i64 = 1048576;
/// Mínimo da estimativa de linhas.
pub(crate) const RTREE_MIN_ROWEST: i64 = 100;

/// Valores de `Rtree.eCoordType`.
pub(crate) const RTREE_COORD_REAL32: u8 = 0;
/// Coordenadas inteiras de 32 bits (`rtree_i32`).
pub(crate) const RTREE_COORD_INT32: u8 = 1;

/// O número máximo de células de um nó recém criado.
pub(crate) const RTREE_MAXCELLS: i32 = 51;

/// Um r-tree tem profundidade 40 ou menos (3^40 é maior que 2^64).
pub(crate) const RTREE_MAX_DEPTH: i32 = 40;

/// Entradas do cache de nós do cursor: a primeira guarda o nó de `sPoint`, as outras guardam os nós
/// dos primeiros elementos da fila de prioridade.
const RTREE_CACHE_SZ: usize = 5;

/// Valores de `RtreeConstraint.op`.
pub(crate) const RTREE_EQ: i32 = 0x41;
/// `<=`.
pub(crate) const RTREE_LE: i32 = 0x42;
/// `<`.
pub(crate) const RTREE_LT: i32 = 0x43;
/// `>=`.
pub(crate) const RTREE_GE: i32 = 0x44;
/// `>`.
pub(crate) const RTREE_GT: i32 = 0x45;
/// `sqlite3_rtree_geometry_callback()` (estilo antigo).
pub(crate) const RTREE_MATCH: i32 = 0x46;
/// `sqlite3_rtree_query_callback()` (estilo novo).
pub(crate) const RTREE_QUERY: i32 = 0x47;
/// Operador só de cursor: sempre satisfeito (`x<'abc'`).
pub(crate) const RTREE_TRUE: i32 = 0x3f;
/// Operador só de cursor: nunca satisfeito (`x=NULL`).
pub(crate) const RTREE_FALSE: i32 = 0x40;

/// Objeto completamente fora da região da consulta.
pub const NOT_WITHIN: i32 = 0;
/// Objeto sobreposto em parte à região da consulta.
pub const PARTLY_WITHIN: i32 = 1;
/// Objeto totalmente dentro da região da consulta.
pub const FULLY_WITHIN: i32 = 2;

/// Constante de arredondamento float para double em direção ao zero.
const RNDTOWARDS: f64 = 1.0 - 1.0 / 8388608.0;
/// Constante de arredondamento float para double para longe do zero.
const RNDAWAY: f64 = 1.0 + 1.0 / 8388608.0;

/// Handle de um [`RtreeNode`] no arenal de [`Rtree`] (o `RtreeNode*` do C).
pub(crate) type NodeId = usize;

/// `struct RtreeNode`: um nó do r-tree em memória.
pub(crate) struct RtreeNode {
    /// Nó pai.
    pub(crate) p_parent: Option<NodeId>,
    /// Número do nó (0 enquanto não foi gravado; a altura quando está em `pDeleted`).
    pub(crate) i_node: i64,
    /// Número de referências.
    pub(crate) n_ref: i32,
    /// Verdadeiro se o nó precisa ser gravado.
    pub(crate) is_dirty: bool,
    /// O conteúdo do nó, como no disco (`iNodeSize` bytes).
    pub(crate) z_data: Vec<u8>,
    /// Próximo nó da cadeia de colisão do hash (ou da lista `pDeleted`).
    pub(crate) p_next: Option<NodeId>,
}

/// `struct Rtree`: uma tabela virtual r-tree.
pub struct Rtree {
    /// `zErrMsg`.
    pub(crate) z_err_msg: Option<Vec<u8>>,
    /// Tamanho em bytes de cada nó da tabela de nós.
    pub(crate) i_node_size: i32,
    /// Número de dimensões.
    pub(crate) n_dim: u8,
    /// Duas vezes o número de dimensões.
    pub(crate) n_dim2: u8,
    /// `RTREE_COORD_REAL32` ou `RTREE_COORD_INT32`.
    pub(crate) e_coord_type: u8,
    /// Bytes consumidos por célula.
    pub(crate) n_bytes_per_cell: u8,
    /// Verdadeiro dentro de uma transação de escrita.
    pub(crate) in_wr_trans: u8,
    /// Número de colunas auxiliares em `%_rowid`.
    pub(crate) n_aux: u8,
    /// Profundidade corrente da estrutura.
    pub(crate) i_depth: i32,
    /// Nome do banco que contém a tabela.
    pub(crate) z_db: Vec<u8>,
    /// Nome da tabela.
    pub(crate) z_name: Vec<u8>,
    /// Nome da tabela `%_node`.
    pub(crate) z_node_name: Vec<u8>,
    /// Número corrente de usuários da estrutura.
    pub(crate) n_busy: u32,
    /// Estimativa do número de linhas.
    pub(crate) n_row_est: i64,
    /// Número de cursores abertos.
    pub(crate) n_cursor: u32,
    /// Número de nós com `nRef` positivo.
    pub(crate) n_node_ref: u32,
    /// SQL do comando que lê os dados auxiliares.
    pub(crate) z_read_aux_sql: Option<Vec<u8>>,
    /// Nós removidos durante um `CondenseTree`, encadeados por `p_next`.
    pub(crate) p_deleted: Option<NodeId>,
    /// Leitura de um nó de `xxx_node` (o `pNodeBlob`, ver o cabeçalho do módulo).
    pub(crate) p_node_blob: Option<StmtId>,
    /// Grava um registro de `xxx_node`.
    pub(crate) p_write_node: Option<StmtId>,
    /// Apaga um registro de `xxx_node`.
    pub(crate) p_delete_node: Option<StmtId>,
    /// Lê um registro de `xxx_rowid`.
    pub(crate) p_read_rowid: Option<StmtId>,
    /// Grava um registro de `xxx_rowid`.
    pub(crate) p_write_rowid: Option<StmtId>,
    /// Apaga um registro de `xxx_rowid`.
    pub(crate) p_delete_rowid: Option<StmtId>,
    /// Lê um registro de `xxx_parent`.
    pub(crate) p_read_parent: Option<StmtId>,
    /// Grava um registro de `xxx_parent`.
    pub(crate) p_write_parent: Option<StmtId>,
    /// Apaga um registro de `xxx_parent`.
    pub(crate) p_delete_parent: Option<StmtId>,
    /// Grava os campos "aux:".
    pub(crate) p_write_aux: Option<StmtId>,
    /// Tabela hash dos nós em memória (`aHash`).
    pub(crate) a_hash: Vec<Option<NodeId>>,
    /// O arenal dos nós.
    pub(crate) nodes: Vec<Option<RtreeNode>>,
    /// Vagas livres do arenal.
    pub(crate) free_nodes: Vec<NodeId>,
}

impl Rtree {
    /// Um `Rtree` zerado, com `nBusy` 1 (o `memset` e o `nBusy = 1` de `rtreeInit`).
    pub(crate) fn new(e_coord_type: u8, z_db: Vec<u8>, z_name: Vec<u8>) -> Rtree {
        let mut z_node_name = z_name.clone();
        z_node_name.extend_from_slice(b"_node");
        Rtree {
            z_err_msg: None,
            i_node_size: 0,
            n_dim: 0,
            n_dim2: 0,
            e_coord_type,
            n_bytes_per_cell: 0,
            in_wr_trans: 0,
            n_aux: 0,
            i_depth: 0,
            z_db,
            z_name,
            z_node_name,
            n_busy: 1,
            n_row_est: 0,
            n_cursor: 0,
            n_node_ref: 0,
            z_read_aux_sql: None,
            p_deleted: None,
            p_node_blob: None,
            p_write_node: None,
            p_delete_node: None,
            p_read_rowid: None,
            p_write_rowid: None,
            p_delete_rowid: None,
            p_read_parent: None,
            p_write_parent: None,
            p_delete_parent: None,
            p_write_aux: None,
            a_hash: vec![None; HASHSIZE],
            nodes: Vec::new(),
            free_nodes: Vec::new(),
        }
    }

    /// O nó `id`.
    pub(crate) fn node(&self, id: NodeId) -> &RtreeNode {
        self.nodes[id].as_ref().expect("rtree: nó liberado")
    }

    /// O nó `id`, mutável.
    pub(crate) fn node_mut(&mut self, id: NodeId) -> &mut RtreeNode {
        self.nodes[id].as_mut().expect("rtree: nó liberado")
    }

    /// Aloca o nó no arenal.
    fn node_alloc(&mut self, n: RtreeNode) -> NodeId {
        if let Some(id) = self.free_nodes.pop() {
            self.nodes[id] = Some(n);
            id
        } else {
            self.nodes.push(Some(n));
            self.nodes.len() - 1
        }
    }

    /// `sqlite3_free(pNode)`.
    fn node_free(&mut self, id: NodeId) {
        self.nodes[id] = None;
        self.free_nodes.push(id);
    }

    /// `RTREE_MINCELLS(p)`.
    pub(crate) fn min_cells(&self) -> i32 {
        ((self.i_node_size - 4) / self.n_bytes_per_cell as i32) / 3
    }

    /// `NCELL(pNode)`.
    pub(crate) fn n_cell(&self, id: NodeId) -> i32 {
        read_int16(&self.node(id).z_data[2..])
    }
}

/// `sqlite3_finalize` de um comando que pode ser nulo.
pub(crate) fn finalize_stmt(db: &mut Connection, p: &mut Option<StmtId>) -> i32 {
    match p.take() {
        Some(id) => finalize(db, id),
        None => SQLITE_OK,
    }
}

// ---------------------------------------------------------------------------------------------
// Coordenadas, células e (de)serialização
// ---------------------------------------------------------------------------------------------

/// `union RtreeCoord`: uma coordenada, guardada como os 32 bits crus (`u`); `f` e `i` são as duas
/// leituras possíveis.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct RtreeCoord(pub(crate) u32);

impl RtreeCoord {
    /// O campo `f`.
    #[inline]
    pub(crate) fn f(self) -> f32 {
        f32::from_bits(self.0)
    }

    /// O campo `i`.
    #[inline]
    pub(crate) fn i(self) -> i32 {
        self.0 as i32
    }

    /// Atribui o campo `f`.
    #[inline]
    pub(crate) fn from_f(f: f32) -> RtreeCoord {
        RtreeCoord(f.to_bits())
    }

    /// Atribui o campo `i`.
    #[inline]
    pub(crate) fn from_i(i: i32) -> RtreeCoord {
        RtreeCoord(i as u32)
    }

    /// `DCOORD(coord)`: o valor como `double`.
    #[inline]
    pub(crate) fn d(self, e_coord_type: u8) -> f64 {
        if e_coord_type == RTREE_COORD_REAL32 {
            self.f() as f64
        } else {
            self.i() as f64
        }
    }
}

/// `struct RtreeCell`: uma célula de um nó, deserializada.
#[derive(Clone, Copy, Default)]
pub(crate) struct RtreeCell {
    /// Rowid da entrada ou número do nó.
    pub(crate) i_rowid: i64,
    /// Coordenadas da caixa envolvente.
    pub(crate) a_coord: [RtreeCoord; RTREE_MAX_DIMENSIONS * 2],
}

/// `readInt16`.
#[inline]
pub(crate) fn read_int16(p: &[u8]) -> i32 {
    ((p[0] as i32) << 8) + p[1] as i32
}

/// `readCoord`.
#[inline]
pub(crate) fn read_coord(p: &[u8]) -> RtreeCoord {
    RtreeCoord(u32::from_be_bytes([p[0], p[1], p[2], p[3]]))
}

/// `readInt64`.
#[inline]
pub(crate) fn read_int64(p: &[u8]) -> i64 {
    i64::from_be_bytes([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]])
}

/// `writeInt16`.
#[inline]
pub(crate) fn write_int16(p: &mut [u8], i: i32) {
    p[0] = ((i >> 8) & 0xFF) as u8;
    p[1] = (i & 0xFF) as u8;
}

/// `writeCoord`: devolve o número de bytes gravados (sempre 4).
#[inline]
fn write_coord(p: &mut [u8], c: RtreeCoord) -> usize {
    p[..4].copy_from_slice(&c.0.to_be_bytes());
    4
}

/// `writeInt64`: devolve o número de bytes gravados (sempre 8).
#[inline]
fn write_int64(p: &mut [u8], i: i64) -> usize {
    p[..8].copy_from_slice(&i.to_be_bytes());
    8
}

/// O corpo de `nodeGetCell` sobre os bytes de um nó (também serve a `rtreenode()`, que não tem um
/// `Rtree` de verdade): a célula `i_cell` de um nó de `data` com células de `n_bytes_per_cell`
/// bytes e `n_dim2` coordenadas.
pub(crate) fn cell_from_data(
    n_bytes_per_cell: usize,
    n_dim2: usize,
    data: &[u8],
    i_cell: usize,
) -> RtreeCell {
    let mut cell = RtreeCell { i_rowid: read_int64(&data[4 + n_bytes_per_cell * i_cell..]), ..RtreeCell::default() };
    let mut off = 12 + n_bytes_per_cell * i_cell;
    let mut ii = 0;
    loop {
        cell.a_coord[ii] = read_coord(&data[off..]);
        cell.a_coord[ii + 1] = read_coord(&data[off + 4..]);
        off += 8;
        ii += 2;
        if ii >= n_dim2 {
            break;
        }
    }
    cell
}

// ---------------------------------------------------------------------------------------------
// Nós
// ---------------------------------------------------------------------------------------------

/// `nodeReference`.
pub(crate) fn node_reference(p_rtree: &mut Rtree, p: Option<NodeId>) {
    if let Some(id) = p {
        debug_assert!(p_rtree.node(id).n_ref > 0);
        p_rtree.node_mut(id).n_ref += 1;
    }
}

/// `nodeZero`: zera o conteúdo do nó (menos os 2 primeiros bytes).
fn node_zero(p_rtree: &mut Rtree, id: NodeId) {
    let n = p_rtree.i_node_size as usize;
    let node = p_rtree.node_mut(id);
    node.z_data[2..n].fill(0);
    node.is_dirty = true;
}

/// `nodeHash`.
fn node_hash(i_node: i64) -> usize {
    ((i_node as u32) as usize) % HASHSIZE
}

/// `nodeHashLookup`.
pub(crate) fn node_hash_lookup(p_rtree: &Rtree, i_node: i64) -> Option<NodeId> {
    let mut p = p_rtree.a_hash[node_hash(i_node)];
    while let Some(id) = p {
        if p_rtree.node(id).i_node == i_node {
            break;
        }
        p = p_rtree.node(id).p_next;
    }
    p
}

/// `nodeHashInsert`.
fn node_hash_insert(p_rtree: &mut Rtree, id: NodeId) {
    debug_assert!(p_rtree.node(id).p_next.is_none());
    let i_hash = node_hash(p_rtree.node(id).i_node);
    let head = p_rtree.a_hash[i_hash];
    p_rtree.node_mut(id).p_next = head;
    p_rtree.a_hash[i_hash] = Some(id);
}

/// `nodeHashDelete`.
fn node_hash_delete(p_rtree: &mut Rtree, id: NodeId) {
    if p_rtree.node(id).i_node != 0 {
        let i_hash = node_hash(p_rtree.node(id).i_node);
        let next = p_rtree.node(id).p_next;
        if p_rtree.a_hash[i_hash] == Some(id) {
            p_rtree.a_hash[i_hash] = next;
        } else {
            let mut cur = p_rtree.a_hash[i_hash];
            while let Some(c) = cur {
                if p_rtree.node(c).p_next == Some(id) {
                    p_rtree.node_mut(c).p_next = next;
                    break;
                }
                cur = p_rtree.node(c).p_next;
            }
        }
        p_rtree.node_mut(id).p_next = None;
    }
}

/// `nodeNew`: um nó novo, com `iNode==0` (o número vem quando `nodeWrite()` o grava).
pub(crate) fn node_new(p_rtree: &mut Rtree, p_parent: Option<NodeId>) -> NodeId {
    let id = p_rtree.node_alloc(RtreeNode {
        p_parent,
        i_node: 0,
        n_ref: 1,
        is_dirty: true,
        z_data: vec![0u8; p_rtree.i_node_size as usize],
        p_next: None,
    });
    p_rtree.n_node_ref += 1;
    node_reference(p_rtree, p_parent);
    id
}

/// `nodeBlobReset`.
pub(crate) fn node_blob_reset(db: &mut Connection, p_rtree: &mut Rtree) {
    finalize_stmt(db, &mut p_rtree.p_node_blob);
}

/// O trabalho do `sqlite3_blob_open`/`_reopen` e `_read` de `nodeAcquire` (ver o cabeçalho do
/// módulo): lê o blob `data` da linha `i_node` de `%_node`. Em erro devolve o código que o C
/// receberia do `sqlite3_blob_*` (`SQLITE_ERROR` se a linha não existe ou a coluna não é BLOB nem
/// TEXT).
fn node_blob_read(
    db: &mut Connection,
    p_rtree: &mut Rtree,
    i_node: i64,
) -> Result<Vec<u8>, i32> {
    if p_rtree.p_node_blob.is_none() {
        let z_sql = mprintf(
            b"SELECT data FROM \"%w\".\"%w\" WHERE nodeno=?1",
            &[
                PrintfArg::Text(Some(p_rtree.z_db.clone())),
                PrintfArg::Text(Some(p_rtree.z_node_name.clone())),
            ],
        )
        .ok_or(SQLITE_NOMEM)?;
        let (rc, stmt, _) =
            prepare_v3(db, &z_sql, -1, SQLITE_PREPARE_PERSISTENT | SQLITE_PREPARE_NO_VTAB);
        if rc != SQLITE_OK {
            return Err(rc);
        }
        p_rtree.p_node_blob = stmt;
    }
    let id = p_rtree.p_node_blob.unwrap_or_default();
    bind_int64(db, id, 1, i_node);
    let rc = step(db, id);
    let res = if rc == SQLITE_ROW {
        let e_type = column_type(db, id, 0);
        if e_type == SQLITE_BLOB || e_type == SQLITE_TEXT {
            Ok(column_blob(db, id, 0).map(<[u8]>::to_vec).unwrap_or_default())
        } else {
            Err(SQLITE_ERROR)
        }
    } else if rc == SQLITE_DONE {
        Err(SQLITE_ERROR)
    } else {
        Err(rc)
    };
    reset(db, id);
    res
}

/// `nodeAcquire`: obtém uma referência a um nó.
pub(crate) fn node_acquire(
    db: &mut Connection,
    p_rtree: &mut Rtree,
    i_node: i64,
    p_parent: Option<NodeId>,
    pp_node: &mut Option<NodeId>,
) -> i32 {
    let mut rc;
    let mut p_node: Option<NodeId> = None;

    // O nó pedido já está na tabela hash? Então só aumenta a contagem.
    if let Some(n) = node_hash_lookup(p_rtree, i_node) {
        if p_parent.is_some() && p_parent != p_rtree.node(n).p_parent {
            return SQLITE_CORRUPT_VTAB;
        }
        p_rtree.node_mut(n).n_ref += 1;
        *pp_node = Some(n);
        return SQLITE_OK;
    }

    match node_blob_read(db, p_rtree, i_node) {
        Err(e) => {
            rc = e;
            node_blob_reset(db, p_rtree);
            if rc == SQLITE_NOMEM {
                return SQLITE_NOMEM;
            }
            *pp_node = None;
            // Se o blob não abre na linha pedida, só pode ser dado errado nas tabelas sombra.
            if rc == SQLITE_ERROR {
                rc = SQLITE_CORRUPT_VTAB;
            }
        }
        Ok(data) => {
            rc = SQLITE_OK;
            if p_rtree.i_node_size as usize == data.len() {
                p_node = Some(p_rtree.node_alloc(RtreeNode {
                    p_parent,
                    i_node,
                    n_ref: 1,
                    is_dirty: false,
                    z_data: data,
                    p_next: None,
                }));
                p_rtree.n_node_ref += 1;
            }
        }
    }

    // Se a raiz acabou de ser carregada, `iDepth` é a altura da estrutura. Profundidade maior que
    // `RTREE_MAX_DEPTH` é r-tree corrompido.
    if rc == SQLITE_OK {
        if let Some(n) = p_node {
            if i_node == 1 {
                p_rtree.i_depth = read_int16(&p_rtree.node(n).z_data);
                if p_rtree.i_depth > RTREE_MAX_DEPTH {
                    rc = SQLITE_CORRUPT_VTAB;
                }
            }
        }
    }

    // O campo "número de entradas" grande demais também é corrupção.
    if let Some(n) = p_node {
        if rc == SQLITE_OK && p_rtree.n_cell(n) > (p_rtree.i_node_size - 4) / p_rtree.n_bytes_per_cell as i32 {
            rc = SQLITE_CORRUPT_VTAB;
        }
    }

    if rc == SQLITE_OK {
        if let Some(n) = p_node {
            node_reference(p_rtree, p_parent);
            node_hash_insert(p_rtree, n);
        } else {
            rc = SQLITE_CORRUPT_VTAB;
        }
        *pp_node = p_node;
    } else {
        node_blob_reset(db, p_rtree);
        if let Some(n) = p_node {
            p_rtree.n_node_ref -= 1;
            p_rtree.node_free(n);
        }
        *pp_node = None;
    }

    rc
}

/// `nodeOverwriteCell`: sobrescreve a célula `i_cell` do nó com `p_cell`.
pub(crate) fn node_overwrite_cell(
    p_rtree: &mut Rtree,
    id: NodeId,
    p_cell: &RtreeCell,
    i_cell: i32,
) {
    let n_dim2 = p_rtree.n_dim2 as usize;
    let mut off = 4 + p_rtree.n_bytes_per_cell as usize * i_cell as usize;
    let node = p_rtree.node_mut(id);
    off += write_int64(&mut node.z_data[off..], p_cell.i_rowid);
    for ii in 0..n_dim2 {
        off += write_coord(&mut node.z_data[off..], p_cell.a_coord[ii]);
    }
    node.is_dirty = true;
}

/// `nodeDeleteCell`: remove a célula `i_cell` do nó.
fn node_delete_cell(p_rtree: &mut Rtree, id: NodeId, i_cell: i32) {
    let bpc = p_rtree.n_bytes_per_cell as i32;
    let n_cell = p_rtree.n_cell(id);
    let dst = (4 + bpc * i_cell) as usize;
    let n_byte = ((n_cell - i_cell - 1) * bpc) as usize;
    let node = p_rtree.node_mut(id);
    node.z_data.copy_within(dst + bpc as usize..dst + bpc as usize + n_byte, dst);
    write_int16(&mut node.z_data[2..], n_cell - 1);
    node.is_dirty = true;
}

/// `nodeInsertCell`: insere a célula no nó; devolve diferente de zero (o `SQLITE_FULL` do
/// comentário do C) se o nó já está cheio e nada foi inserido.
pub(crate) fn node_insert_cell(p_rtree: &mut Rtree, id: NodeId, p_cell: &RtreeCell) -> i32 {
    let n_max_cell = (p_rtree.i_node_size - 4) / p_rtree.n_bytes_per_cell as i32;
    let n_cell = p_rtree.n_cell(id);
    debug_assert!(n_cell <= n_max_cell);
    if n_cell < n_max_cell {
        node_overwrite_cell(p_rtree, id, p_cell, n_cell);
        let node = p_rtree.node_mut(id);
        write_int16(&mut node.z_data[2..], n_cell + 1);
        node.is_dirty = true;
    }
    (n_cell == n_max_cell) as i32
}

/// `nodeWrite`: se o nó está sujo, grava-o no banco.
pub(crate) fn node_write(db: &mut Connection, p_rtree: &mut Rtree, id: NodeId) -> i32 {
    let mut rc = SQLITE_OK;
    if p_rtree.node(id).is_dirty {
        let p = p_rtree.p_write_node.unwrap_or_default();
        if p_rtree.node(id).i_node != 0 {
            bind_int64(db, p, 1, p_rtree.node(id).i_node);
        } else {
            bind_null(db, p, 1);
        }
        bind_blob(
            db,
            p,
            2,
            Some(&p_rtree.node(id).z_data[..]),
            p_rtree.i_node_size,
            crate::mem::StrDtor::Transient,
        );
        step(db, p);
        p_rtree.node_mut(id).is_dirty = false;
        rc = reset(db, p);
        bind_null(db, p, 2);
        if p_rtree.node(id).i_node == 0 && rc == SQLITE_OK {
            p_rtree.node_mut(id).i_node = crate::main::last_insert_rowid(db);
            node_hash_insert(p_rtree, id);
        }
    }
    rc
}

/// `nodeRelease`: solta uma referência a um nó. Se ele está sujo e a contagem chega a zero, é
/// gravado.
pub(crate) fn node_release(db: &mut Connection, p_rtree: &mut Rtree, p_node: Option<NodeId>) -> i32 {
    let mut rc = SQLITE_OK;
    if let Some(id) = p_node {
        debug_assert!(p_rtree.node(id).n_ref > 0);
        debug_assert!(p_rtree.n_node_ref > 0);
        p_rtree.node_mut(id).n_ref -= 1;
        if p_rtree.node(id).n_ref == 0 {
            p_rtree.n_node_ref -= 1;
            if p_rtree.node(id).i_node == 1 {
                p_rtree.i_depth = -1;
            }
            if let Some(par) = p_rtree.node(id).p_parent {
                rc = node_release(db, p_rtree, Some(par));
            }
            if rc == SQLITE_OK {
                rc = node_write(db, p_rtree, id);
            }
            node_hash_delete(p_rtree, id);
            p_rtree.node_free(id);
        }
    }
    rc
}

/// `nodeGetRowid`: o inteiro de 64 bits da célula `i_cell` (rowid na folha, número do filho no nó
/// interno).
pub(crate) fn node_get_rowid(p_rtree: &Rtree, id: NodeId, i_cell: i32) -> i64 {
    debug_assert!(i_cell < p_rtree.n_cell(id));
    read_int64(&p_rtree.node(id).z_data[4 + p_rtree.n_bytes_per_cell as usize * i_cell as usize..])
}

/// `nodeGetCoord`: a coordenada `i_coord` da célula `i_cell`.
pub(crate) fn node_get_coord(p_rtree: &Rtree, id: NodeId, i_cell: i32, i_coord: i32) -> RtreeCoord {
    debug_assert!(i_cell < p_rtree.n_cell(id));
    read_coord(
        &p_rtree.node(id).z_data
            [12 + p_rtree.n_bytes_per_cell as usize * i_cell as usize + 4 * i_coord as usize..],
    )
}

/// `nodeGetCell`: deserializa a célula `i_cell` do nó.
pub(crate) fn node_get_cell(p_rtree: &Rtree, id: NodeId, i_cell: i32) -> RtreeCell {
    cell_from_data(
        p_rtree.n_bytes_per_cell as usize,
        p_rtree.n_dim2 as usize,
        &p_rtree.node(id).z_data,
        i_cell as usize,
    )
}

/// `rtreeRelease`: solta uma referência à tabela. Quando a contagem chega a zero os comandos
/// preparados são finalizados.
pub(crate) fn rtree_release(db: &mut Connection, p_rtree: &mut Rtree) {
    p_rtree.n_busy -= 1;
    if p_rtree.n_busy == 0 {
        p_rtree.in_wr_trans = 0;
        debug_assert!(p_rtree.n_cursor == 0);
        node_blob_reset(db, p_rtree);
        finalize_stmt(db, &mut p_rtree.p_write_node);
        finalize_stmt(db, &mut p_rtree.p_delete_node);
        finalize_stmt(db, &mut p_rtree.p_read_rowid);
        finalize_stmt(db, &mut p_rtree.p_write_rowid);
        finalize_stmt(db, &mut p_rtree.p_delete_rowid);
        finalize_stmt(db, &mut p_rtree.p_read_parent);
        finalize_stmt(db, &mut p_rtree.p_write_parent);
        finalize_stmt(db, &mut p_rtree.p_delete_parent);
        finalize_stmt(db, &mut p_rtree.p_write_aux);
        p_rtree.z_read_aux_sql = None;
    }
}

// ---------------------------------------------------------------------------------------------
// Restrições, geometria e o cursor
// ---------------------------------------------------------------------------------------------

/// `sqlite3_rtree_geometry`: o argumento dos callbacks de `sqlite3_rtree_geometry_callback()`.
#[derive(Default)]
pub struct RtreeGeometry {
    /// Cópia do `pContext` de `sqlite3_rtree_geometry_callback()`.
    pub p_context: Option<Rc<dyn Any>>,
    /// Tamanho de `a_param`.
    pub n_param: i32,
    /// Parâmetros passados à função SQL de geometria.
    pub a_param: Vec<f64>,
    /// Dado do callback; o destrutor (`xDelUser`) é o `Drop` do valor.
    pub p_user: Option<Box<dyn Any>>,
}

/// `sqlite3_rtree_query_info`: o argumento dos callbacks de `sqlite3_rtree_query_callback()`. Os
/// 5 primeiros campos são os de [`RtreeGeometry`] (`base`, acessíveis também por `Deref`).
#[derive(Default)]
pub struct RtreeQueryInfo {
    /// Os campos herdados de `sqlite3_rtree_geometry`.
    pub base: RtreeGeometry,
    /// Coordenadas do nó ou da entrada a verificar.
    pub a_coord: Vec<f64>,
    /// Número de entradas pendentes na fila, por nível.
    pub an_queue: Vec<u32>,
    /// Número de coordenadas.
    pub n_coord: i32,
    /// Nível do nó ou da entrada corrente.
    pub i_level: i32,
    /// O maior valor de `iLevel` da árvore.
    pub mx_level: i32,
    /// Rowid da entrada corrente.
    pub i_rowid: i64,
    /// Pontuação do nó pai.
    pub r_parent_score: f64,
    /// Visibilidade do nó pai.
    pub e_parent_within: i32,
    /// SAÍDA: visibilidade.
    pub e_within: i32,
    /// SAÍDA: a pontuação.
    pub r_score: f64,
    /// Os valores SQL originais dos parâmetros (3.8.11).
    pub ap_sql_param: Rc<Vec<Mem>>,
}

impl std::ops::Deref for RtreeQueryInfo {
    type Target = RtreeGeometry;

    fn deref(&self) -> &RtreeGeometry {
        &self.base
    }
}

impl std::ops::DerefMut for RtreeQueryInfo {
    fn deref_mut(&mut self) -> &mut RtreeGeometry {
        &mut self.base
    }
}

/// O callback de geometria do estilo antigo: `(geometria, nCoord, coordenadas, &within)`.
pub type RtreeGeomFn = fn(&mut RtreeGeometry, i32, &[f64], &mut i32) -> i32;

/// O callback de consulta do estilo novo.
pub type RtreeQueryFn = fn(&mut RtreeQueryInfo) -> i32;

/// `struct RtreeConstraint`: uma restrição de busca.
#[derive(Default)]
pub(crate) struct RtreeConstraint {
    /// Índice da coordenada restringida.
    pub(crate) i_coord: i32,
    /// Operação (`RTREE_*`).
    pub(crate) op: i32,
    /// O valor da restrição (`u.rValue`).
    pub(crate) r_value: f64,
    /// `u.xGeom`.
    pub(crate) x_geom: Option<RtreeGeomFn>,
    /// `u.xQueryFunc`.
    pub(crate) x_query_func: Option<RtreeQueryFn>,
    /// Argumento de `xGeom` e `xQueryFunc`.
    pub(crate) p_info: Option<Box<RtreeQueryInfo>>,
}

/// `struct RtreeSearchPoint`: um resultado intermediário da caminhada pela árvore.
#[derive(Clone, Copy, Default)]
pub(crate) struct RtreeSearchPoint {
    /// A pontuação do ponto; a menor vem primeiro.
    pub(crate) r_score: f64,
    /// O número do nó.
    pub(crate) id: i64,
    /// 0 é entrada, 1 é folha, 2 ou mais são níveis acima.
    pub(crate) i_level: u8,
    /// `PARTLY_WITHIN` ou `FULLY_WITHIN`.
    pub(crate) e_within: u8,
    /// Índice da célula dentro do nó.
    pub(crate) i_cell: u8,
}

/// A posição de um ponto de busca no cursor: `sPoint` ou `aPoint[i]` (o `RtreeSearchPoint*`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sp {
    /// `&pCur->sPoint`.
    S,
    /// `&pCur->aPoint[i]`.
    Q(usize),
}

/// `struct RtreeCursor`: o cursor de uma tabela r-tree.
pub struct RtreeCursor {
    /// Verdadeiro no fim da busca.
    pub(crate) at_eof: u8,
    /// Verdadeiro se `sPoint` é válido.
    pub(crate) b_point: u8,
    /// Verdadeiro se `pReadAux` é válido.
    pub(crate) b_aux_valid: u8,
    /// Cópia do parâmetro `idxNum`.
    pub(crate) i_strategy: i32,
    /// As restrições da busca (`aConstraint`/`nConstraint`).
    pub(crate) a_constraint: Vec<RtreeConstraint>,
    /// A fila de prioridade (`aPoint`/`nPoint`).
    pub(crate) a_point: Vec<RtreeSearchPoint>,
    /// O comando que lê os dados auxiliares.
    pub(crate) p_read_aux: Option<StmtId>,
    /// O próximo ponto de busca, em cache.
    pub(crate) s_point: RtreeSearchPoint,
    /// O cache de nós.
    pub(crate) a_node: [Option<NodeId>; RTREE_CACHE_SZ],
    /// Número de entradas na fila, por `iLevel`.
    pub(crate) an_queue: [u32; RTREE_MAX_DEPTH as usize + 1],
}

impl RtreeCursor {
    /// O cursor logo depois de `rtreeOpen`.
    pub(crate) fn new() -> RtreeCursor {
        RtreeCursor {
            at_eof: 0,
            b_point: 0,
            b_aux_valid: 0,
            i_strategy: 0,
            a_constraint: Vec::new(),
            a_point: Vec::new(),
            p_read_aux: None,
            s_point: RtreeSearchPoint::default(),
            a_node: [None; RTREE_CACHE_SZ],
            an_queue: [0; RTREE_MAX_DEPTH as usize + 1],
        }
    }

    /// O ponto de busca em `p`.
    pub(crate) fn point(&self, p: Sp) -> &RtreeSearchPoint {
        match p {
            Sp::S => &self.s_point,
            Sp::Q(i) => &self.a_point[i],
        }
    }

    /// O ponto de busca em `p`, mutável.
    pub(crate) fn point_mut(&mut self, p: Sp) -> &mut RtreeSearchPoint {
        match p {
            Sp::S => &mut self.s_point,
            Sp::Q(i) => &mut self.a_point[i],
        }
    }
}

/// `resetCursor`: volta o cursor ao estado em que `rtreeOpen()` o deixa.
pub(crate) fn reset_cursor(db: &mut Connection, p_rtree: &mut Rtree, p_csr: &mut RtreeCursor) {
    // Soltar as restrições roda o `xDelUser` de cada `pUser` (o `Drop`).
    p_csr.a_constraint = Vec::new();
    for ii in 0..RTREE_CACHE_SZ {
        let n = p_csr.a_node[ii].take();
        node_release(db, p_rtree, n);
    }
    let p_stmt = p_csr.p_read_aux;
    *p_csr = RtreeCursor::new();
    p_csr.p_read_aux = p_stmt;
}

/// `RTREE_DECODE_COORD(eInt, a, r)`.
#[inline]
fn decode_coord(e_int: bool, a: &[u8]) -> f64 {
    let c = read_coord(a);
    if e_int {
        c.i() as f64
    } else {
        c.f() as f64
    }
}

/// `rtreeCallbackConstraint`: confere o nó ou a entrada `p_cell_data` contra a restrição MATCH.
fn rtree_callback_constraint(
    p_constraint: &mut RtreeConstraint,
    e_int: bool,
    p_cell_data: &[u8],
    p_search: &RtreeSearchPoint,
    an_queue: &[u32],
    pr_score: &mut f64,
    pe_within: &mut i32,
) -> i32 {
    let op = p_constraint.op;
    let x_geom = p_constraint.x_geom;
    let x_query_func = p_constraint.x_query_func;
    let Some(p_info) = p_constraint.p_info.as_mut() else {
        return SQLITE_ERROR;
    };
    let n_coord = p_info.n_coord;
    debug_assert!(op == RTREE_MATCH || op == RTREE_QUERY);
    debug_assert!(matches!(n_coord, 2 | 4 | 6 | 8 | 10));

    if op == RTREE_QUERY && p_search.i_level == 1 {
        p_info.i_rowid = read_int64(p_cell_data);
    }
    let cell = &p_cell_data[8..];
    let a_coord: Vec<f64> =
        (0..n_coord as usize).map(|i| decode_coord(e_int, &cell[4 * i..])).collect();
    let rc;
    if op == RTREE_MATCH {
        let mut e_within = 0;
        rc = match x_geom {
            Some(f) => f(&mut p_info.base, n_coord, &a_coord, &mut e_within),
            None => SQLITE_ERROR,
        };
        if e_within == 0 {
            *pe_within = NOT_WITHIN;
        }
        *pr_score = 0.0;
    } else {
        p_info.a_coord = a_coord;
        p_info.an_queue.clear();
        p_info.an_queue.extend_from_slice(an_queue);
        p_info.i_level = p_search.i_level as i32 - 1;
        p_info.r_score = p_search.r_score;
        p_info.r_parent_score = p_search.r_score;
        p_info.e_within = p_search.e_within as i32;
        p_info.e_parent_within = p_search.e_within as i32;
        rc = match x_query_func {
            Some(f) => f(p_info),
            None => SQLITE_ERROR,
        };
        if p_info.e_within < *pe_within {
            *pe_within = p_info.e_within;
        }
        if p_info.r_score < *pr_score || *pr_score < 0.0 {
            *pr_score = p_info.r_score;
        }
    }
    rc
}

/// `rtreeNonleafConstraint`: confere o nó interno `p_cell_data` contra a restrição `p`. Se nenhum
/// filho pode satisfazê-la, `*pe_within` vira `NOT_WITHIN`.
fn rtree_nonleaf_constraint(p: &RtreeConstraint, e_int: bool, p_cell_data: &[u8], pe_within: &mut i32) {
    // `p->iCoord` pode apontar para o limite inferior ou o superior do par; o ponteiro vai para o
    // inferior.
    let mut off = 8 + 4 * (p.i_coord & 0xfe) as usize;
    debug_assert!(matches!(
        p.op,
        RTREE_LE | RTREE_LT | RTREE_GE | RTREE_GT | RTREE_EQ | RTREE_TRUE | RTREE_FALSE
    ));
    match p.op {
        RTREE_TRUE => return, // sempre satisfeita
        RTREE_FALSE => {}     // nunca satisfeita
        RTREE_EQ => {
            let val = decode_coord(e_int, &p_cell_data[off..]);
            // `val` é o limite inferior do par.
            if p.r_value >= val {
                off += 4;
                let val = decode_coord(e_int, &p_cell_data[off..]);
                // `val` é o limite superior do par.
                if p.r_value <= val {
                    return;
                }
            }
        }
        RTREE_LE | RTREE_LT => {
            let val = decode_coord(e_int, &p_cell_data[off..]);
            if p.r_value >= val {
                return;
            }
        }
        _ => {
            off += 4;
            let val = decode_coord(e_int, &p_cell_data[off..]);
            if p.r_value <= val {
                return;
            }
        }
    }
    *pe_within = NOT_WITHIN;
}

/// `rtreeLeafConstraint`: confere a célula de folha `p_cell_data` contra a restrição `p`
/// (`xN op $val`). Se não é satisfeita, `*pe_within` vira `NOT_WITHIN`.
fn rtree_leaf_constraint(p: &RtreeConstraint, e_int: bool, p_cell_data: &[u8], pe_within: &mut i32) {
    debug_assert!(matches!(
        p.op,
        RTREE_LE | RTREE_LT | RTREE_GE | RTREE_GT | RTREE_EQ | RTREE_TRUE | RTREE_FALSE
    ));
    let x_n = decode_coord(e_int, &p_cell_data[8 + p.i_coord as usize * 4..]);
    match p.op {
        RTREE_TRUE => return,
        RTREE_FALSE => {}
        RTREE_LE => {
            if x_n <= p.r_value {
                return;
            }
        }
        RTREE_LT => {
            if x_n < p.r_value {
                return;
            }
        }
        RTREE_GE => {
            if x_n >= p.r_value {
                return;
            }
        }
        RTREE_GT => {
            if x_n > p.r_value {
                return;
            }
        }
        _ => {
            if x_n == p.r_value {
                return;
            }
        }
    }
    *pe_within = NOT_WITHIN;
}

/// `nodeRowidIndex`: uma das células do nó tem o rowid `i_rowid`; devolve o índice dela.
pub(crate) fn node_rowid_index(
    p_rtree: &Rtree,
    id: NodeId,
    i_rowid: i64,
    pi_index: &mut i32,
) -> i32 {
    let n_cell = p_rtree.n_cell(id);
    debug_assert!(n_cell < 200);
    for ii in 0..n_cell {
        if node_get_rowid(p_rtree, id, ii) == i_rowid {
            *pi_index = ii;
            return SQLITE_OK;
        }
    }
    SQLITE_CORRUPT_VTAB
}

/// `nodeParentIndex`: o índice da célula do pai que aponta para o nó (-1 na raiz).
pub(crate) fn node_parent_index(p_rtree: &Rtree, id: NodeId, pi_index: &mut i32) -> i32 {
    match p_rtree.node(id).p_parent {
        Some(par) => node_rowid_index(p_rtree, par, p_rtree.node(id).i_node, pi_index),
        None => {
            *pi_index = -1;
            SQLITE_OK
        }
    }
}

// ---------------------------------------------------------------------------------------------
// A fila de prioridade da busca
// ---------------------------------------------------------------------------------------------

/// `rtreeSearchPointCompare`: negativo, zero ou positivo se `a` é menor, igual ou maior que `b`.
/// A pontuação é a chave primária; empatadas, o menor `iLevel` vem primeiro, o que dá a busca em
/// profundidade quando todas têm a mesma pontuação.
fn rtree_search_point_compare(a: &RtreeSearchPoint, b: &RtreeSearchPoint) -> i32 {
    if a.r_score < b.r_score {
        return -1;
    }
    if a.r_score > b.r_score {
        return 1;
    }
    if a.i_level < b.i_level {
        return -1;
    }
    if a.i_level > b.i_level {
        return 1;
    }
    0
}

/// `rtreeSearchPointSwap`: troca dois pontos de busca do cursor, e os nós em cache com eles.
fn rtree_search_point_swap(
    db: &mut Connection,
    p_rtree: &mut Rtree,
    p: &mut RtreeCursor,
    i: usize,
    j: usize,
) {
    debug_assert!(i < j);
    p.a_point.swap(i, j);
    let i = i + 1;
    let j = j + 1;
    if i < RTREE_CACHE_SZ {
        if j >= RTREE_CACHE_SZ {
            let n = p.a_node[i].take();
            node_release(db, p_rtree, n);
        } else {
            p.a_node.swap(i, j);
        }
    }
}

/// `rtreeSearchPointFirst`: o ponto de busca de menor pontuação.
pub(crate) fn rtree_search_point_first(p_cur: &RtreeCursor) -> Option<Sp> {
    if p_cur.b_point != 0 {
        Some(Sp::S)
    } else if !p_cur.a_point.is_empty() {
        Some(Sp::Q(0))
    } else {
        None
    }
}

/// `rtreeNodeOfFirstSearchPoint`: o nó do ponto de busca de menor pontuação.
pub(crate) fn rtree_node_of_first_search_point(
    db: &mut Connection,
    p_rtree: &mut Rtree,
    p_cur: &mut RtreeCursor,
    p_rc: &mut i32,
) -> Option<NodeId> {
    let ii = 1 - p_cur.b_point as usize;
    debug_assert!(ii == 0 || ii == 1);
    debug_assert!(p_cur.b_point != 0 || !p_cur.a_point.is_empty());
    if p_cur.a_node[ii].is_none() {
        let id = if ii != 0 { p_cur.a_point[0].id } else { p_cur.s_point.id };
        let mut n = None;
        *p_rc = node_acquire(db, p_rtree, id, None, &mut n);
        p_cur.a_node[ii] = n;
    }
    p_cur.a_node[ii]
}

/// `rtreeEnqueue`: empilha um ponto novo na fila.
fn rtree_enqueue(
    db: &mut Connection,
    p_rtree: &mut Rtree,
    p_cur: &mut RtreeCursor,
    r_score: f64,
    i_level: u8,
) -> usize {
    let mut i = p_cur.a_point.len();
    p_cur.a_point.push(RtreeSearchPoint { r_score, i_level, ..RtreeSearchPoint::default() });
    debug_assert!(i_level as i32 <= RTREE_MAX_DEPTH);
    while i > 0 {
        let j = (i - 1) / 2;
        if rtree_search_point_compare(&p_cur.a_point[i], &p_cur.a_point[j]) >= 0 {
            break;
        }
        rtree_search_point_swap(db, p_rtree, p_cur, j, i);
        i = j;
    }
    i
}

/// `rtreeSearchPointNew`: aloca um ponto de busca novo e devolve a posição dele.
pub(crate) fn rtree_search_point_new(
    db: &mut Connection,
    p_rtree: &mut Rtree,
    p_cur: &mut RtreeCursor,
    r_score: f64,
    i_level: u8,
) -> Sp {
    let p_first = rtree_search_point_first(p_cur).map(|p| *p_cur.point(p));
    p_cur.an_queue[i_level as usize] += 1;
    let better = match p_first {
        None => true,
        Some(f) => f.r_score > r_score || (f.r_score == r_score && f.i_level > i_level),
    };
    if better {
        if p_cur.b_point != 0 {
            let i_new = rtree_enqueue(db, p_rtree, p_cur, r_score, i_level);
            let ii = i_new + 1;
            debug_assert!(ii == 1);
            if ii < RTREE_CACHE_SZ {
                debug_assert!(p_cur.a_node[ii].is_none());
                p_cur.a_node[ii] = p_cur.a_node[0];
            } else {
                let n = p_cur.a_node[0];
                node_release(db, p_rtree, n);
            }
            p_cur.a_node[0] = None;
            p_cur.a_point[i_new] = p_cur.s_point;
        }
        p_cur.s_point.r_score = r_score;
        p_cur.s_point.i_level = i_level;
        p_cur.b_point = 1;
        Sp::S
    } else {
        Sp::Q(rtree_enqueue(db, p_rtree, p_cur, r_score, i_level))
    }
}

/// `rtreeSearchPointPop`: remove o ponto de busca de menor pontuação.
pub(crate) fn rtree_search_point_pop(db: &mut Connection, p_rtree: &mut Rtree, p: &mut RtreeCursor) {
    let i = 1 - p.b_point as usize;
    debug_assert!(i == 0 || i == 1);
    if let Some(n) = p.a_node[i].take() {
        node_release(db, p_rtree, Some(n));
    }
    if p.b_point != 0 {
        p.an_queue[p.s_point.i_level as usize] -= 1;
        p.b_point = 0;
    } else if !p.a_point.is_empty() {
        p.an_queue[p.a_point[0].i_level as usize] -= 1;
        let n = p.a_point.len() - 1;
        p.a_point[0] = p.a_point[n];
        p.a_point.truncate(n);
        if n < RTREE_CACHE_SZ - 1 {
            p.a_node[1] = p.a_node[n + 1];
            p.a_node[n + 1] = None;
        }
        let mut i = 0usize;
        loop {
            let j = i * 2 + 1;
            if j >= n {
                break;
            }
            let k = j + 1;
            if k < n && rtree_search_point_compare(&p.a_point[k], &p.a_point[j]) < 0 {
                if rtree_search_point_compare(&p.a_point[k], &p.a_point[i]) < 0 {
                    rtree_search_point_swap(db, p_rtree, p, i, k);
                    i = k;
                } else {
                    break;
                }
            } else if rtree_search_point_compare(&p.a_point[j], &p.a_point[i]) < 0 {
                rtree_search_point_swap(db, p_rtree, p, i, j);
                i = j;
            } else {
                break;
            }
        }
    }
}

/// `rtreeStepToLeaf`: continua a busca até a frente da fila conter uma entrada própria para virar
/// linha do resultado, ou até a fila esvaziar (a consulta terminou).
pub(crate) fn rtree_step_to_leaf(
    db: &mut Connection,
    p_rtree: &mut Rtree,
    p_cur: &mut RtreeCursor,
) -> i32 {
    let n_constraint = p_cur.a_constraint.len();
    let e_int = p_rtree.e_coord_type == RTREE_COORD_INT32;
    let bpc = p_rtree.n_bytes_per_cell as usize;
    let mut rc = SQLITE_OK;
    let mut p;

    loop {
        p = rtree_search_point_first(p_cur);
        let Some(mut pp) = p else { break };
        if p_cur.point(pp).i_level == 0 {
            break;
        }
        let Some(p_node) = rtree_node_of_first_search_point(db, p_rtree, p_cur, &mut rc) else {
            return if rc != SQLITE_OK { rc } else { SQLITE_CORRUPT_VTAB };
        };
        if rc != SQLITE_OK {
            return rc;
        }
        let n_cell = p_rtree.n_cell(p_node);
        debug_assert!(n_cell < 200);
        let mut cell_off = 4 + bpc * p_cur.point(pp).i_cell as usize;
        while (p_cur.point(pp).i_cell as i32) < n_cell {
            let mut r_score: f64 = -1.0;
            let mut e_within = FULLY_WITHIN;
            let sp = *p_cur.point(pp);
            for ii in 0..n_constraint {
                let p_data = &p_rtree.node(p_node).z_data[cell_off..];
                if p_cur.a_constraint[ii].op >= RTREE_MATCH {
                    rc = rtree_callback_constraint(
                        &mut p_cur.a_constraint[ii],
                        e_int,
                        p_data,
                        &sp,
                        &p_cur.an_queue,
                        &mut r_score,
                        &mut e_within,
                    );
                    if rc != SQLITE_OK {
                        return rc;
                    }
                } else if sp.i_level == 1 {
                    rtree_leaf_constraint(&p_cur.a_constraint[ii], e_int, p_data, &mut e_within);
                } else {
                    rtree_nonleaf_constraint(&p_cur.a_constraint[ii], e_int, p_data, &mut e_within);
                }
                if e_within == NOT_WITHIN {
                    let pt = p_cur.point_mut(pp);
                    pt.i_cell = pt.i_cell.wrapping_add(1);
                    cell_off += bpc;
                    break;
                }
            }
            if e_within == NOT_WITHIN {
                continue;
            }
            {
                let pt = p_cur.point_mut(pp);
                pt.i_cell = pt.i_cell.wrapping_add(1);
            }
            let sp = *p_cur.point(pp);
            let x_level = sp.i_level - 1;
            let x_id;
            let x_cell;
            if x_level != 0 {
                x_id = read_int64(&p_rtree.node(p_node).z_data[cell_off..]);
                if p_cur.a_point.iter().any(|q| q.id == x_id) {
                    return SQLITE_CORRUPT_VTAB;
                }
                x_cell = 0u8;
            } else {
                x_id = sp.id;
                x_cell = sp.i_cell.wrapping_sub(1);
            }
            if (sp.i_cell as i32) >= n_cell {
                rtree_search_point_pop(db, p_rtree, p_cur);
            }
            if r_score < 0.0 {
                r_score = 0.0;
            }
            pp = rtree_search_point_new(db, p_rtree, p_cur, r_score, x_level);
            let pt = p_cur.point_mut(pp);
            pt.e_within = e_within as u8;
            pt.id = x_id;
            pt.i_cell = x_cell;
            break;
        }
        if (p_cur.point(pp).i_cell as i32) >= n_cell {
            rtree_search_point_pop(db, p_rtree, p_cur);
        }
    }
    p_cur.at_eof = p.is_none() as u8;
    if p.is_some() {
        // `xRowid` não recebe a conexão: o nó da entrada em que a busca parou fica em cache já
        // aqui (ver o cabeçalho de `rtree2.rs`).
        let mut rc2 = SQLITE_OK;
        rtree_node_of_first_search_point(db, p_rtree, p_cur, &mut rc2);
        return rc2;
    }
    SQLITE_OK
}

/// `findLeafNode`: usa `nodeAcquire()` para obter a folha que contém o registro de rowid
/// `i_rowid`. Sem o registro, `*pp_leaf` fica nulo e o resultado é `SQLITE_OK`.
pub(crate) fn find_leaf_node(
    db: &mut Connection,
    p_rtree: &mut Rtree,
    i_rowid: i64,
    pp_leaf: &mut Option<NodeId>,
    pi_node: Option<&mut i64>,
) -> i32 {
    *pp_leaf = None;
    let p_read = p_rtree.p_read_rowid.unwrap_or_default();
    bind_int64(db, p_read, 1, i_rowid);
    if step(db, p_read) == SQLITE_ROW {
        let i_node = column_int64(db, p_read, 0);
        if let Some(out) = pi_node {
            *out = i_node;
        }
        let rc = node_acquire(db, p_rtree, i_node, None, pp_leaf);
        reset(db, p_read);
        rc
    } else {
        reset(db, p_read)
    }
}

/// `deserializeGeometry`: configura a restrição `p_cons` de um MATCH. `p_value` é o operando
/// direito do MATCH.
pub(crate) fn deserialize_geometry(p_value: &Mem, p_cons: &mut RtreeConstraint) -> i32 {
    let Some(p_src) = crate::mem2::value_pointer(p_value, b"RtreeMatchArg")
        .and_then(|a| a.downcast_ref::<RtreeMatchArg>())
    else {
        return SQLITE_ERROR;
    };
    let p_info = RtreeQueryInfo {
        base: RtreeGeometry {
            p_context: p_src.cb.p_context.clone(),
            n_param: p_src.n_param,
            a_param: p_src.a_param.clone(),
            p_user: None,
        },
        ap_sql_param: p_src.ap_sql_param.clone(),
        ..RtreeQueryInfo::default()
    };
    if p_src.cb.x_geom.is_some() {
        p_cons.x_geom = p_src.cb.x_geom;
    } else {
        p_cons.op = RTREE_QUERY;
        p_cons.x_query_func = p_src.cb.x_query_func;
    }
    p_cons.p_info = Some(Box::new(p_info));
    SQLITE_OK
}

/// `struct RtreeGeomCallback`: o `sqlite3_user_data()` das funções SQL criadas por
/// `sqlite3_rtree_geometry_callback()` e `sqlite3_rtree_query_callback()`. Exatamente um entre
/// `x_geom` e `x_query_func` é não nulo. O `xDestructor` é o `x_destroy` de
/// `create_function_api` (ver `rtree2.rs`).
#[derive(Clone)]
pub struct RtreeGeomCallback {
    /// Callback de geometria (estilo antigo).
    pub x_geom: Option<RtreeGeomFn>,
    /// Callback de consulta (estilo novo).
    pub x_query_func: Option<RtreeQueryFn>,
    /// O `pContext` que o usuário passou.
    pub p_context: Option<Rc<dyn Any>>,
}

/// `struct RtreeMatchArg`: o valor ponteiro `"RtreeMatchArg"` que as funções SQL de geometria
/// devolvem e que é lido como operando direito do MATCH.
pub(crate) struct RtreeMatchArg {
    /// Informação sobre os callbacks.
    pub(crate) cb: RtreeGeomCallback,
    /// Número de parâmetros da função SQL.
    pub(crate) n_param: i32,
    /// Os valores SQL originais dos parâmetros.
    pub(crate) ap_sql_param: Rc<Vec<Mem>>,
    /// Os valores dos parâmetros como `double`.
    pub(crate) a_param: Vec<f64>,
}

// ---------------------------------------------------------------------------------------------
// Geometria das células
// ---------------------------------------------------------------------------------------------

/// `cellArea`: o volume N-dimensional da célula.
pub(crate) fn cell_area(p_rtree: &Rtree, p: &RtreeCell) -> f64 {
    let n_dim = p_rtree.n_dim as usize;
    debug_assert!((1..=5).contains(&n_dim));
    let mut area = 1.0f64;
    if p_rtree.e_coord_type == RTREE_COORD_REAL32 {
        let diff = |hi: usize, lo: usize| (p.a_coord[hi].f() - p.a_coord[lo].f()) as f64;
        if n_dim == 5 {
            area = diff(9, 8);
        }
        for k in (0..n_dim.min(4)).rev() {
            area *= diff(2 * k + 1, 2 * k);
        }
    } else {
        let diff = |hi: usize, lo: usize| (p.a_coord[hi].i() as i64 - p.a_coord[lo].i() as i64) as f64;
        if n_dim == 5 {
            area = diff(9, 8);
        }
        for k in (0..n_dim.min(4)).rev() {
            area *= diff(2 * k + 1, 2 * k);
        }
    }
    area
}

/// `cellMargin`: o comprimento da margem da célula, a soma dos tamanhos em cada dimensão.
fn cell_margin(p_rtree: &Rtree, p: &RtreeCell) -> f64 {
    let t = p_rtree.e_coord_type;
    let mut margin = 0.0f64;
    let mut ii = p_rtree.n_dim2 as i32 - 2;
    loop {
        margin += p.a_coord[ii as usize + 1].d(t) - p.a_coord[ii as usize].d(t);
        ii -= 2;
        if ii < 0 {
            break;
        }
    }
    margin
}

/// `cellUnion`: guarda em `p1` a união das células `p1` e `p2`.
pub(crate) fn cell_union(p_rtree: &Rtree, p1: &mut RtreeCell, p2: &RtreeCell) {
    let mut ii = 0usize;
    if p_rtree.e_coord_type == RTREE_COORD_REAL32 {
        loop {
            let (a, b) = (p1.a_coord[ii].f(), p2.a_coord[ii].f());
            p1.a_coord[ii] = RtreeCoord::from_f(if a > b { b } else { a });
            let (a, b) = (p1.a_coord[ii + 1].f(), p2.a_coord[ii + 1].f());
            p1.a_coord[ii + 1] = RtreeCoord::from_f(if a < b { b } else { a });
            ii += 2;
            if ii >= p_rtree.n_dim2 as usize {
                break;
            }
        }
    } else {
        loop {
            let (a, b) = (p1.a_coord[ii].i(), p2.a_coord[ii].i());
            p1.a_coord[ii] = RtreeCoord::from_i(if a > b { b } else { a });
            let (a, b) = (p1.a_coord[ii + 1].i(), p2.a_coord[ii + 1].i());
            p1.a_coord[ii + 1] = RtreeCoord::from_i(if a < b { b } else { a });
            ii += 2;
            if ii >= p_rtree.n_dim2 as usize {
                break;
            }
        }
    }
}

/// `cellContains`: verdadeiro se a área de `p2` é um subconjunto da de `p1`.
pub(crate) fn cell_contains(p_rtree: &Rtree, p1: &RtreeCell, p2: &RtreeCell) -> bool {
    let n_dim2 = p_rtree.n_dim2 as usize;
    if p_rtree.e_coord_type == RTREE_COORD_INT32 {
        for ii in (0..n_dim2).step_by(2) {
            if p2.a_coord[ii].i() < p1.a_coord[ii].i() || p2.a_coord[ii + 1].i() > p1.a_coord[ii + 1].i() {
                return false;
            }
        }
    } else {
        for ii in (0..n_dim2).step_by(2) {
            if p2.a_coord[ii].f() < p1.a_coord[ii].f() || p2.a_coord[ii + 1].f() > p1.a_coord[ii + 1].f() {
                return false;
            }
        }
    }
    true
}

/// `cellOverlap`.
fn cell_overlap(p_rtree: &Rtree, p: &RtreeCell, a_cell: &[RtreeCell]) -> f64 {
    let t = p_rtree.e_coord_type;
    let mut overlap = 0.0f64;
    for cell in a_cell {
        let mut o = 1.0f64;
        for jj in (0..p_rtree.n_dim2 as usize).step_by(2) {
            let (a, b) = (p.a_coord[jj].d(t), cell.a_coord[jj].d(t));
            let x1 = if a < b { b } else { a };
            let (a, b) = (p.a_coord[jj + 1].d(t), cell.a_coord[jj + 1].d(t));
            let x2 = if a > b { b } else { a };
            if x2 < x1 {
                o = 0.0;
                break;
            } else {
                o *= x2 - x1;
            }
        }
        overlap += o;
    }
    overlap
}

/// `ChooseLeaf`: o algoritmo ChooseLeaf de Gutman[84] (ChooseSubTree na terminologia R*-tree).
pub(crate) fn choose_leaf(
    db: &mut Connection,
    p_rtree: &mut Rtree,
    p_cell: &RtreeCell,
    i_height: i32,
    pp_leaf: &mut Option<NodeId>,
) -> i32 {
    let mut p_node: Option<NodeId> = None;
    let mut rc = node_acquire(db, p_rtree, 1, None, &mut p_node);

    let mut ii = 0;
    while rc == SQLITE_OK && ii < (p_rtree.i_depth - i_height) {
        let Some(node) = p_node else { break };
        let mut i_best: i64 = 0;
        let mut b_found = false;
        let mut f_min_growth = 0.0f64;
        let mut f_min_area = 0.0f64;
        let n_cell = p_rtree.n_cell(node);
        let mut p_child: Option<NodeId> = None;

        // Primeiro vê se alguma célula do nó contém `pCell` por inteiro. Se duas ou mais contêm,
        // escolhe a menor.
        for i_cell in 0..n_cell {
            let cell = node_get_cell(p_rtree, node, i_cell);
            if cell_contains(p_rtree, &cell, p_cell) {
                let area = cell_area(p_rtree, &cell);
                if !b_found || area < f_min_area {
                    i_best = cell.i_rowid;
                    f_min_area = area;
                    b_found = true;
                }
            }
        }
        if !b_found {
            // Nenhuma célula contém `pCell`: escolhe a que cresce menos ao recebê-lo, desempatando
            // pela menor.
            for i_cell in 0..n_cell {
                let mut cell = node_get_cell(p_rtree, node, i_cell);
                let area = cell_area(p_rtree, &cell);
                cell_union(p_rtree, &mut cell, p_cell);
                let growth = cell_area(p_rtree, &cell) - area;
                if i_cell == 0
                    || growth < f_min_growth
                    || (growth == f_min_growth && area < f_min_area)
                {
                    f_min_growth = growth;
                    f_min_area = area;
                    i_best = cell.i_rowid;
                }
            }
        }

        rc = node_acquire(db, p_rtree, i_best, Some(node), &mut p_child);
        node_release(db, p_rtree, Some(node));
        p_node = p_child;
        ii += 1;
    }

    *pp_leaf = p_node;
    rc
}

/// `AdjustTree`: uma célula igual a `p_cell` acaba de entrar no nó; atualiza as caixas
/// envolventes de todos os ancestrais.
pub(crate) fn adjust_tree(p_rtree: &mut Rtree, p_node: NodeId, p_cell: &RtreeCell) -> i32 {
    let mut p = p_node;
    let mut cnt = 0;
    while let Some(p_parent) = p_rtree.node(p).p_parent {
        cnt += 1;
        if cnt > 100 {
            return SQLITE_CORRUPT_VTAB;
        }
        let mut i_cell = 0;
        let rc = node_parent_index(p_rtree, p, &mut i_cell);
        if rc != SQLITE_OK {
            return SQLITE_CORRUPT_VTAB;
        }

        let mut cell = node_get_cell(p_rtree, p_parent, i_cell);
        if !cell_contains(p_rtree, &cell, p_cell) {
            cell_union(p_rtree, &mut cell, p_cell);
            node_overwrite_cell(p_rtree, p_parent, &cell, i_cell);
        }

        p = p_parent;
    }
    SQLITE_OK
}

/// `rowidWrite`: grava o mapa (rowid->nó) em `<rtree>_rowid`.
pub(crate) fn rowid_write(db: &mut Connection, p_rtree: &Rtree, i_rowid: i64, i_node: i64) -> i32 {
    let p = p_rtree.p_write_rowid.unwrap_or_default();
    bind_int64(db, p, 1, i_rowid);
    bind_int64(db, p, 2, i_node);
    step(db, p);
    reset(db, p)
}

/// `parentWrite`: grava o mapa (nó->pai) em `<rtree>_parent`.
pub(crate) fn parent_write(db: &mut Connection, p_rtree: &Rtree, i_node: i64, i_par: i64) -> i32 {
    let p = p_rtree.p_write_parent.unwrap_or_default();
    bind_int64(db, p, 1, i_node);
    bind_int64(db, p, 2, i_par);
    step(db, p);
    reset(db, p)
}

/// `SortByDimension`: ordena os índices de `a_idx` pela dimensão `i_dim` das células de `a_cell`.
/// O mínimo da dimensão vem primeiro, o máximo desempata. `a_spare` é espaço de trabalho.
fn sort_by_dimension(
    p_rtree: &Rtree,
    a_idx: &mut [usize],
    i_dim: usize,
    a_cell: &[RtreeCell],
    a_spare: &mut [usize],
) {
    let n_idx = a_idx.len();
    if n_idx > 1 {
        let t = p_rtree.e_coord_type;
        let mut i_left = 0usize;
        let mut i_right = 0usize;

        let n_left = n_idx / 2;
        let n_right = n_idx - n_left;
        {
            let (l, r) = a_idx.split_at_mut(n_left);
            sort_by_dimension(p_rtree, l, i_dim, a_cell, a_spare);
            sort_by_dimension(p_rtree, r, i_dim, a_cell, a_spare);
        }

        a_spare[..n_left].copy_from_slice(&a_idx[..n_left]);
        while i_left < n_left || i_right < n_right {
            let xleft1 = a_cell[a_spare[i_left.min(n_left - 1)]].a_coord[i_dim * 2].d(t);
            let xleft2 = a_cell[a_spare[i_left.min(n_left - 1)]].a_coord[i_dim * 2 + 1].d(t);
            let right = a_idx[n_left + i_right.min(n_right - 1)];
            let xright1 = a_cell[right].a_coord[i_dim * 2].d(t);
            let xright2 = a_cell[right].a_coord[i_dim * 2 + 1].d(t);
            if i_left != n_left
                && (i_right == n_right
                    || xleft1 < xright1
                    || (xleft1 == xright1 && xleft2 < xright2))
            {
                a_idx[i_left + i_right] = a_spare[i_left];
                i_left += 1;
            } else {
                a_idx[i_left + i_right] = right;
                i_right += 1;
            }
        }
    }
}

/// `splitNodeStartree`: a variante R*-tree de SplitNode de Beckman[1990].
fn split_node_startree(
    p_rtree: &mut Rtree,
    a_cell: &[RtreeCell],
    p_left: NodeId,
    p_right: NodeId,
    p_bbox_left: &mut RtreeCell,
    p_bbox_right: &mut RtreeCell,
) -> i32 {
    let n_cell = a_cell.len();
    let n_dim = p_rtree.n_dim as usize;
    let min_cells = p_rtree.min_cells();

    let mut a_sorted: Vec<Vec<usize>> = Vec::with_capacity(n_dim);
    let mut a_spare = vec![0usize; n_cell];
    for ii in 0..n_dim {
        let mut v: Vec<usize> = (0..n_cell).collect();
        sort_by_dimension(p_rtree, &mut v, ii, a_cell, &mut a_spare);
        a_sorted.push(v);
    }

    let mut i_best_dim = 0usize;
    let mut i_best_split = 0usize;
    let mut f_best_margin = 0.0f64;

    for ii in 0..n_dim {
        let mut margin = 0.0f64;
        let mut f_best_overlap = 0.0f64;
        let mut f_best_area = 0.0f64;
        let mut i_best_left = 0usize;

        let mut n_left = min_cells;
        while n_left <= (n_cell as i32 - min_cells) {
            let mut left = a_cell[a_sorted[ii][0]];
            let mut right = a_cell[a_sorted[ii][n_cell - 1]];
            for kk in 1..(n_cell - 1) {
                if (kk as i32) < n_left {
                    cell_union(p_rtree, &mut left, &a_cell[a_sorted[ii][kk]]);
                } else {
                    cell_union(p_rtree, &mut right, &a_cell[a_sorted[ii][kk]]);
                }
            }
            margin += cell_margin(p_rtree, &left);
            margin += cell_margin(p_rtree, &right);
            let overlap = cell_overlap(p_rtree, &left, std::slice::from_ref(&right));
            let area = cell_area(p_rtree, &left) + cell_area(p_rtree, &right);
            if n_left == min_cells
                || overlap < f_best_overlap
                || (overlap == f_best_overlap && area < f_best_area)
            {
                i_best_left = n_left as usize;
                f_best_overlap = overlap;
                f_best_area = area;
            }
            n_left += 1;
        }

        if ii == 0 || margin < f_best_margin {
            i_best_dim = ii;
            f_best_margin = margin;
            i_best_split = i_best_left;
        }
    }

    *p_bbox_left = a_cell[a_sorted[i_best_dim][0]];
    *p_bbox_right = a_cell[a_sorted[i_best_dim][i_best_split]];
    for ii in 0..n_cell {
        let p_target = if ii < i_best_split { p_left } else { p_right };
        let p_cell = &a_cell[a_sorted[i_best_dim][ii]];
        node_insert_cell(p_rtree, p_target, p_cell);
        if ii < i_best_split {
            cell_union(p_rtree, p_bbox_left, p_cell);
        } else {
            cell_union(p_rtree, p_bbox_right, p_cell);
        }
    }

    SQLITE_OK
}

/// `updateMapping`.
fn update_mapping(
    db: &mut Connection,
    p_rtree: &mut Rtree,
    i_rowid: i64,
    p_node: NodeId,
    i_height: i32,
) -> i32 {
    if i_height > 0 {
        let p_child = node_hash_lookup(p_rtree, i_rowid);
        let mut p = Some(p_node);
        while let Some(q) = p {
            if Some(q) == p_child {
                return SQLITE_CORRUPT_VTAB;
            }
            p = p_rtree.node(q).p_parent;
        }
        if let Some(child) = p_child {
            let old_parent = p_rtree.node(child).p_parent;
            node_release(db, p_rtree, old_parent);
            node_reference(p_rtree, Some(p_node));
            p_rtree.node_mut(child).p_parent = Some(p_node);
        }
    }
    let i_node = p_rtree.node(p_node).i_node;
    if i_height == 0 {
        rowid_write(db, p_rtree, i_rowid, i_node)
    } else {
        parent_write(db, p_rtree, i_rowid, i_node)
    }
}

/// `SplitNode`.
fn split_node(
    db: &mut Connection,
    p_rtree: &mut Rtree,
    p_node: NodeId,
    p_cell: &RtreeCell,
    i_height: i32,
) -> i32 {
    let mut new_cell_is_right = false;
    let mut rc;
    let n_cell = p_rtree.n_cell(p_node);
    let mut p_left: Option<NodeId> = None;
    let mut p_right: Option<NodeId> = None;
    let mut leftbbox = RtreeCell::default();
    let mut rightbbox = RtreeCell::default();

    'out: {
        // Copia `pCell` e todas as células do nó para um vetor e zera o nó original.
        let mut a_cell: Vec<RtreeCell> = Vec::with_capacity(n_cell as usize + 1);
        for i in 0..n_cell {
            a_cell.push(node_get_cell(p_rtree, p_node, i));
        }
        node_zero(p_rtree, p_node);
        a_cell.push(*p_cell);

        if p_rtree.node(p_node).i_node == 1 {
            p_right = Some(node_new(p_rtree, Some(p_node)));
            p_left = Some(node_new(p_rtree, Some(p_node)));
            p_rtree.i_depth += 1;
            let depth = p_rtree.i_depth;
            let node = p_rtree.node_mut(p_node);
            node.is_dirty = true;
            write_int16(&mut node.z_data, depth);
        } else {
            p_left = Some(p_node);
            let par = p_rtree.node(p_node).p_parent;
            p_right = Some(node_new(p_rtree, par));
            p_rtree.node_mut(p_node).n_ref += 1;
        }
        let (Some(left), Some(right)) = (p_left, p_right) else {
            rc = SQLITE_NOMEM;
            break 'out;
        };

        p_rtree.node_mut(left).z_data.fill(0);
        p_rtree.node_mut(right).z_data.fill(0);

        rc = split_node_startree(p_rtree, &a_cell, left, right, &mut leftbbox, &mut rightbbox);
        if rc != SQLITE_OK {
            break 'out;
        }

        // Garante que os dois nós filhos têm número, chamando `nodeWrite()`. O da direita sempre
        // precisa; o da esquerda às vezes já tem, e então evita-se a chamada.
        rc = node_write(db, p_rtree, right);
        if rc != SQLITE_OK {
            break 'out;
        }
        if p_rtree.node(left).i_node == 0 {
            rc = node_write(db, p_rtree, left);
            if rc != SQLITE_OK {
                break 'out;
            }
        }

        rightbbox.i_rowid = p_rtree.node(right).i_node;
        leftbbox.i_rowid = p_rtree.node(left).i_node;

        if p_rtree.node(p_node).i_node == 1 {
            let Some(par) = p_rtree.node(left).p_parent else {
                rc = SQLITE_CORRUPT_VTAB;
                break 'out;
            };
            rc = rtree_insert_cell(db, p_rtree, par, &leftbbox, i_height + 1);
            if rc != SQLITE_OK {
                break 'out;
            }
        } else {
            let Some(p_parent) = p_rtree.node(left).p_parent else {
                rc = SQLITE_CORRUPT_VTAB;
                break 'out;
            };
            let mut i_cell = 0;
            rc = node_parent_index(p_rtree, left, &mut i_cell);
            if rc == SQLITE_OK {
                node_overwrite_cell(p_rtree, p_parent, &leftbbox, i_cell);
                rc = adjust_tree(p_rtree, p_parent, &leftbbox);
                debug_assert!(rc == SQLITE_OK);
            }
            if rc != SQLITE_OK {
                break 'out;
            }
        }
        let Some(right_parent) = p_rtree.node(right).p_parent else {
            rc = SQLITE_CORRUPT_VTAB;
            break 'out;
        };
        rc = rtree_insert_cell(db, p_rtree, right_parent, &rightbbox, i_height + 1);
        if rc != SQLITE_OK {
            break 'out;
        }

        let mut i = 0;
        while i < p_rtree.n_cell(right) {
            let i_rowid = node_get_rowid(p_rtree, right, i);
            rc = update_mapping(db, p_rtree, i_rowid, right, i_height);
            if i_rowid == p_cell.i_rowid {
                new_cell_is_right = true;
            }
            if rc != SQLITE_OK {
                break 'out;
            }
            i += 1;
        }
        if p_rtree.node(p_node).i_node == 1 {
            let mut i = 0;
            while i < p_rtree.n_cell(left) {
                let i_rowid = node_get_rowid(p_rtree, left, i);
                rc = update_mapping(db, p_rtree, i_rowid, left, i_height);
                if rc != SQLITE_OK {
                    break 'out;
                }
                i += 1;
            }
        } else if !new_cell_is_right {
            rc = update_mapping(db, p_rtree, p_cell.i_rowid, left, i_height);
        }

        if rc == SQLITE_OK {
            rc = node_release(db, p_rtree, p_right);
            p_right = None;
        }
        if rc == SQLITE_OK {
            rc = node_release(db, p_rtree, p_left);
            p_left = None;
        }
    }

    node_release(db, p_rtree, p_right);
    node_release(db, p_rtree, p_left);
    rc
}

/// `fixLeafParent`: se a folha não é a raiz e o `pParent` dela ainda é nulo, carrega na memória
/// todos os ancestrais e monta a cadeia `pParent` até a raiz. Precisa disso quando uma linha é
/// apagada (ou atualizada, que é apagar e inserir): o SQLite dá o rowid, que acha a folha, e esta
/// função descobre a ascendência dela.
fn fix_leaf_parent(db: &mut Connection, p_rtree: &mut Rtree, p_leaf: NodeId) -> i32 {
    let mut rc = SQLITE_OK;
    let mut p_child = p_leaf;
    while rc == SQLITE_OK
        && p_rtree.node(p_child).i_node != 1
        && p_rtree.node(p_child).p_parent.is_none()
    {
        let mut rc2 = SQLITE_OK;
        let p_read = p_rtree.p_read_parent.unwrap_or_default();
        bind_int64(db, p_read, 1, p_rtree.node(p_child).i_node);
        rc = step(db, p_read);
        if rc == SQLITE_ROW {
            // Antes de pôr `pChild->pParent`, confere que não se cria um laço de referências (como
            // aconteceria se `pChild==pParent`), que vazaria memória ao apagar os nós.
            let i_node = column_int64(db, p_read, 0);
            let mut p_test = Some(p_leaf);
            while let Some(t) = p_test {
                if p_rtree.node(t).i_node == i_node {
                    break;
                }
                p_test = p_rtree.node(t).p_parent;
            }
            if p_test.is_none() {
                let mut par = None;
                rc2 = node_acquire(db, p_rtree, i_node, None, &mut par);
                p_rtree.node_mut(p_child).p_parent = par;
            }
        }
        rc = reset(db, p_read);
        if rc == SQLITE_OK {
            rc = rc2;
        }
        if rc == SQLITE_OK && p_rtree.node(p_child).p_parent.is_none() {
            rc = SQLITE_CORRUPT_VTAB;
        }
        match p_rtree.node(p_child).p_parent {
            Some(p) => p_child = p,
            None => break,
        }
    }
    rc
}

/// `removeNode`.
fn remove_node(db: &mut Connection, p_rtree: &mut Rtree, p_node: NodeId, i_height: i32) -> i32 {
    let mut p_parent: Option<NodeId> = None;
    let mut i_cell = 0;

    debug_assert!(p_rtree.node(p_node).n_ref == 1);

    // Remove a entrada na célula do pai.
    let mut rc = node_parent_index(p_rtree, p_node, &mut i_cell);
    if rc == SQLITE_OK {
        p_parent = p_rtree.node(p_node).p_parent;
        p_rtree.node_mut(p_node).p_parent = None;
        rc = match p_parent {
            Some(par) => delete_cell(db, p_rtree, par, i_cell, i_height + 1),
            None => SQLITE_CORRUPT_VTAB,
        };
    }
    let rc2 = node_release(db, p_rtree, p_parent);
    if rc == SQLITE_OK {
        rc = rc2;
    }
    if rc != SQLITE_OK {
        return rc;
    }

    // Remove a entrada de xxx_node.
    let i_node = p_rtree.node(p_node).i_node;
    let p_del = p_rtree.p_delete_node.unwrap_or_default();
    bind_int64(db, p_del, 1, i_node);
    step(db, p_del);
    rc = reset(db, p_del);
    if rc != SQLITE_OK {
        return rc;
    }

    // Remove a entrada de xxx_parent.
    let p_del = p_rtree.p_delete_parent.unwrap_or_default();
    bind_int64(db, p_del, 1, i_node);
    step(db, p_del);
    rc = reset(db, p_del);
    if rc != SQLITE_OK {
        return rc;
    }

    // Tira o nó da tabela hash em memória e o liga à lista `Rtree.pDeleted`; o conteúdo dele será
    // reinserido depois.
    node_hash_delete(p_rtree, p_node);
    let deleted = p_rtree.p_deleted;
    let node = p_rtree.node_mut(p_node);
    node.i_node = i_height as i64;
    node.p_next = deleted;
    node.n_ref += 1;
    p_rtree.p_deleted = Some(p_node);

    SQLITE_OK
}

/// `fixBoundingBox`.
fn fix_bounding_box(p_rtree: &mut Rtree, p_node: NodeId) -> i32 {
    let mut rc = SQLITE_OK;
    if let Some(p_parent) = p_rtree.node(p_node).p_parent {
        let n_cell = p_rtree.n_cell(p_node);
        // A caixa envolvente do nó.
        let mut bbox = node_get_cell(p_rtree, p_node, 0);
        for ii in 1..n_cell {
            let cell = node_get_cell(p_rtree, p_node, ii);
            cell_union(p_rtree, &mut bbox, &cell);
        }
        bbox.i_rowid = p_rtree.node(p_node).i_node;
        let mut ii = 0;
        rc = node_parent_index(p_rtree, p_node, &mut ii);
        if rc == SQLITE_OK {
            node_overwrite_cell(p_rtree, p_parent, &bbox, ii);
            rc = fix_bounding_box(p_rtree, p_parent);
        }
    }
    rc
}

/// `deleteCell`: apaga a célula `i_cell` do nó e ajusta a estrutura se for preciso.
fn delete_cell(
    db: &mut Connection,
    p_rtree: &mut Rtree,
    p_node: NodeId,
    i_cell: i32,
    i_height: i32,
) -> i32 {
    let mut rc = fix_leaf_parent(db, p_rtree, p_node);
    if rc != SQLITE_OK {
        return rc;
    }

    // Tira a célula do nó. Só move bytes na imagem em memória, então não pode falhar.
    node_delete_cell(p_rtree, p_node, i_cell);

    // Se o nó não é a raiz e ficou com menos que o mínimo de células, sai da árvore. Senão a
    // célula do pai passa a envolvê-lo com justeza.
    let p_parent = p_rtree.node(p_node).p_parent;
    debug_assert!(p_parent.is_some() || p_rtree.node(p_node).i_node == 1);
    if p_parent.is_some() {
        if p_rtree.n_cell(p_node) < p_rtree.min_cells() {
            rc = remove_node(db, p_rtree, p_node, i_height);
        } else {
            rc = fix_bounding_box(p_rtree, p_node);
        }
    }

    rc
}

/// `rtreeInsertCell`: insere a célula no nó, que é a cabeça de uma subárvore de altura `i_height`
/// (as folhas têm `i_height==0`).
pub(crate) fn rtree_insert_cell(
    db: &mut Connection,
    p_rtree: &mut Rtree,
    p_node: NodeId,
    p_cell: &RtreeCell,
    i_height: i32,
) -> i32 {
    let mut rc;
    if i_height > 0 {
        if let Some(p_child) = node_hash_lookup(p_rtree, p_cell.i_rowid) {
            let old = p_rtree.node(p_child).p_parent;
            node_release(db, p_rtree, old);
            node_reference(p_rtree, Some(p_node));
            p_rtree.node_mut(p_child).p_parent = Some(p_node);
        }
    }
    if node_insert_cell(p_rtree, p_node, p_cell) != 0 {
        rc = split_node(db, p_rtree, p_node, p_cell, i_height);
    } else {
        rc = adjust_tree(p_rtree, p_node, p_cell);
        if rc == SQLITE_OK {
            let i_node = p_rtree.node(p_node).i_node;
            if i_height == 0 {
                rc = rowid_write(db, p_rtree, p_cell.i_rowid, i_node);
            } else {
                rc = parent_write(db, p_rtree, p_cell.i_rowid, i_node);
            }
        }
    }
    rc
}

/// `reinsertNodeContent`.
fn reinsert_node_content(db: &mut Connection, p_rtree: &mut Rtree, p_node: NodeId) -> i32 {
    let mut rc = SQLITE_OK;
    let n_cell = p_rtree.n_cell(p_node);

    let mut ii = 0;
    while rc == SQLITE_OK && ii < n_cell {
        let cell = node_get_cell(p_rtree, p_node, ii);

        // Acha um nó para guardar a célula. `pNode->iNode` guarda agora a altura da subárvore
        // encabeçada pela célula.
        let i_height = p_rtree.node(p_node).i_node as i32;
        let mut p_insert: Option<NodeId> = None;
        rc = choose_leaf(db, p_rtree, &cell, i_height, &mut p_insert);
        if rc == SQLITE_OK {
            let Some(ins) = p_insert else {
                rc = SQLITE_CORRUPT_VTAB;
                break;
            };
            rc = rtree_insert_cell(db, p_rtree, ins, &cell, i_height);
            let rc2 = node_release(db, p_rtree, Some(ins));
            if rc == SQLITE_OK {
                rc = rc2;
            }
        }
        ii += 1;
    }
    rc
}

/// `rtreeNewRowid`: escolhe um rowid não usado para um registro novo.
pub(crate) fn rtree_new_rowid(db: &mut Connection, p_rtree: &Rtree, pi_rowid: &mut i64) -> i32 {
    let p = p_rtree.p_write_rowid.unwrap_or_default();
    bind_null(db, p, 1);
    bind_null(db, p, 2);
    step(db, p);
    let rc = reset(db, p);
    *pi_rowid = crate::main::last_insert_rowid(db);
    rc
}

/// `rtreeDeleteRowid`: tira da estrutura o registro de rowid `i_delete`.
pub(crate) fn rtree_delete_rowid(db: &mut Connection, p_rtree: &mut Rtree, i_delete: i64) -> i32 {
    let mut p_leaf: Option<NodeId> = None;
    let mut p_root: Option<NodeId> = None;

    // Pega uma referência à raiz para inicializar `Rtree.iDepth`.
    let mut rc = node_acquire(db, p_rtree, 1, None, &mut p_root);

    // Pega uma referência à folha que contém a entrada a apagar.
    if rc == SQLITE_OK {
        rc = find_leaf_node(db, p_rtree, i_delete, &mut p_leaf, None);
    }

    // Apaga a célula da folha.
    if rc == SQLITE_OK {
        if let Some(leaf) = p_leaf {
            let mut i_cell = 0;
            rc = node_rowid_index(p_rtree, leaf, i_delete, &mut i_cell);
            if rc == SQLITE_OK {
                rc = delete_cell(db, p_rtree, leaf, i_cell, 0);
            }
            let rc2 = node_release(db, p_rtree, Some(leaf));
            if rc == SQLITE_OK {
                rc = rc2;
            }
        }
    }

    // Apaga a entrada correspondente de `<rtree>_rowid`.
    if rc == SQLITE_OK {
        let p = p_rtree.p_delete_rowid.unwrap_or_default();
        bind_int64(db, p, 1, i_delete);
        step(db, p);
        rc = reset(db, p);
    }

    // Se a raiz tem agora exatamente um filho, remove-o, agenda o conteúdo dele para reinserção e
    // reduz a altura da árvore em um. Equivale a copiar o conteúdo do filho para a raiz (a
    // operação que o artigo de Gutman manda fazer).
    if rc == SQLITE_OK && p_rtree.i_depth > 0 {
        if let Some(root) = p_root {
            if p_rtree.n_cell(root) == 1 {
                let mut p_child: Option<NodeId> = None;
                let i_child = node_get_rowid(p_rtree, root, 0);
                rc = node_acquire(db, p_rtree, i_child, Some(root), &mut p_child);
                if rc == SQLITE_OK {
                    if let Some(ch) = p_child {
                        let depth = p_rtree.i_depth - 1;
                        rc = remove_node(db, p_rtree, ch, depth);
                    }
                }
                let rc2 = node_release(db, p_rtree, p_child);
                if rc == SQLITE_OK {
                    rc = rc2;
                }
                if rc == SQLITE_OK {
                    p_rtree.i_depth -= 1;
                    let depth = p_rtree.i_depth;
                    let node = p_rtree.node_mut(root);
                    write_int16(&mut node.z_data, depth);
                    node.is_dirty = true;
                }
            }
        }
    }

    // Reinsere o conteúdo dos nós incompletos que saíram da árvore.
    while let Some(leaf) = p_rtree.p_deleted {
        if rc == SQLITE_OK {
            rc = reinsert_node_content(db, p_rtree, leaf);
        }
        p_rtree.p_deleted = p_rtree.node(leaf).p_next;
        p_rtree.n_node_ref -= 1;
        p_rtree.node_free(leaf);
    }

    // Solta a referência à raiz.
    if rc == SQLITE_OK {
        rc = node_release(db, p_rtree, p_root);
    } else {
        node_release(db, p_rtree, p_root);
    }

    rc
}

/// `rtreeValueDown`: converte um valor SQL em `float` arredondando para o infinito negativo.
pub(crate) fn rtree_value_down(v: &Mem) -> f32 {
    let d = value_double(v);
    let mut f = d as f32;
    if f as f64 > d {
        f = (d * (if d < 0.0 { RNDAWAY } else { RNDTOWARDS })) as f32;
    }
    f
}

/// `rtreeValueUp`: converte um valor SQL em `float` arredondando para o infinito positivo.
pub(crate) fn rtree_value_up(v: &Mem) -> f32 {
    let d = value_double(v);
    let mut f = d as f32;
    if (f as f64) < d {
        f = (d * (if d < 0.0 { RNDTOWARDS } else { RNDAWAY })) as f32;
    }
    f
}
