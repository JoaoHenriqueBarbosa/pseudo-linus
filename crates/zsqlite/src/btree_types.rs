//! Tipos de btreeInt.h e de btree.h no modelo v2 (CONVENTIONS.md, itens 1 a 4): `MemPage`,
//! `BtLock`, `Btree`, `BtShared`, `CellInfo`, `BtCursor`, `IntegrityCk`, `BtreePayload` e o
//! `Slab` dos cursores. As constantes (`PTF_*`, `BTCF_*`, `CURSOR_*`, `BTS_*`, `TRANS_*`,
//! `PTRMAP_*`, `BTREE_*`, `BTALLOC_*`, `BTCURSOR_MAX_DEPTH`, `SQLITE_N_BTREE_META`) vivem em
//! `crate::consts` (`btree_int.rs` e `btree.rs`); `get2byte`, `put2byte`, `get4byte` e `put4byte`
//! em `crate::util`.
//!
//! CAMPOS:
//!
//! `MemPage` (só metadados decodificados; vive no `extra` do slot do `Pager<MemPage>`):
//!   geom: PageGeom                 (novo) cópia de pageSize, usableSize, maxLocal... do BtShared
//!   is_init: bool                  isInit
//!   int_key: bool                  intKey
//!   int_key_leaf: bool             intKeyLeaf
//!   pgno: u32                      pgno
//!   leaf: bool                     leaf
//!   hdr_offset: u8                 hdrOffset
//!   child_ptr_size: u8             childPtrSize
//!   max1byte_payload: u8           max1bytePayload
//!   n_overflow: u8                 nOverflow
//!   max_local: u16                 maxLocal
//!   min_local: u16                 minLocal
//!   cell_offset: u16               cellOffset
//!   n_free: i32                    nFree
//!   n_cell: u16                    nCell
//!   mask_page: u16                 maskPage
//!   ai_ovfl: [u16; 4]              aiOvfl
//!   ap_ovfl: [Option<Vec<u8>>; 4]  apOvfl (corpo da célula em overflow, por posse)
//!   a_data_end: usize              aDataEnd (offset, fim da página inteira)
//!   a_cell_idx: usize              aCellIdx (offset da área de índice de células)
//!   a_data_ofst: usize             aDataOfst (offset: 0 em folha, 4 em interior)
//!   (somem: pBt, aData, pDbPage, xCellSize, xParseCell)
//!
//! `BtLock`:
//!   i_table: u32                   iTable
//!   e_lock: u8                     eLock (READ_LOCK ou WRITE_LOCK)
//!   (somem: pBtree, pNext)
//!
//! `Btree` (possui o `BtShared` por valor):
//!   db_index: i32                  índice em `Connection.dbs` (o `db`/`pBt` do C viram posse)
//!   in_trans: u8                   inTrans
//!   sharable: bool                 sharable
//!   locked: bool                   locked
//!   has_incrblob_cur: bool         hasIncrblobCur
//!   want_to_lock: i32              wantToLock
//!   n_backup: i32                  nBackup
//!   i_b_data_version: u32          iBDataVersion
//!   lock: BtLock                   lock (trava da página 1)
//!   bt: BtShared                   pBt, possuído
//!   (somem: db, pNext, pPrev, nSeek)
//!
//! `BtShared`:
//!   pager: Pager<MemPage>          pPager
//!   db_flags: u64                  cópia de db->flags (o C lê pBt->db->flags)
//!   cursors: Slab<BtCursor>        pCursor (lista) como slab por handle
//!   p_page1: Option<PgId>          pPage1
//!   open_flags: u8                 openFlags
//!   auto_vacuum: u8                autoVacuum
//!   incr_vacuum: u8                incrVacuum
//!   do_truncate: u8                bDoTruncate
//!   in_transaction: u8             inTransaction
//!   max1byte_payload: u8           max1bytePayload
//!   n_reserve_wanted: u8           nReserveWanted
//!   bts_flags: u16                 btsFlags
//!   max_local: u16                 maxLocal
//!   min_local: u16                 minLocal
//!   max_leaf: u16                  maxLeaf
//!   min_leaf: u16                  minLeaf
//!   page_size: u32                 pageSize
//!   usable_size: u32               usableSize
//!   n_transaction: i32             nTransaction
//!   n_page: u32                    nPage
//!   p_schema: Option<Box<dyn Any>> pSchema (o destrutor xFreeSchema é o Drop do Box)
//!   p_has_content: Option<Box<Bitvec>> pHasContent (o `bitvec_create` devolve `Box`)
//!   lock_list: Vec<BtLock>         pLock
//!   has_writer: bool               pWriter != 0 (com Btree == BtShared, o escritor é o próprio)
//!   p_tmp_space: Vec<u8>           pTmpSpace
//!   n_preformat_size: i32          nPreformatSize
//!   (somem: db, mutex, nRef, pNext, xFreeSchema)
//!
//! `CellInfo`:
//!   n_key: i64                     nKey
//!   p_payload: usize               pPayload (offset no buffer entregue ao parse da célula)
//!   n_payload: u32                 nPayload
//!   n_local: u16                   nLocal
//!   n_size: u16                    nSize
//!
//! `BtCursor`:
//!   e_state: u8                    eState
//!   cur_flags: u8                  curFlags
//!   cur_pager_flags: u8            curPagerFlags
//!   hints: u8                      hints
//!   skip_next: i32                 skipNext
//!   p_overflow: Vec<Option<u32>>   aOverflow (None é o 0 do C: localização desconhecida)
//!   bt_key: Option<Vec<u8>>        pKey
//!   info: CellInfo                 info
//!   n_key: i64                     nKey
//!   pgno_root: u32                 pgnoRoot
//!   i_page: i8                     iPage
//!   cur_int_key: u8                curIntKey
//!   ix: u16                        ix
//!   ai_idx: [u16; 19]              aiIdx
//!   p_key_info: Option<Rc<KeyInfo>> pKeyInfo
//!   p_page: Option<PgId>           pPage
//!   ap_page: [Option<PgId>; 19]    apPage
//!   (somem: pBtree, pBt, pNext: o dono é o slab do `BtShared`)
//!
//! `IntegrityCk`:
//!   a_pg_ref: Vec<u8>              aPgRef (1 bit por página)
//!   n_ck_page: u32                 nCkPage
//!   mx_err: i32                    mxErr
//!   n_err: i32                     nErr
//!   rc: i32                        rc
//!   n_step: u32                    nStep
//!   z_pfx: &'static str            zPfx
//!   v0: u32, v1: u32, v2: i32      v0, v1, v2
//!   err_msg: StrAccum              errMsg
//!   heap: Vec<u32>                 heap
//!   n_row: i64                     nRow
//!   (somem: pBt, pPager, db)
//!
//! `BtreePayload<'a>`:
//!   p_key: Option<&'a [u8]>        pKey (com `n_key` bytes em índice)
//!   n_key: i64                     nKey
//!   p_data: Option<&'a [u8]>       pData
//!   a_mem: &'a [Mem]               aMem
//!   n_mem: u16                     nMem
//!   n_data: i32                    nData
//!   n_zero: i32                    nZero
//!
//! Decisões e desvios do C:
//!
//! * `MemPage` tem os flags `isInit`, `intKey`, `intKeyLeaf` e `leaf` como `bool`. O C zera os
//!   oito primeiros bytes quando o pager aloca a página; aqui o `Default` faz o papel (tudo
//!   zerado, `is_init == false`). `xCellSize` e `xParseCell` são escolhidos pelo btree.c a partir
//!   de `int_key`, `int_key_leaf` e `leaf`, então não são campos.
//! * `a_data_end`, `a_cell_idx` e `a_data_ofst` são offsets em relação ao início da página; o
//!   `aData + off` do C é `data[off..]` com `data` vindo de `pager.page_parts(pg)`.
//! * `CellInfo.p_payload` é o offset do payload dentro do buffer entregue à análise da célula:
//!   a página inteira quando a célula está na página, ou o buffer da célula quando está solta
//!   (célula nova, `ap_ovfl`, `p_tmp_space`).
//! * Os arrays de profundidade têm `BTCURSOR_MAX_DEPTH - 1` (19) entradas, como no C. Cada página
//!   em `ap_page`/`p_page` tem uma referência contada no pager.
//! * `Btree == BtShared` (item 4): `p_has_content`, `lock_list` e `has_writer` seguem existindo para
//!   o código do C que os consulta, mas com cache compartilhado desligado só há um dono.
//! * `db_flags` é espelho de `db->flags` que a conexão atualiza antes de chamar o btree (o C lê
//!   `CellSizeCk`, `ResetDatabase` e `ReadUncommit` por `pBt->db`). `nSavepoint`, o busy handler
//!   e o esquema de `sqlite3BtreeSchema` (`p_schema`) chegam por parâmetro ou pelo módulo de
//!   esquema.
//! * `ptrmapPageno` e `PTRMAP_ISPAGE` dependem de funções de btree.c e ficam lá; aqui só os
//!   macros de expressão do cabeçalho (`ptrmap_ptroffset`, `mx_cell_size`, `mx_cell`,
//!   `pending_byte_page`). `get2byteAligned` é `get2byte` (a versão alinhada é só otimização).

