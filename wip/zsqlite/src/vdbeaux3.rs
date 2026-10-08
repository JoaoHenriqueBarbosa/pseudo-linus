//! `vdbeaux.c` (parte 3): trechos 010 a 013 do `vdbeaux.c` do SQLite 3.46.1. O que ainda não
//! existia em `record.rs` e `mem.rs`: os invólucros de `sqlite3VdbeIdxRowid` e
//! `sqlite3VdbeIdxKeyCompare` que leem o payload do cursor, o contador de mudanças, a expiração
//! dos comandos preparados, `sqlite3VdbeGetBoundValue`, `sqlite3VdbeSetVarmask`,
//! `sqlite3NotPureFunc`, `sqlite3VtabImportErrmsg` e o gancho de pré-atualização
//! (`sqlite3VdbePreUpdateHook`, com `vdbeFreeUnpacked`).
//!
//! Já existem e NÃO foram repetidos aqui (ver `record.rs` e `mem.rs`): `sqlite3SmallTypeSizes`,
//! `sqlite3VdbeSerialTypeLen`, `sqlite3VdbeOneByteSerialTypeLen`, `serialGet`, `serialGet7`,
//! `sqlite3VdbeSerialGet`, `sqlite3VdbeAllocUnpackedRecord`, `sqlite3VdbeRecordUnpack`,
//! `vdbeRecordDecodeInt`, `sqlite3VdbeRecordCompareWithSkip`, `sqlite3VdbeRecordCompare`,
//! `vdbeRecordCompareInt`, `vdbeRecordCompareString`, `sqlite3VdbeFindCompare` (todos em
//! `record.rs`) e `isAllZero`, `sqlite3BlobCompare`, `sqlite3IntFloatCompare`,
//! `sqlite3MemCompare`, `vdbeCompareMemString` (em `mem.rs`). Os blocos `SQLITE_DEBUG`
//! (`vdbeAssertFieldCountWithinLimits`, `vdbeRecordCompareDebug`), `SQLITE_MIXED_ENDIAN_64BIT_FLOAT`
//! (`sqlite3FloatSwap`) e `SQLITE_ENABLE_CURSOR_HINTS` com `SQLITE_DEBUG`
//! (`sqlite3CursorRangeHintExprCheck`) não existem na build do Debian.
//!
//! Desvios do C, todos decorrentes do modelo v2 (ver `vdbeaux.rs`, `vdbeaux2.rs` e
//! `CONVENTIONS.md`):
//!
//! * `sqlite3VdbeIdxRowid` e `sqlite3VdbeIdxKeyCompare` recebem o `BtCursor` e o `BtShared` já
//!   emprestados (quem chama tem o `with_btree_cursor` que retira o cursor do slab), como o C
//!   recebe o `BtCursor`. O `VdbeCursor` do `sqlite3VdbeIdxKeyCompare` só servia para chegar ao
//!   `uc.pCursor`; o `db` só servia para o `sqlite3VdbeMemInit`.
//! * `sqlite3VdbeDb` e `sqlite3VdbePrepareFlags` não existem: o `Vdbe` não tem `db` e
//!   `Vdbe.prep_flags` é campo público (acessor de campo não é função).
//! * `sqlite3ExpirePreparedStatements` percorre `Connection.stmts` (os comandos retirados do slab
//!   por `take` no momento não estão na lista, como o C só enxerga os que estão encadeados).
//! * `sqlite3NotPureFunc` não tem o `pCtx->pVdbe->aOp + pCtx->iOp`: o `Context` já diz se o
//!   opcode é `OP_PureFunc` (`is_pure_func`) e quem chama passa o `p5` do opcode. O
//!   `sqlite3_result_error` é inlinado (`is_error` mais `mem_set_str` UTF-8). O teste
//!   `pCtx->pVdbe==0` do `SQLITE_ENABLE_STAT4` não existe: o `Context` não tem ponteiro de `Vdbe`.
//! * `sqlite3VtabImportErrmsg` recebe o `zErrMsg` do `sqlite3_vtab` por referência (o tipo
//!   `Vtab` mora em `vtab.rs`), e MOVE o texto em vez de duplicar e liberar.
//! * `sqlite3VdbePreUpdateHook`: `preupdate.v`/`pCsr` viram o handle do comando (`stmt`) e o
//!   `i_db`/`cursor` do `PreUpdate`; `zDb` sai de `Connection.dbs[i_db]` (é o que o `PreUpdate`
//!   guarda), logo não é parâmetro. O gancho (`PreUpdateFn`) recebe `&mut Connection` e o
//!   `&mut PreUpdate`: este é movido para fora de `Connection.p_pre_update` durante a chamada
//!   (não se pode emprestar o mesmo objeto duas vezes), de modo que `p_pre_update` fica `None`
//!   enquanto o gancho roda e as funções `sqlite3_preupdate_*` leem o argumento do gancho.

