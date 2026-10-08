//! `dbstat.c`: a tabela virtual `dbstat`, que extrai informação de armazenamento de baixo nível do
//! banco (o que o `sqlite3_analyzer` usa; `SQLITE_ENABLE_DBSTAT_VTAB` está ligada no Debian 13).
//!
//! Caminhos de página (`path`): a raiz de uma árvore é `/`; o filho mais à esquerda da raiz é
//! `/000/`, o seguinte `/001/`, e assim por diante, cada irmão com três dígitos hexadecimais. As
//! páginas de overflow de uma célula acrescentam `+` e seis dígitos hexadecimais ao caminho da
//! célula (`/1c2/000+000000`). Ordenados em BINARY, os overflows de uma célula vêm antes do
//! filho dela.
//!
//! Desvios do C, decorrentes do modelo v2 (CONVENTIONS.md):
//!
//! - O `sqlite3 *db` de `StatTable` é o `&mut Connection` que cada método recebe; a árvore e o
//!   pager vêm de `db.dbs[i_db]` (ver `dbpage::db_btree`).
//! - Os ponteiros viram `Vec`/`Option`: `aPg` é um `Vec<u8>` (vazio é o ponteiro nulo) com os
//!   `DBSTAT_PAGE_PADDING_BYTES` finais zerados, `aCell` e `aOvfl` são `Vec`, e `zName`/`zPath`
//!   são cópias (o C aponta para a coluna do `pStmt`, que só vale até o próximo `sqlite3_step`).
//! - `statSizeAndOffset` omite o `sqlite3OsFileControl(fd, 230440, ...)` do ZIPVFS (extensão
//!   proprietária que nenhum VFS deste porte atende): vale o ramo "não é ZIPVFS".
//! - Sem a falta de memória das alocações.

use std::any::Any;
use std::rc::Rc;

use crate::btree::{btree_get_page_size, btree_get_reserve_no_mutex};
use crate::btree_types::Btree;
use crate::build::{find_db, find_db_name};
use crate::connection::{
    Connection, Context, IndexInfo, ModuleCaps, StmtId, Vtab, VtabCursor, VtabModule,
};
use crate::consts::{
    SQLITE_CONSTRAINT, SQLITE_CORRUPT_BKPT, SQLITE_ERROR, SQLITE_INDEX_CONSTRAINT_EQ, SQLITE_MISUSE,
    SQLITE_NOMEM_BKPT, SQLITE_OK, SQLITE_ROW, SQLITE_VTAB_DIRECTONLY,
};
use crate::dbpage::db_btree;
use crate::main::db_printf;
use crate::mem::{Mem, StrDtor};
use crate::prepare::prepare_v2;
use crate::printf::{mprintf, PrintfArg};
use crate::sqlite_int::Token;
use crate::util::{get2byte, get4byte, get_varint, get_varint32};
use crate::vdbeapi::{
    column_int64, column_text, finalize, reset, result_int, result_int64, result_text, step,
    text_of, value_double,
};
use crate::vtab::{create_module, declare_vtab, vtab_config};

/// `DBSTAT_PAGE_PADDING_BYTES`: o pager e o btree deixam cerca de 200 bytes endereçáveis depois
/// de cada buffer de página, para que leituras além do fim causadas por páginas corrompidas não
/// tenham comportamento indefinido; este módulo acolchoa o buffer com o mesmo propósito.
const DBSTAT_PAGE_PADDING_BYTES: usize = 256;

/// `zDbstatSchema`.
const DBSTAT_SCHEMA: &[u8] = concat!(
    "CREATE TABLE x(",
    " name       TEXT,",          // 0 Nome da tabela ou índice
    " path       TEXT,",          // 1 Caminho da página desde a raiz (NULL no agregado)
    " pageno     INTEGER,",       // 2 Número da página (contagem de páginas no agregado)
    " pagetype   TEXT,",          // 3 'internal', 'leaf', 'overflow' ou NULL
    " ncell      INTEGER,",       // 4 Células na página (0 em overflow)
    " payload    INTEGER,",       // 5 Bytes de payload nesta página
    " unused     INTEGER,",       // 6 Bytes sem uso nesta página
    " mx_payload INTEGER,",       // 7 O maior payload entre as células
    " pgoffset   INTEGER,",       // 8 Posição da página no arquivo (NULL no agregado)
    " pgsize     INTEGER,",       // 9 Tamanho da página (soma no agregado)
    " schema     TEXT HIDDEN,",   // 10 Banco analisado
    " aggregate  BOOLEAN HIDDEN", // 11 Informação agregada por tabela
    ")"
)
.as_bytes();