use std::any::Any;
use std::rc::Rc;

use crate::bitvec::Bitvec;
use crate::consts::PENDING_BYTE;
use crate::mem::{KeyInfo, Mem};
use crate::pager::Pager;
use crate::pcache::PgId;
use crate::printf::StrAccum;

/// Número máximo de entradas das pilhas de páginas do cursor (`BTCURSOR_MAX_DEPTH - 1`).
pub const CURSOR_STACK: usize = crate::consts::BTCURSOR_MAX_DEPTH - 1;

/// Handle de um cursor no `Slab` de cursores do `BtShared`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CursorId(pub u32);

/// Armazém de itens por índice, com lista de livres. `take` retira o item mas mantém a vaga
/// reservada (para o chamador devolver com `put` depois de contornar o empréstimo); `remove`
/// libera a vaga para reuso.
pub struct Slab<T> {
    slots: Vec<Option<T>>,
    used: Vec<bool>,
    free: Vec<u32>,
}

impl<T> Default for Slab<T> {
    fn default() -> Self {
        Slab { slots: Vec::new(), used: Vec::new(), free: Vec::new() }
    }
}

impl<T> Slab<T> {
    /// Guarda `value` numa vaga livre (ou nova) e devolve o handle.
    pub fn insert(&mut self, value: T) -> CursorId {
        if let Some(i) = self.free.pop() {
            self.slots[i as usize] = Some(value);
            self.used[i as usize] = true;
            CursorId(i)
        } else {
            self.slots.push(Some(value));
            self.used.push(true);
            CursorId((self.slots.len() - 1) as u32)
        }
    }