use std::rc::Rc;

use crate::btree_cursor::{
    btree_max_record_size, btree_payload, btree_payload_fetch, btree_payload_size,
};
use crate::btree_types::{BtCursor, BtShared};
use crate::build::primary_key_index;
use crate::connection::{Connection, Context, StmtId};
use crate::consts::{
    MEM_NULL, NC_GENCOL, NC_ISCHECK, SQLITE_ERROR, SQLITE_LIMIT_LENGTH, SQLITE_OK,
    SQLITE_UPDATE,
};
use crate::mem::{
    apply_affinity, mem_copy, mem_from_btree_zero_offset, mem_release, mem_release_malloc,
    mem_set_str, KeyInfo, Mem, StrDtor, UnpackedRecord, ENC_UTF8,
};
use crate::printf::{mprintf, PrintfArg};
use crate::record::{idx_key_check_cell_size, idx_key_compare, idx_rowid};
use crate::sqlite_int::Table;
use crate::vdbe_types::{PreUpdate, Vdbe, VdbeCursor};

// ---------------------------------------------------------------------------------------------
// chunk 012 (final)
// ---------------------------------------------------------------------------------------------

/// O trecho comum de `sqlite3VdbeIdxRowid` e `sqlite3VdbeIdxKeyCompare`: depois de ler o tamanho
/// do payload (que prepara a informação da célula), `sqlite3VdbeMemFromBtreeZeroOffset(pCur,
/// n_cell_key, m)`. Se o payload inteiro cabe no trecho local da página a célula recebe uma cópia
/// efêmera; senão lê o payload todo (inclusive as páginas de overflow).
pub(crate) fn mem_from_cursor_zero_offset(m: &mut Mem, cur: &mut BtCursor, bt: &mut BtShared, amt: u32) -> i32 {
    let max_record_size = btree_max_record_size(bt).clamp(0, u32::MAX as i64) as u32;
    let fits = btree_payload_fetch(cur, bt).len() >= amt as usize;
    if fits {
        // O leitor de payload nunca é chamado quando o trecho local basta.
        let available = btree_payload_fetch(cur, bt);
        mem_from_btree_zero_offset(m, max_record_size, amt, available, |_| SQLITE_OK)
    } else {
        mem_from_btree_zero_offset(m, max_record_size, amt, &[], |buf| {
            btree_payload(cur, bt, 0, amt, buf)
        })
    }
}

/// `sqlite3VdbeIdxRowid`: `cur` aponta para uma entrada de índice criada pelo `OP_MakeRecord`.
/// Lê o rowid (o último campo do registro) e o grava em `rowid`. Devolve `SQLITE_OK` ou um
/// código de erro. O conteúdo pode vir de um arquivo corrompido, então é conferido em
/// [`idx_rowid`].
pub(crate) fn vdbe_idx_rowid(cur: &mut BtCursor, bt: &mut BtShared, rowid: &mut i64) -> i32 {
    // Tamanho da entrada do índice. Só entradas com menos de 2GiB são aceitas: qualquer coisa
    // maior é corrupção, detectada em `sqlite3BtreeParseCellPtr()`.
    let n_cell_key = btree_payload_size(cur, bt) as i64;
    debug_assert!((n_cell_key & 0xffff_ffff) == n_cell_key);

    // Lê o conteúdo completo da entrada do índice.
    let mut m = Mem::default();
    let rc = mem_from_cursor_zero_offset(&mut m, cur, bt, n_cell_key as u32);
    if rc != SQLITE_OK {
        return rc;
    }

    let n = (m.n.max(0) as usize).min(m.z.len());
    let rc = idx_rowid(&m.z[..n], rowid);
    mem_release_malloc(&mut m);
    rc
}