/// O número de entradas de `StatCursor.aPage` (`ArraySize(pCsr->aPage)`).
const STAT_MAX_PAGES: usize = 32;

/// `struct StatCell`: tamanhos de uma célula de uma página de btree.
#[derive(Default)]
struct StatCell {
    /// Bytes de payload local.
    n_local: i32,
    /// Nó filho (ou 0 se é folha).
    i_child_pg: u32,
    /// `aOvfl`: os números das páginas de overflow (`nOvfl` é o comprimento).
    a_ovfl: Vec<u32>,
    /// Bytes de payload na última página de overflow.
    n_last_ovfl: i32,
    /// Percorre `aOvfl`.
    i_ovfl: i32,
}

/// `struct StatPage`: tamanhos de uma página de btree.
#[derive(Default)]
struct StatPage {
    /// Número da página.
    i_pgno: u32,
    /// O buffer da página (vazio é o ponteiro nulo).
    a_pg: Vec<u8>,
    /// Célula corrente.
    i_cell: i32,
    /// Caminho até a página.
    z_path: Option<Vec<u8>>,

    // Preenchidos por `stat_decode_page`:
    /// Cópia do byte de flags.
    flags: u8,
    /// Número de células da página.
    n_cell: i32,
    /// Bytes sem uso na página.
    n_unused: i32,
    /// As células já decodificadas.
    a_cell: Vec<StatCell>,
    /// Página filha da direita (ou 0).
    i_right_child_pg: u32,
    /// O maior payload entre as células.
    n_mx_payload: i32,
}

/// `struct StatCursor`: o cursor da varredura da tabela virtual `dbstat`.
struct StatCursor {
    /// `pStmt`: percorre o conjunto de páginas raiz.
    p_stmt: Option<StmtId>,
    /// Depois que `pStmt` devolveu `SQLITE_DONE`.
    is_eof: bool,
    /// Agrega os resultados por tabela.
    is_agg: bool,
    /// Banco usado nesta consulta.
    i_db: i32,

    /// Páginas do caminho até a página corrente.
    a_page: Vec<StatPage>,
    /// Entrada corrente de `a_page`.
    i_page: i32,

    // Valores a devolver.
    /// Valor de `pageno`.
    i_pageno: u32,
    /// Valor de `name`.
    z_name: Option<Vec<u8>>,
    /// Valor de `path`.
    z_path: Option<Vec<u8>>,
    /// Valor de `pagetype`.
    z_pagetype: Option<&'static str>,
    /// Páginas da árvore corrente.
    n_page: i32,
    /// Valor de `ncell`.
    n_cell: i32,
    /// Valor de `mx_payload`.
    n_mx_payload: i32,
    /// Valor de `unused`.
    n_unused: i64,
    /// Valor de `payload`.
    n_payload: i64,
    /// Valor de `pgoffset`.
    i_offset: i64,
    /// Valor de `pgsize`.
    sz_page: i64,
}

/// `struct StatTable`: uma instância da tabela virtual `dbstat`.
struct StatTable {
    /// `zErrMsg`.
    z_err_msg: Option<Vec<u8>>,
    /// Banco a analisar.
    i_db: i32,
}

impl Vtab for StatTable {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn err_msg_mut(&mut self) -> &mut Option<Vec<u8>> {
        &mut self.z_err_msg
    }