    /// Retira o item mantendo a vaga reservada; `None` se já estiver retirado ou liberado.
    pub fn take(&mut self, id: CursorId) -> Option<T> {
        self.slots.get_mut(id.0 as usize).and_then(|s| s.take())
    }

    /// Devolve à vaga reservada um item antes retirado com `take`.
    pub fn put(&mut self, id: CursorId, value: T) {
        debug_assert!(self.used[id.0 as usize] && self.slots[id.0 as usize].is_none());
        self.slots[id.0 as usize] = Some(value);
    }

    /// Libera a vaga e devolve o item que estava nela (se não estivesse retirado).
    pub fn remove(&mut self, id: CursorId) -> Option<T> {
        let i = id.0 as usize;
        if i >= self.slots.len() || !self.used[i] {
            return None;
        }
        self.used[i] = false;
        self.free.push(id.0);
        self.slots[i].take()
    }

    pub fn get(&self, id: CursorId) -> Option<&T> {
        self.slots.get(id.0 as usize).and_then(|s| s.as_ref())
    }

    pub fn get_mut(&mut self, id: CursorId) -> Option<&mut T> {
        self.slots.get_mut(id.0 as usize).and_then(|s| s.as_mut())
    }

    /// Itens presentes (não retirados, não liberados), na ordem dos índices.
    pub fn iter(&self) -> impl Iterator<Item = (CursorId, &T)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|v| (CursorId(i as u32), v)))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (CursorId, &mut T)> {
        self.slots
            .iter_mut()
            .enumerate()
            .filter_map(|(i, s)| s.as_mut().map(|v| (CursorId(i as u32), v)))
    }

    /// Vagas reservadas (inclui as retiradas por `take`).
    pub fn len(&self) -> usize {
        self.slots.len() - self.free.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Cópia, guardada em cada `MemPage`, dos campos do `BtShared` que o C lê por `pPage->pBt`
/// (`pageSize`, `usableSize`, `maxLocal`, `minLocal`, `maxLeaf`, `minLeaf`, `max1bytePayload` e
/// o bit `SQLITE_CellSizeCk` de `pBt->db->flags`). Existe porque o `MemPage` não aponta para o
/// `BtShared` e porque o `reiniter` do pager (`pageReinit`) é uma `fn` sem acesso ao `BtShared`.
/// `btree_init_page` e `zero_page` a preenchem a partir do `BtShared` vigente.
#[derive(Debug, Clone, Copy, Default)]
pub struct PageGeom {
    /// `pBt->pageSize`.
    pub page_size: u32,
    /// `pBt->usableSize`.
    pub usable_size: u32,
    /// `pBt->maxLocal`.
    pub max_local: u16,
    /// `pBt->minLocal`.
    pub min_local: u16,
    /// `pBt->maxLeaf`.
    pub max_leaf: u16,
    /// `pBt->minLeaf`.
    pub min_leaf: u16,
    /// `pBt->max1bytePayload`.
    pub max1byte_payload: u8,
    /// `pBt->db->flags & SQLITE_CellSizeCk`.
    pub cell_size_ck: bool,
}

/// `struct MemPage`: cabeçalho decodificado de uma página de árvore-b. Sem bytes: estes ficam
/// no slot da página e se alcançam por `pager.page_parts(pg)`.
#[derive(Default)]
pub struct MemPage {
    /// Geometria do `BtShared` vista na última inicialização (substitui `pPage->pBt->...`).
    pub geom: PageGeom,
    /// Verdadeiro se a página já foi inicializada.
    pub is_init: bool,
    /// Verdadeiro em árvores de tabela, falso em índices.
    pub int_key: bool,
    /// Verdadeiro na folha de uma tabela intKey.
    pub int_key_leaf: bool,
    /// Número desta página.
    pub pgno: u32,
    /// Verdadeiro em página folha.
    pub leaf: bool,
    /// 100 na página 1, 0 nas demais.
    pub hdr_offset: u8,
    /// 0 se folha, 4 se interior.
    pub child_ptr_size: u8,
    /// `min(max_local, 127)`.
    pub max1byte_payload: u8,
    /// Número de corpos de célula em overflow.
    pub n_overflow: u8,
    /// Cópia de `BtShared.max_local` ou `max_leaf`.
    pub max_local: u16,
    /// Cópia de `BtShared.min_local` ou `min_leaf`.
    pub min_local: u16,
    /// Offset da primeira entrada do índice de células.
    pub cell_offset: u16,
    /// Bytes livres na página; -1 se desconhecido.
    pub n_free: i32,
    /// Número de células, locais e em overflow.
    pub n_cell: u16,
    /// Máscara de offset de página.
    pub mask_page: u16,
    /// A i-ésima célula em overflow entra antes da `ai_ovfl[i]`-ésima célula não-overflow.
    pub ai_ovfl: [u16; 4],
    /// Corpo das células em overflow.
    pub ap_ovfl: [Option<Vec<u8>>; 4],
    /// Offset do fim da página inteira (não só do espaço usável).
    pub a_data_end: usize,
    /// Offset da área do índice de células.
    pub a_cell_idx: usize,
    /// Offset igual a 0 em folhas e 4 em páginas interiores.
    pub a_data_ofst: usize,
}

/// `struct BtLock`: uma trava de tabela do cache compartilhado.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BtLock {
    /// Raiz da tabela.
    pub i_table: u32,
    /// `READ_LOCK` ou `WRITE_LOCK`.
    pub e_lock: u8,
}