/// `sqlite3VdbeIdxKeyCompare`: compara a chave da entrada de índice para a qual `cur` aponta com
/// `p_unpacked` e grava em `res` um número negativo, zero ou positivo. `p_unpacked` foi criado
/// sem o rowid ou truncado antes dele, e o rowid do fim da entrada também é ignorado: só os
/// prefixos das chaves são comparados. Devolve `SQLITE_OK` em caso de sucesso.
pub(crate) fn vdbe_idx_key_compare(
    cur: &mut BtCursor,
    bt: &mut BtShared,
    p_unpacked: &mut UnpackedRecord,
    res: &mut i32,
) -> i32 {
    let n_cell_key = btree_payload_size(cur, bt) as i64;
    // `n_cell_key` está sempre entre 0 e 0xffffffff por causa de `btreeParseCellPtr()` e de
    // `sqlite3GetVarint32()`.
    let rc = idx_key_check_cell_size(n_cell_key, res);
    if rc != SQLITE_OK {
        return rc;
    }
    let mut m = Mem::default();
    let rc = mem_from_cursor_zero_offset(&mut m, cur, bt, n_cell_key as u32);
    if rc != SQLITE_OK {
        return rc;
    }
    let n = (m.n.max(0) as usize).min(m.z.len());
    let rc = idx_key_compare(&m.z[..n], p_unpacked, res);
    mem_release_malloc(&mut m);
    rc
}

/// `sqlite3VdbeSetChanges`: o valor devolvido pelas chamadas seguintes de `sqlite3_changes()`.
pub fn vdbe_set_changes(db: &mut Connection, n_change: i64) {
    db.n_change = n_change;
    db.n_total_change = db.n_total_change.wrapping_add(n_change);
}

// ---------------------------------------------------------------------------------------------
// chunk 013
// ---------------------------------------------------------------------------------------------

/// `sqlite3VdbeCountChanges`: liga o sinal para atualizar o contador de mudanças quando o `Vdbe`
/// for finalizado ou reiniciado.
pub fn vdbe_count_changes(v: &mut Vdbe) {
    v.change_cnt_on = true;
}

/// `sqlite3ExpirePreparedStatements`: marca todos os comandos preparados da conexão como
/// expirados. Expirado quer dizer que se recomenda recompilar. Com `i_code == 1` a expiração é
/// consultiva: o comando deve ser reprocessado antes de reiniciar, mas se já está rodando pode
/// terminar. O campo `Vdbe.expired` recebe 1 para expiração imediata e 2 para a consultiva.
pub fn expire_prepared_statements(db: &mut Connection, i_code: i32) {
    for (_, p) in db.stmts.iter_mut() {
        p.expired = (i_code + 1) as u8;
    }
}

/// `sqlite3VdbeGetBoundValue`: o valor da variável ligada `i_var` (a partir de 1) do `Vdbe`,
/// como uma célula nova, depois de aplicar a afinidade `aff`. `None` se `v` é nulo ou se o valor
/// é `NULL` do SQL.
pub fn vdbe_get_bound_value(v: Option<&Vdbe>, i_var: i32, aff: u8) -> Option<Mem> {
    debug_assert!(i_var > 0);
    let v = v?;
    let p_mem = &v.a_var[(i_var - 1) as usize];
    if p_mem.flags & MEM_NULL == 0 {
        let mut p_ret = Mem::value_new();
        mem_copy(&mut p_ret, p_mem);
        apply_affinity(&mut p_ret, aff, ENC_UTF8);
        return Some(p_ret);
    }
    None
}

/// `sqlite3VdbeSetVarmask`: configura a variável `i_var` de modo que ligar um valor novo a ela
/// avise o `sqlite3_reoptimize()` de que reprocessar o comando pode dar um plano melhor.
pub fn vdbe_set_varmask(v: &mut Vdbe, i_var: i32) {
    debug_assert!(i_var > 0);
    if i_var >= 32 {
        v.exp_mask |= 0x8000_0000;
    } else {
        v.exp_mask |= 1u32 << (i_var - 1);
    }
}

/// `sqlite3NotPureFunc`: faz a função lançar um erro se foi chamada por `OP_PureFunc` em vez de
/// `OP_Function`. `OP_PureFunc` quer dizer que a função tem de ser determinística e deve falhar
/// com entradas que a tornariam não determinística (as funções de data e hora que usam `'now'`).
/// `p5` é o `p5` do opcode em execução. Devolve 1 se pode seguir, 0 se gravou o erro.
pub fn not_pure_func(ctx: &mut Context<'_>, p5: u16) -> i32 {
    if ctx.is_pure_func {
        let z_context: &[u8] = if (p5 as i32 & NC_ISCHECK) != 0 {
            b"a CHECK constraint"
        } else if (p5 as i32 & NC_GENCOL) != 0 {
            b"a generated column"
        } else {
            b"an index"
        };
        let z_msg = mprintf(
            b"non-deterministic use of %s() in %s",
            &[
                PrintfArg::Text(Some(ctx.arg_func.z_name.clone())),
                PrintfArg::Text(Some(z_context.to_vec())),
            ],
        );
        // `sqlite3_result_error(pCtx, zMsg, -1)`.
        ctx.is_error = SQLITE_ERROR;
        let limit = ctx.db.a_limit[SQLITE_LIMIT_LENGTH as usize];
        mem_set_str(&mut ctx.out, z_msg.as_deref(), -1, ENC_UTF8, StrDtor::Transient, limit);
        return 0;
    }
    1
}