    /// `statBestIndex`: calcula a melhor estratégia e a devolve em `idxNum`.
    ///
    /// - `0x01`: há um termo `schema=?` no WHERE;
    /// - `0x02`: há um termo `name=?`;
    /// - `0x04`: há um termo `aggregate=?`;
    /// - `0x08`: a saída deve vir ordenada por nome e caminho.
    fn best_index(&mut self, _db: &mut Connection, info: &mut IndexInfo) -> i32 {
        let mut i_schema: i32 = -1;
        let mut i_name: i32 = -1;
        let mut i_agg: i32 = -1;

        // Procura restrições `schema=?`, `name=?` e `aggregate=?` válidas.
        for (i, c) in info.a_constraint.iter().enumerate() {
            if c.op as i32 != SQLITE_INDEX_CONSTRAINT_EQ {
                continue;
            }
            if !c.usable {
                // O DBSTAT precisa ser sempre a tabela mais à direita de uma junção.
                return SQLITE_CONSTRAINT;
            }
            match c.i_column {
                0 => i_name = i as i32,    // name
                10 => i_schema = i as i32, // schema
                11 => i_agg = i as i32,    // aggregate
                _ => {}
            }
        }
        let mut i = 0;
        if i_schema >= 0 {
            i += 1;
            info.a_constraint_usage[i_schema as usize].argv_index = i;
            info.a_constraint_usage[i_schema as usize].omit = true;
            info.idx_num |= 0x01;
        }
        if i_name >= 0 {
            i += 1;
            info.a_constraint_usage[i_name as usize].argv_index = i;
            info.idx_num |= 0x02;
        }
        if i_agg >= 0 {
            i += 1;
            info.a_constraint_usage[i_agg as usize].argv_index = i;
            info.idx_num |= 0x04;
        }
        info.estimated_cost = 1.0;

        // As linhas saem sempre em ordem crescente de (name, path). Se isso basta ao cliente,
        // `orderByConsumed` evita a ordenação externa.
        let ob = &info.a_order_by;
        if (ob.len() == 1 && ob[0].i_column == 0 && !ob[0].desc)
            || (ob.len() == 2
                && ob[0].i_column == 0
                && !ob[0].desc
                && ob[1].i_column == 1
                && !ob[1].desc)
        {
            info.order_by_consumed = 1;
            info.idx_num |= 0x08;
        }
        SQLITE_OK
    }

    /// `statDisconnect`.
    fn disconnect(&mut self, _db: &mut Connection) -> i32 {
        SQLITE_OK
    }

    /// `xDestroy` é o mesmo `statDisconnect`.
    fn destroy(&mut self, _db: &mut Connection) -> i32 {
        SQLITE_OK
    }

    /// `statOpen`.
    fn open(&mut self, _db: &mut Connection) -> Result<Box<dyn VtabCursor>, i32> {
        Ok(Box::new(StatCursor::new(self.i_db)))
    }
}

/// `statClearCells`.
fn stat_clear_cells(p: &mut StatPage) {
    p.n_cell = 0;
    p.a_cell = Vec::new();
}

/// `statClearPage`: zera a página, menos o buffer.
fn stat_clear_page(p: &mut StatPage) {
    let a_pg = std::mem::take(&mut p.a_pg);
    *p = StatPage { a_pg, ..StatPage::default() };
}

/// `getLocalPayload`: para uma célula, os bytes de conteúdo (payload) guardados na própria
/// página, isto é, os que não estão em páginas de overflow.
fn get_local_payload(n_usable: i32, flags: u8, n_total: i32) -> i32 {
    let n_min_local: i32;
    let n_max_local: i32;
    if flags == 0x0D {
        // Folha de tabela.
        n_min_local = (n_usable - 12) * 32 / 255 - 23;
        n_max_local = n_usable - 35;
    } else {
        // Nós internos e folhas de índice.
        n_min_local = (n_usable - 12) * 32 / 255 - 23;
        n_max_local = (n_usable - 12) * 64 / 255 - 23;
    }
    let mut n_local = n_min_local.wrapping_add(n_total.wrapping_sub(n_min_local) % (n_usable - 4));
    if n_local > n_max_local {
        n_local = n_min_local;
    }
    n_local
}

/// O motivo de `stat_decode_cells` não completar.
enum Decode {
    /// `goto statPageIsCorrupt`.
    Corrupt,
    /// Um `return rc` com um código de erro do pager.
    Rc(i32),
}