/// `struct Btree`: o handle do banco; com o cache compartilhado desligado possui o `BtShared`.
pub struct Btree {
    /// Posição deste banco em `Connection.dbs`.
    pub db_index: i32,
    /// `TRANS_NONE`, `TRANS_READ` ou `TRANS_WRITE`.
    pub in_trans: u8,
    pub sharable: bool,
    pub locked: bool,
    pub has_incrblob_cur: bool,
    /// Chamadas aninhadas de `sqlite3BtreeEnter`.
    pub want_to_lock: i32,
    /// Backups lendo esta árvore.
    pub n_backup: i32,
    /// Combina com `pager.i_data_version`.
    pub i_b_data_version: u32,
    /// Objeto usado para travar a página 1.
    pub lock: BtLock,
    /// O conteúdo compartilhável (`pBt`), possuído.
    pub bt: BtShared,
}

/// `struct BtShared`: o arquivo de banco aberto.
pub struct BtShared {
    pub pager: Pager<MemPage>,
    /// Espelho de `db->flags` (ver as decisões no topo do arquivo).
    pub db_flags: u64,
    pub cursors: Slab<BtCursor>,
    pub p_page1: Option<PgId>,
    pub open_flags: u8,
    pub auto_vacuum: u8,
    pub incr_vacuum: u8,
    pub do_truncate: u8,
    pub in_transaction: u8,
    pub max1byte_payload: u8,
    pub n_reserve_wanted: u8,
    pub bts_flags: u16,
    pub max_local: u16,
    pub min_local: u16,
    pub max_leaf: u16,
    pub min_leaf: u16,
    pub page_size: u32,
    pub usable_size: u32,
    pub n_transaction: i32,
    pub n_page: u32,
    /// O esquema do banco, alocado pelo módulo de esquema (`sqlite3BtreeSchema`).
    pub p_schema: Option<Box<dyn Any>>,
    /// Páginas que passaram para a lista livre nesta transação.
    pub p_has_content: Option<Box<Bitvec>>,
    /// Travas de tabela do cache compartilhado.
    pub lock_list: Vec<BtLock>,
    /// Há uma transação de escrita aberta (o `pWriter` do C, que só pode ser este Btree).
    pub has_writer: bool,
    /// Espaço para uma célula.
    pub p_tmp_space: Vec<u8>,
    /// Tamanho da última célula escrita por `TransferRow`.
    pub n_preformat_size: i32,
}