/// `sqlite3VtabImportErrmsg`: passa o texto de erro de `sqlite3_vtab.zErrMsg` (`vtab_z_err_msg`)
/// para `Vdbe.z_err_msg`. O texto é movido: o `zErrMsg` da tabela virtual fica `None`.
pub fn vtab_import_errmsg(p: &mut Vdbe, vtab_z_err_msg: &mut Option<Vec<u8>>) {
    if let Some(mut z) = vtab_z_err_msg.take() {
        // O C duplica a string terminada em zero (`sqlite3DbStrDup`).
        if let Some(i) = z.iter().position(|&c| c == 0) {
            z.truncate(i);
        }
        p.z_err_msg = Some(z);
    }
}

/// `vdbeFreeUnpacked`: libera as alocações das `n_field` primeiras células de `p.a_mem` e o
/// próprio `UnpackedRecord`. Usada para liberar o que `vdbeUnpackRecord()` (`vdbeapi.c`) criou.
pub fn vdbe_free_unpacked(n_field: usize, p: Option<Box<UnpackedRecord>>) {
    if let Some(mut p) = p {
        for p_mem in p.a_mem.iter_mut().take(n_field) {
            if p_mem.sz_malloc != 0 {
                mem_release_malloc(p_mem);
            }
        }
    }
}

/// `sqlite3VdbePreUpdateHook`: chama o gancho de pré-atualização. Em UPDATE ou DELETE o cursor
/// `csr` aponta para a linha prestes a ser alterada ou apagada: se a aplicação chamar
/// `sqlite3_preupdate_old()`, o valor é lido da linha do cursor. `v` é o comando (`stmt` é o
/// handle dele em `Connection.stmts`, que o `PreUpdate` guarda), `i_key1` a chave inicial e
/// `i_reg` o registro do `new.*`.
#[allow(clippy::too_many_arguments)]
pub fn vdbe_pre_update_hook(
    db: &mut Connection,
    v: &Vdbe,
    stmt: StmtId,
    csr: &VdbeCursor,
    op: i32,
    p_tab: &Rc<Table>,
    i_key1: i64,
    i_reg: i32,
    i_blob_write: i32,
) {
    let mut i_key1 = i_key1;
    let i_key2: i64;
    let mut preupdate = PreUpdate::default();

    debug_assert!(db.p_pre_update.is_none());
    if !p_tab.has_rowid() {
        i_key1 = 0;
        i_key2 = 0;
        preupdate.p_pk = primary_key_index(p_tab).cloned();
    } else if op == SQLITE_UPDATE {
        i_key2 = v.a_mem[i_reg as usize].u_i;
    } else {
        i_key2 = i_key1;
    }

    preupdate.stmt = Some(stmt);
    preupdate.i_db = csr.i_db as i32;
    preupdate.cursor = csr.p_cursor;
    preupdate.op = op;
    preupdate.i_new_reg = i_reg;
    // `fakeSortOrder`: um único byte de ordem zero.
    preupdate.keyinfo = Some(Rc::new(KeyInfo {
        enc: db.enc,
        n_key_field: p_tab.n_col as u16,
        n_all_field: 0,
        a_sort_flags: vec![0],
        a_coll: Vec::new(),
    }));
    preupdate.i_key1 = i_key1;
    preupdate.i_key2 = i_key2;
    preupdate.p_tab = Some(Rc::clone(p_tab));
    preupdate.i_blob_write = i_blob_write;

    // O gancho recebe o `PreUpdate` por empréstimo exclusivo (ver o cabeçalho do módulo).
    if let Some(mut x_pre_update) = db.x_pre_update_callback.take() {
        x_pre_update(db, &mut preupdate);
        db.x_pre_update_callback = Some(x_pre_update);
    }
    preupdate.a_record = Vec::new();
    let n_field = preupdate.keyinfo.as_ref().map_or(0, |k| k.n_key_field as usize) + 1;
    vdbe_free_unpacked(n_field, preupdate.p_unpacked.take());
    vdbe_free_unpacked(n_field, preupdate.p_new_unpacked.take());
    for p_mem in preupdate.a_new.iter_mut().take(csr.n_field.max(0) as usize) {
        mem_release(p_mem);
    }
    preupdate.a_new = Vec::new();
}