/// O corpo de `statDecodePage` sobre os bytes `a` da página (que o chamador retirou de `p`).
fn stat_decode_cells(bt: &mut Btree, p: &mut StatPage, a: &[u8]) -> Result<(), Decode> {
    let hdr = if p.i_pgno == 1 { 100 } else { 0 };

    p.flags = a[hdr];
    let is_leaf;
    let mut n_hdr: i32;
    if p.flags == 0x0A || p.flags == 0x0D {
        is_leaf = true;
        n_hdr = 8;
    } else if p.flags == 0x05 || p.flags == 0x02 {
        is_leaf = false;
        n_hdr = 12;
    } else {
        return Err(Decode::Corrupt);
    }
    if p.i_pgno == 1 {
        n_hdr += 100;
    }
    p.n_cell = get2byte(&a[hdr + 3..]) as i32;
    p.n_mx_payload = 0;
    let sz_page = btree_get_page_size(bt);

    let mut n_unused = (get2byte(&a[hdr + 5..]) as i32) - n_hdr - 2 * p.n_cell;
    n_unused += a[hdr + 7] as i32;
    let mut i_off = get2byte(&a[hdr + 1..]) as i32;
    while i_off != 0 {
        if i_off >= sz_page {
            return Err(Decode::Corrupt);
        }
        n_unused += get2byte(&a[i_off as usize + 2..]) as i32;
        let i_next = get2byte(&a[i_off as usize..]) as i32;
        if i_next < i_off + 4 && i_next > 0 {
            return Err(Decode::Corrupt);
        }
        i_off = i_next;
    }
    p.n_unused = n_unused;
    p.i_right_child_pg = if is_leaf { 0 } else { get4byte(&a[hdr + 8..]) };

    if p.n_cell != 0 {
        let n_usable = sz_page - btree_get_reserve_no_mutex(bt);
        p.a_cell = (0..p.n_cell).map(|_| StatCell::default()).collect();

        for i in 0..p.n_cell {
            i_off = get2byte(&a[(n_hdr + i * 2) as usize..]) as i32;
            if i_off < n_hdr || i_off >= sz_page {
                return Err(Decode::Corrupt);
            }
            if !is_leaf {
                p.a_cell[i as usize].i_child_pg = get4byte(&a[i_off as usize..]);
                i_off += 4;
            }
            if p.flags == 0x05 {
                // Nó interno de tabela: nPayload==0.
            } else {
                // Bytes de payload no total (local mais overflow).
                let (n, n_payload) = get_varint32(&a[i_off as usize..]);
                i_off += n as i32;
                if p.flags == 0x0D {
                    let (n, _dummy) = get_varint(&a[i_off as usize..]);
                    i_off += n as i32;
                }
                if n_payload > p.n_mx_payload as u32 {
                    p.n_mx_payload = n_payload as i32;
                }
                let n_local = get_local_payload(n_usable, p.flags, n_payload as i32);
                if n_local < 0 {
                    return Err(Decode::Corrupt);
                }
                p.a_cell[i as usize].n_local = n_local;
                debug_assert!(n_payload >= n_local as u32);
                debug_assert!(n_local <= n_usable - 35);
                if n_payload > n_local as u32 {
                    let usable4 = (n_usable - 4) as u32;
                    let n_ovfl = (n_payload
                        .wrapping_sub(n_local as u32)
                        .wrapping_add(usable4)
                        .wrapping_sub(1)
                        / usable4) as i32;
                    if i_off + n_local + 4 > n_usable || n_payload > 0x7fff_ffff {
                        return Err(Decode::Corrupt);
                    }
                    let cell = &mut p.a_cell[i as usize];
                    cell.n_last_ovfl = n_payload
                        .wrapping_sub(n_local as u32)
                        .wrapping_sub((n_ovfl - 1).wrapping_mul(n_usable - 4) as u32)
                        as i32;
                    cell.a_ovfl = vec![0u32; n_ovfl as usize];
                    cell.a_ovfl[0] = get4byte(&a[(i_off + n_local) as usize..]);
                    for j in 1..n_ovfl as usize {
                        let i_prev = cell.a_ovfl[j - 1];
                        let pager = &mut bt.bt.pager;
                        match pager.get(i_prev, 0) {
                            Ok(pg) => {
                                cell.a_ovfl[j] = get4byte(pager.page_data(pg));
                                pager.unref(Some(pg));
                            }
                            Err(rc) => return Err(Decode::Rc(rc)),
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// `statDecodePage`: preenche o `StatPage` com a informação de todas as células da página em
/// análise.
fn stat_decode_page(bt: &mut Btree, p: &mut StatPage) -> i32 {
    let a_data = std::mem::take(&mut p.a_pg);
    let r = stat_decode_cells(bt, p, &a_data);
    p.a_pg = a_data;
    match r {
        Ok(()) => SQLITE_OK,
        Err(Decode::Corrupt) => {
            // statPageIsCorrupt
            p.flags = 0;
            stat_clear_cells(p);
            SQLITE_OK
        }
        Err(Decode::Rc(rc)) => rc,
    }
}

/// `statGetPage`: carrega uma cópia da página `i_pg` no buffer de `p_pg`, alocando-o se preciso.
fn stat_get_page(bt: &mut Btree, i_pg: u32, p_pg: &mut StatPage) -> i32 {
    let pgsz = btree_get_page_size(bt) as usize;
    if p_pg.a_pg.is_empty() {
        p_pg.a_pg = vec![0u8; pgsz + DBSTAT_PAGE_PADDING_BYTES];
    }
    let pager = &mut bt.bt.pager;
    match pager.get(i_pg, 0) {
        Ok(pg) => {
            p_pg.a_pg[..pgsz].copy_from_slice(&pager.page_data(pg)[..pgsz]);
            pager.unref(Some(pg));
            SQLITE_OK
        }
        Err(rc) => rc,
    }
}

impl StatCursor {
    /// `statOpen`: o cursor zerado, no banco `i_db`.
    fn new(i_db: i32) -> StatCursor {
        StatCursor {
            p_stmt: None,
            is_eof: false,
            is_agg: false,
            i_db,
            a_page: (0..STAT_MAX_PAGES).map(|_| StatPage::default()).collect(),
            i_page: 0,
            i_pageno: 0,
            z_name: None,
            z_path: None,
            z_pagetype: None,
            n_page: 0,
            n_cell: 0,
            n_mx_payload: 0,
            n_unused: 0,
            n_payload: 0,
            i_offset: 0,
            sz_page: 0,
        }
    }

    /// `statResetCsr`.
    fn reset_csr(&mut self, db: &mut Connection) {
        for p in self.a_page.iter_mut() {
            stat_clear_page(p);
            p.a_pg = Vec::new();
        }
        if let Some(stmt) = self.p_stmt {
            reset(db, stmt);
        }
        self.i_page = 0;
        self.z_path = None;
        self.is_eof = false;
    }

    /// `statResetCounts`: zera os contadores de espaço do cursor.
    fn reset_counts(&mut self) {
        self.n_cell = 0;
        self.n_mx_payload = 0;
        self.n_unused = 0;
        self.n_payload = 0;
        self.sz_page = 0;
        self.n_page = 0;
    }

    /// `statSizeAndOffset`: preenche `iOffset` e `szPage` a partir de `iPageno`.
    fn size_and_offset(&mut self, db: &mut Connection) {
        if let Some(bt) = db_btree(db, self.i_db) {
            self.sz_page += btree_get_page_size(bt) as i64;
            self.i_offset = self.sz_page * self.i_pageno.wrapping_sub(1) as i64;
        }
    }

    /// `statNext`: avança o cursor para a próxima entrada. Normalmente é a próxima página; no
    /// modo agregado (`isAgg`) é a próxima árvore.
    fn stat_next(&mut self, db: &mut Connection) -> i32 {
        let Some(stmt) = self.p_stmt else {
            return SQLITE_MISUSE;
        };
        self.z_path = None;

        loop {
            // statNextRestart
            let mut rc: i32;
            if self.i_page < 0 {
                // Começa a medir o espaço da próxima árvore.
                self.reset_counts();
                let rc_step = step(db, stmt);
                if rc_step == SQLITE_ROW {
                    let i_root = column_int64(db, stmt, 1) as u32;
                    let Some(bt) = db_btree(db, self.i_db) else {
                        return SQLITE_ERROR;
                    };
                    let n_page = bt.bt.pager.pagecount();
                    if n_page == 0 {
                        self.is_eof = true;
                        return reset(db, stmt);
                    }
                    rc = stat_get_page(bt, i_root, &mut self.a_page[0]);
                    self.a_page[0].i_pgno = i_root;
                    self.a_page[0].i_cell = 0;
                    if !self.is_agg {
                        self.a_page[0].z_path = Some(b"/".to_vec());
                    }
                    self.i_page = 0;
                    self.n_page = 1;
                } else {
                    self.is_eof = true;
                    return reset(db, stmt);
                }
            } else {
                // Continua analisando a árvore já começada.
                let ip = self.i_page as usize;
                if !self.is_agg {
                    self.reset_counts();
                }
                while self.a_page[ip].i_cell < self.a_page[ip].n_cell {
                    let ci = self.a_page[ip].i_cell as usize;
                    while (self.a_page[ip].a_cell[ci].i_ovfl as usize)
                        < self.a_page[ip].a_cell[ci].a_ovfl.len()
                    {
                        let Some(bt) = db_btree(db, self.i_db) else {
                            return SQLITE_ERROR;
                        };
                        let n_usable = btree_get_page_size(bt) - btree_get_reserve_no_mutex(bt);
                        self.n_page += 1;
                        self.size_and_offset(db);
                        let cell = &mut self.a_page[ip].a_cell[ci];
                        if (cell.i_ovfl as usize) < cell.a_ovfl.len() - 1 {
                            self.n_payload += (n_usable - 4) as i64;
                        } else {
                            self.n_payload += cell.n_last_ovfl as i64;
                            self.n_unused += (n_usable - 4 - cell.n_last_ovfl) as i64;
                        }
                        let i_ovfl = cell.i_ovfl;
                        cell.i_ovfl += 1;
                        if !self.is_agg {
                            let pg_no = cell.a_ovfl[i_ovfl as usize];
                            self.z_name = column_text(db, stmt, 0).map(|z| z.to_vec());
                            self.i_pageno = pg_no;
                            self.z_pagetype = Some("overflow");
                            let page = &self.a_page[ip];
                            self.z_path = mprintf(
                                b"%s%.3x+%.6x",
                                &[
                                    PrintfArg::Text(page.z_path.clone()),
                                    PrintfArg::Int(page.i_cell as i64),
                                    PrintfArg::Int(i_ovfl as i64),
                                ],
                            );
                            return if self.z_path.is_none() { SQLITE_NOMEM_BKPT } else { SQLITE_OK };
                        }
                    }
                    if self.a_page[ip].i_right_child_pg != 0 {
                        break;
                    }
                    self.a_page[ip].i_cell += 1;
                }

                if self.a_page[ip].i_right_child_pg == 0
                    || self.a_page[ip].i_cell > self.a_page[ip].n_cell
                {
                    stat_clear_page(&mut self.a_page[ip]);
                    self.i_page -= 1;
                    if self.is_agg && self.i_page < 0 {
                        // label-statNext-done: ao calcular o uso de espaço agregado de uma
                        // árvore inteira, esta é a saída da função.
                        return SQLITE_OK;
                    }
                    continue; // statNextRestart
                }
                self.i_page += 1;
                if self.i_page as usize >= STAT_MAX_PAGES {
                    self.reset_csr(db);
                    return SQLITE_CORRUPT_BKPT;
                }

                let child_pg = if self.a_page[ip].i_cell == self.a_page[ip].n_cell {
                    self.a_page[ip].i_right_child_pg
                } else {
                    self.a_page[ip].a_cell[self.a_page[ip].i_cell as usize].i_child_pg
                };
                self.a_page[ip + 1].i_pgno = child_pg;
                let Some(bt) = db_btree(db, self.i_db) else {
                    return SQLITE_ERROR;
                };
                rc = stat_get_page(bt, child_pg, &mut self.a_page[ip + 1]);
                self.n_page += 1;
                self.a_page[ip + 1].i_cell = 0;
                if !self.is_agg {
                    let parent = &self.a_page[ip];
                    let z = mprintf(
                        b"%s%.3x/",
                        &[
                            PrintfArg::Text(parent.z_path.clone()),
                            PrintfArg::Int(parent.i_cell as i64),
                        ],
                    );
                    if z.is_none() {
                        rc = SQLITE_NOMEM_BKPT;
                    }
                    self.a_page[ip + 1].z_path = z;
                }
                self.a_page[ip].i_cell += 1;
            }

            // Preenche os campos do cursor com os valores que `xColumn` e `xRowid` devolvem.
            if rc == SQLITE_OK {
                let ip = self.i_page as usize;
                self.z_name = column_text(db, stmt, 0).map(|z| z.to_vec());
                self.i_pageno = self.a_page[ip].i_pgno;

                let Some(bt) = db_btree(db, self.i_db) else {
                    return SQLITE_ERROR;
                };
                rc = stat_decode_page(bt, &mut self.a_page[ip]);
                if rc == SQLITE_OK {
                    self.size_and_offset(db);

                    let p = &self.a_page[ip];
                    self.z_pagetype = Some(match p.flags {
                        0x05 | 0x02 => "internal",
                        0x0D | 0x0A => "leaf",
                        _ => "corrupted",
                    });
                    self.n_cell += p.n_cell;
                    self.n_unused += p.n_unused as i64;
                    if p.n_mx_payload > self.n_mx_payload {
                        self.n_mx_payload = p.n_mx_payload;
                    }
                    if !self.is_agg {
                        self.z_path = p.z_path.clone();
                    }
                    let mut n_payload: i32 = 0;
                    for c in p.a_cell.iter().take(p.n_cell as usize) {
                        n_payload = n_payload.wrapping_add(c.n_local);
                    }
                    self.n_payload += n_payload as i64;

                    // No modo agregado por árvore segue com a próxima página. O laço sai pelo
                    // `return` de label-statNext-done.
                    if self.is_agg {
                        continue; // statNextRestart
                    }
                }
            }
            return rc;
        }
    }
}

impl VtabCursor for StatCursor {
    /// `statClose`.
    fn close(&mut self, db: &mut Connection, _vtab: &mut dyn Vtab) -> i32 {
        self.reset_csr(db);
        if let Some(stmt) = self.p_stmt.take() {
            finalize(db, stmt);
        }
        SQLITE_OK
    }

    /// `statFilter`: inicializa o cursor conforme o plano `idxNum` (ver `best_index`) com os
    /// argumentos de `argv`.
    fn filter(
        &mut self,
        db: &mut Connection,
        vtab: &mut dyn Vtab,
        idx_num: i32,
        _idx_str: Option<&[u8]>,
        argv: &[Mem],
    ) -> i32 {
        let tab_i_db = vtab.as_any_mut().downcast_mut::<StatTable>().map_or(0, |t| t.i_db);
        let mut i_arg = 0;

        self.reset_csr(db);
        if let Some(stmt) = self.p_stmt.take() {
            finalize(db, stmt);
        }
        if idx_num & 0x01 != 0 {
            // Há `schema=?`: pega o valor.
            let z_dbase = text_of(&argv[i_arg]);
            i_arg += 1;
            self.i_db = find_db_name(db, z_dbase.as_deref());
            if self.i_db < 0 {
                self.i_db = 0;
                self.is_eof = true;
                return SQLITE_OK;
            }
        } else {
            self.i_db = tab_i_db;
        }
        let mut z_name: Option<Vec<u8>> = None;
        if idx_num & 0x02 != 0 {
            // Há `name=?`.
            z_name = text_of(&argv[i_arg]).map(|z| z.into_owned());
            i_arg += 1;
        }
        if idx_num & 0x04 != 0 {
            // Há `aggregate=?`.
            self.is_agg = value_double(&argv[i_arg]) != 0.0;
        } else {
            self.is_agg = false;
        }
        let Some(schema_name) = usize::try_from(self.i_db)
            .ok()
            .and_then(|i| db.dbs.get(i))
            .map(|slot| slot.z_db_s_name.clone())
        else {
            return SQLITE_ERROR;
        };
        let Some(mut z_sql) = db_printf(
            db,
            b"SELECT * FROM (SELECT 'sqlite_schema' AS name,1 AS rootpage,'table' AS type UNION ALL SELECT name,rootpage,type FROM \"%w\".sqlite_schema WHERE rootpage!=0)",
            &[PrintfArg::Text(Some(schema_name))],
        ) else {
            return SQLITE_NOMEM_BKPT;
        };
        if let Some(name) = z_name {
            // Sem espaço antes do WHERE, como no C.
            let Some(z) = db_printf(db, b"WHERE name=%Q", &[PrintfArg::Text(Some(name))]) else {
                return SQLITE_NOMEM_BKPT;
            };
            z_sql.extend_from_slice(&z);
        }
        if idx_num & 0x08 != 0 {
            z_sql.extend_from_slice(b" ORDER BY name");
        }
        let (mut rc, p_stmt, _tail) = prepare_v2(db, &z_sql, -1);
        self.p_stmt = p_stmt;

        if rc == SQLITE_OK {
            self.i_page = -1;
            rc = self.stat_next(db);
        }
        rc
    }

    /// `statNext`.
    fn next(&mut self, db: &mut Connection, _vtab: &mut dyn Vtab) -> i32 {
        self.stat_next(db)
    }

    /// `statEof`.
    fn eof(&mut self, _vtab: &mut dyn Vtab) -> i32 {
        self.is_eof as i32
    }

    /// `statColumn`.
    fn column(&mut self, _vtab: &mut dyn Vtab, ctx: &mut Context<'_>, i: i32) -> i32 {
        match i {
            0 => {
                // name
                result_text(ctx, self.z_name.as_deref(), -1, StrDtor::Transient);
            }
            1 => {
                // path
                if !self.is_agg {
                    result_text(ctx, self.z_path.as_deref(), -1, StrDtor::Transient);
                }
            }
            2 => {
                // pageno
                if self.is_agg {
                    result_int64(ctx, self.n_page as i64);
                } else {
                    result_int64(ctx, self.i_pageno as i64);
                }
            }
            3 => {
                // pagetype
                if !self.is_agg {
                    result_text(ctx, self.z_pagetype.map(str::as_bytes), -1, StrDtor::Static);
                }
            }
            4 => result_int64(ctx, self.n_cell as i64),        // ncell
            5 => result_int64(ctx, self.n_payload),            // payload
            6 => result_int64(ctx, self.n_unused),             // unused
            7 => result_int64(ctx, self.n_mx_payload as i64),  // mx_payload
            8 => {
                // pgoffset
                if !self.is_agg {
                    result_int64(ctx, self.i_offset);
                }
            }
            9 => result_int64(ctx, self.sz_page), // pgsize
            10 => {
                // schema
                let name = usize::try_from(self.i_db)
                    .ok()
                    .and_then(|i| ctx.db.dbs.get(i))
                    .map(|slot| slot.z_db_s_name.clone());
                result_text(ctx, name.as_deref(), -1, StrDtor::Transient);
            }
            _ => {
                // aggregate
                result_int(ctx, self.is_agg as i32);
            }
        }
        SQLITE_OK
    }

    /// `statRowid`.
    fn rowid(&mut self, _vtab: &mut dyn Vtab, rowid: &mut i64) -> i32 {
        *rowid = self.i_pageno as i64;
        SQLITE_OK
    }
}

/// `dbstat_module`: `xCreate` e `xConnect` são a mesma função, com `iVersion` 0.
struct DbstatModule;

impl VtabModule for DbstatModule {
    fn i_version(&self) -> i32 {
        0
    }

    fn caps(&self) -> ModuleCaps {
        ModuleCaps { create: true, ..ModuleCaps::default() }
    }

    fn create_is_connect(&self) -> bool {
        true
    }

    /// `statConnect`.
    fn x_connect(
        &self,
        db: &mut Connection,
        _aux: &Option<Rc<dyn Any>>,
        argv: &[Vec<u8>],
        err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        let i_db = if argv.len() >= 4 {
            let nm = Token { z: argv[3].clone(), i_ofst: 0 };
            let i_db = find_db(db, &nm);
            if i_db < 0 {
                *err = mprintf(b"no such database: %s", &[PrintfArg::Text(Some(argv[3].clone()))]);
                return Err(SQLITE_ERROR);
            }
            i_db
        } else {
            0
        };
        vtab_config(db, SQLITE_VTAB_DIRECTONLY, 0);
        let rc = declare_vtab(db, DBSTAT_SCHEMA);
        if rc == SQLITE_OK {
            Ok(Box::new(StatTable { z_err_msg: None, i_db }))
        } else {
            Err(rc)
        }
    }
}

/// `sqlite3DbstatRegister`: registra o módulo `dbstat`.
pub fn dbstat_register(db: &mut Connection) -> i32 {
    let module: Rc<dyn VtabModule> = Rc::new(DbstatModule);
    create_module(db, b"dbstat", Some(module), None, None)
}