impl BtShared {
    /// `MX_CELL_SIZE(pBt)`.
    #[inline]
    pub fn mx_cell_size(&self) -> i32 {
        self.page_size as i32 - 8
    }

    /// `MX_CELL(pBt)`.
    #[inline]
    pub fn mx_cell(&self) -> u32 {
        (self.page_size - 8) / 6
    }

    /// `PENDING_BYTE_PAGE(pBt)`: página que contém o `PENDING_BYTE`, nunca usada.
    #[inline]
    pub fn pending_byte_page(&self) -> u32 {
        (PENDING_BYTE / self.page_size as i64) as u32 + 1
    }
}

/// `PTRMAP_PTROFFSET(pgptrmap, pgno)`: offset da entrada do mapa de ponteiros.
#[inline]
pub fn ptrmap_ptroffset(pgptrmap: u32, pgno: u32) -> u32 {
    5u32.wrapping_mul(pgno.wrapping_sub(pgptrmap).wrapping_sub(1))
}

/// `struct CellInfo`: a análise de uma célula (`btreeParseCell`).
#[derive(Debug, Clone, Copy, Default)]
pub struct CellInfo {
    /// A chave em tabelas intKey; `n_payload` nas demais.
    pub n_key: i64,
    /// Offset do início do payload no buffer analisado.
    pub p_payload: usize,
    pub n_payload: u32,
    /// Payload guardado localmente.
    pub n_local: u16,
    /// Tamanho da célula na página principal.
    pub n_size: u16,
}

/// `struct BtCursor`: uma posição numa árvore. Não aponta para nada: as páginas são `PgId` com
/// referência contada no pager do `BtShared` dono do slab.
#[derive(Default)]
pub struct BtCursor {
    pub e_state: u8,
    pub cur_flags: u8,
    pub cur_pager_flags: u8,
    pub hints: u8,
    /// Prev() é no-op se negativo, Next() se positivo; código de erro em `CURSOR_FAULT`.
    pub skip_next: i32,
    /// Cache das localizações das páginas de overflow.
    pub p_overflow: Vec<Option<u32>>,
    /// Chave salva da última posição conhecida.
    pub bt_key: Option<Vec<u8>>,
    pub info: CellInfo,
    /// Tamanho de `bt_key`, ou a última chave inteira.
    pub n_key: i64,
    pub pgno_root: u32,
    pub i_page: i8,
    pub cur_int_key: u8,
    pub ix: u16,
    pub ai_idx: [u16; CURSOR_STACK],
    pub p_key_info: Option<Rc<KeyInfo>>,
    /// Página corrente.
    pub p_page: Option<PgId>,
    /// Pilha de pais da página corrente.
    pub ap_page: [Option<PgId>; CURSOR_STACK],
}

/// `struct IntegrityCk`: estado global do `PRAGMA integrity_check`.
pub struct IntegrityCk {
    /// 1 bit por página do banco.
    pub a_pg_ref: Vec<u8>,
    /// Páginas do banco; 0 em verificação parcial.
    pub n_ck_page: u32,
    pub mx_err: i32,
    pub n_err: i32,
    pub rc: i32,
    pub n_step: u32,
    /// Prefixo da mensagem de erro (literal do C).
    pub z_pfx: &'static str,
    pub v0: u32,
    pub v1: u32,
    pub v2: i32,
    pub err_msg: StrAccum,
    /// Min-heap da análise de cobertura de células.
    pub heap: Vec<u32>,
    pub n_row: i64,
}

/// `struct BtreePayload`: o conteúdo de uma entrada para `sqlite3BtreeInsert`.
#[derive(Default)]
pub struct BtreePayload<'a> {
    /// Chave de índices; `None` em tabelas.
    pub p_key: Option<&'a [u8]>,
    /// Tamanho de `p_key` em índices; o rowid em tabelas.
    pub n_key: i64,
    /// Dados de tabelas.
    pub p_data: Option<&'a [u8]>,
    /// Valores da chave decomposta.
    pub a_mem: &'a [Mem],
    pub n_mem: u16,
    pub n_data: i32,
    /// Zeros extras depois de `p_data`.
    pub n_zero: i32,
}
