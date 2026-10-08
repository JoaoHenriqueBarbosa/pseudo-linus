//! `fkey.c`: chunks `fkey_c.000` a `fkey_c.003` do SQLite 3.46.1. Suporte a chaves estrangeiras
//! no gerador de código: conferência das restrições (`sqlite3FkCheck`), ações ON DELETE e ON
//! UPDATE (`sqlite3FkActions`) e consultas que o gerador de UPDATE e DELETE faz.
//!
//! Convenções (as mesmas de `delete.rs`, `insert.rs` e `trigger.rs`, ver CONVENTIONS.md):
//!
//! - Funções de código recebem `(db: &mut Connection, parse: &mut Parse, ...)`.
//! - `FKey.pNextFrom` é `TabInfo.p_f_key` (um `Vec<Rc<FKey>>`); `pNextTo` e `pPrevTo` são a lista
//!   `Schema.fkey_hash` sob o nome da tabela pai (`fk_references`). `FKey.pFrom` é o nome
//!   `z_from`: a tabela filha mora no mesmo `Schema` da pai e é achada pelo nome
//!   (`fk_from_table`).
//! - `aChange` é `Option<&[i32]>`, e `aiCol` é um `Vec<i32>` (o `Option` é o ponteiro nulo do
//!   caso de uma coluna só).
//! - `FKey.apTrigger` (o cache dos gatilhos de ação) não existe: o `FKey` é imutável e
//!   compartilhado. O gatilho de ação é refeito a cada `fk_action_trigger` e leva em
//!   `Trigger.z_table` uma chave que o identifica (`fk_trigger_key`); `trigger.rs` reconhece dois
//!   gatilhos de mesma chave como o mesmo (`same_trigger`), então o subprograma é reaproveitado
//!   dentro do `Parse` por `Parse.p_trigger_prg`, que faz as vezes do cache. Por isso
//!   `fk_clear_trigger_cache` não tem o que limpar.
//! - Sem `SQLITE_OMIT_FOREIGN_KEY`/`SQLITE_OMIT_TRIGGER`; `SQLITE_OMIT_AUTHORIZATION` desligada.

use std::rc::Rc;

use crate::auth::auth_read_col;
use crate::build::{
    column_coll, column_expr, find_table, locate_table, table_column_to_storage, table_lock,
    text_arg,
};
use crate::build3::{halt_constraint, may_abort, src_list_append};
use crate::connection::{Connection, Parse};
use crate::consts::{
    COLFLAG_GENERATED, COLFLAG_PRIMKEY, OE_ABORT, OE_CASCADE, OE_NONE, OE_RESTRICT, OE_SET_DFLT,
    OE_SET_NULL, OP_CLOSE, OP_COPY, OP_EQ, OP_AFFINITY, OP_FKCOUNTER,
    OP_FKIFZERO, OP_FOUND, OP_ISNULL, OP_MUSTBEINT, OP_NE, OP_NOTEXISTS, OP_OPENREAD, OP_SCOPY,
    P4_STATIC, P5_CONSTRAINTFK, SQLITE_AFF_INTEGER, SQLITE_CONSTRAINT_FOREIGNKEY, SQLITE_DEFER_FKS,
    SQLITE_FK_NO_ACTION, SQLITE_FOREIGN_KEYS, SQLITE_IGNORE, SQLITE_JUMPIFNULL, SQLITE_NOTNULL,
    TK_COLUMN, TK_DELETE, TK_DOT, TK_EQ, TK_ID, TK_IS, TK_NE, TK_NOT, TK_NULL, TK_RAISE,
    TK_REGISTER, TK_SELECT, TK_UPDATE,
};
use crate::delete::delete_from;
use crate::expr::{
    expr, expr_add_collate_string, expr_alloc, expr_and, expr_dup, expr_list_append,
    expr_list_set_name, p_expr, src_list_dup,
};
use crate::expr_code2::{get_temp_range, get_temp_reg, release_temp_range, release_temp_reg};
use crate::hash::{hash_find, hash_find_mut, hash_insert};
use crate::insert::{index_affinity_str, open_table};
use crate::prepare::schema_to_index;
use crate::resolve::{name_context_new, resolve_expr_names};
use crate::select::{get_vdbe, select_new};
use crate::sqlite_int::{
    Expr, ExprList, ExprY, FKey, Index, SrcList, TabRef, Table, Token, Trigger, TriggerStep,
};
use crate::trigger::{code_row_trigger_direct, col_name};
use crate::util::{error_msg, str_icmp};
use crate::vdbe_types::P4;
use crate::vdbeaux::{
    add_op1, add_op2, add_op3, add_op4, add_op4_int, change_p5, current_addr, jump_here,
    jump_here_or_pop_inst, make_label, resolve_label, set_p4_key_info, vdbe_goto, vdbe_of_parse,
};
use crate::where3::{where_begin, where_end};

/// `COLUMN_MASK(x)`.
#[inline]
fn column_mask(x: i32) -> u32 {
    if x > 31 {
        0xffff_ffff
    } else {
        1u32 << x
    }
}

/// O índice, em `Connection.dbs`, do banco que guarda a tabela `p_tab`.
fn db_index_of(db: &Connection, p_tab: &Table) -> usize {
    schema_to_index(db, p_tab.p_schema).max(0) as usize
}

/// `sqlite3FkReferences`: a lista de `FKey` (a lista `pNextTo` do C) com todas as filhas da
/// tabela `p_tab`. Dado o esquema
///
///   CREATE TABLE t1(a PRIMARY KEY);
///   CREATE TABLE t2(b REFERENCES t1(a);
///
/// chamada com "t1" devolve a chave de "t2"; com "t2" devolve a lista vazia (o ponteiro nulo).
pub fn fk_references(db: &Connection, p_tab: &Table) -> Vec<Rc<FKey>> {
    let i_db = db_index_of(db, p_tab);
    hash_find(&db.dbs[i_db].schema.fkey_hash, &p_tab.z_name).cloned().unwrap_or_default()
}

/// O `FKey.pFrom` do C: a tabela filha da chave, do mesmo esquema da tabela pai `p_tab`.
fn fk_from_table(db: &Connection, p_tab: &Table, p_fkey: &FKey) -> Option<Rc<Table>> {
    let i_db = db_index_of(db, p_tab);
    hash_find(&db.dbs[i_db].schema.tbl_hash, &p_fkey.z_from).cloned()
}

/// As chaves estrangeiras da tabela filha `p_tab` (`pTab->u.tab.pFKey`).
fn fk_of_table(p_tab: &Table) -> &[Rc<FKey>] {
    p_tab.u_tab().map_or(&[], |t| &t.p_f_key[..])
}

/// A chave que identifica o gatilho de ação de `p_fkey` para `i_action` (0 DELETE, 1 UPDATE);
/// o papel do slot `FKey.apTrigger[i_action]`.
fn fk_trigger_key(p_tab: &Table, p_fkey: &FKey, i_action: usize) -> Vec<u8> {
    let mut k: Vec<u8> = b"fk\0".to_vec();
    k.extend_from_slice(&p_tab.p_schema.0.to_le_bytes());
    k.extend_from_slice(&p_fkey.z_from);
    k.push(0);
    k.extend_from_slice(&p_fkey.z_to);
    k.push(0);
    k.push(i_action as u8);
    for c in p_fkey.a_col.iter() {
        k.extend_from_slice(&c.i_from.to_le_bytes());
        if let Some(z) = c.z_col.as_deref() {
            k.extend_from_slice(z);
        }
        k.push(0);
    }
    k
}

// ---------------------------------------------------------------------------------------------
// chunk 000: FkLocateIndex, fkLookupParent
// ---------------------------------------------------------------------------------------------

/// `sqlite3FkLocateIndex`: uma chave estrangeira exige que as colunas da chave na tabela pai
/// estejam sujeitas, juntas, a uma restrição UNIQUE ou PRIMARY KEY. Dado que `p_parent` é a
/// tabela pai da restrição `p_fkey`, procura no esquema um índice único nas colunas da chave pai.
///
/// Se tem êxito devolve zero. Se a chave pai é uma coluna INTEGER PRIMARY KEY, `*pp_idx` fica
/// `None`; senão aponta para o índice único.
///
/// Se a chave pai tem uma coluna só (a chave estrangeira não é composta) e `pai_col` é pedido,
/// `*pai_col` fica `None`. Senão é um array de tamanho N (o número de colunas da chave pai) cujo
/// primeiro elemento é o índice da coluna da tabela filha que a restrição mapeia para a coluna
/// mais à esquerda de `*pp_idx`, o segundo para a segunda, e assim por diante.
///
/// Se o índice exigido não existe, porque (1) as colunas da chave pai nomeadas não existem, (2)
/// existem mas não têm UNIQUE nem PRIMARY KEY, (3) a definição não deu colunas e a tabela pai não
/// tem PRIMARY KEY, ou (4) não deu colunas e a PRIMARY KEY da tabela pai tem número de colunas
/// diferente do da chave filha, devolve não zero e deixa o erro "foreign key mismatch" em
/// `parse`.
pub fn fk_locate_index(
    db: &mut Connection,
    parse: &mut Parse,
    p_parent: &Table,
    p_fkey: &FKey,
    pp_idx: &mut Option<Rc<Index>>,
    pai_col: Option<&mut Option<Vec<i32>>>,
) -> i32 {
    let n_col = p_fkey.a_col.len();
    let z_key: Option<&[u8]> = p_fkey.a_col[0].z_col.as_deref();
    let mut ai_col: Option<Vec<i32>> = None;
    let want_ai_col = pai_col.is_some();

    debug_assert!(pp_idx.is_none());

    // Se a chave é de uma coluna só, confere se mapeia para a INTEGER PRIMARY KEY da pai. Se sim,
    // deixa `*pp_idx` e `*pai_col` nulos e volta. Senão, numa chave composta, aloca o array.
    if n_col == 1 {
        // A chave mapeia para a IPK se (1) há uma coluna INTEGER PRIMARY KEY e a chave mapeia
        // implicitamente para a PRIMARY KEY da pai, ou (2) mapeia explicitamente para uma coluna
        // declarada INTEGER PRIMARY KEY.
        if p_parent.i_p_key >= 0 {
            let Some(z_key) = z_key else {
                return 0;
            };
            if str_icmp(col_name(&p_parent.a_col[p_parent.i_p_key as usize]), z_key) == 0 {
                return 0;
            }
        }
    } else if want_ai_col {
        debug_assert!(n_col > 1);
        ai_col = Some(vec![0i32; n_col]);
    }

    let mut found: Option<Rc<Index>> = None;
    for p_idx in p_parent.p_index.iter() {
        if p_idx.n_key_col as usize == n_col
            && p_idx.is_unique_index()
            && p_idx.p_partial_idx_where.is_none()
        {
            // `p_idx` é UNIQUE (ou PRIMARY KEY) e tem o número certo de colunas. Se cada coluna
            // indexada corresponde a uma coluna da chave de `p_fkey`, o índice serve.
            match z_key {
                None => {
                    // Sem `z_key` a chave mapeia implicitamente para a PRIMARY KEY da pai, que se
                    // identifica pelo teste.
                    if p_idx.is_primary_key_index() {
                        if let Some(a) = ai_col.as_mut() {
                            for i in 0..n_col {
                                a[i] = p_fkey.a_col[i].i_from;
                            }
                        }
                        found = Some(Rc::clone(p_idx));
                        break;
                    }
                }
                Some(_) => {
                    // A chave foi declarada com uma lista explícita de colunas da pai. Confere se
                    // este índice casa com elas e se usa a colação padrão de cada coluna.
                    let mut i = 0usize;
                    while i < n_col {
                        let i_col = p_idx.ai_column[i]; // coluna na tabela pai
                        if i_col < 0 {
                            break; // sem chave estrangeira contra índice de expressão
                        }
                        // Se o índice usa colação diferente da padrão da coluna, não serve.
                        let p_col = &p_parent.a_col[i_col as usize];
                        let z_dflt_coll: &[u8] = column_coll(p_col).unwrap_or(b"BINARY");
                        if str_icmp(&p_idx.az_coll[i], z_dflt_coll) != 0 {
                            break;
                        }
                        let z_idx_col = col_name(p_col); // nome da coluna indexada
                        let mut j = 0usize;
                        while j < n_col {
                            if str_icmp(
                                p_fkey.a_col[j].z_col.as_deref().unwrap_or(&[]),
                                z_idx_col,
                            ) == 0
                            {
                                if let Some(a) = ai_col.as_mut() {
                                    a[i] = p_fkey.a_col[j].i_from;
                                }
                                break;
                            }
                            j += 1;
                        }
                        if j == n_col {
                            break;
                        }
                        i += 1;
                    }
                    if i == n_col {
                        found = Some(Rc::clone(p_idx)); // o índice serve
                        break;
                    }
                }
            }
        }
    }

    let Some(p_idx) = found else {
        if parse.disable_triggers == 0 {
            error_msg(
                db,
                parse,
                b"foreign key mismatch - \"%w\" referencing \"%w\"",
                &[text_arg(&p_fkey.z_from), text_arg(&p_fkey.z_to)],
            );
        }
        return 1;
    };

    *pp_idx = Some(p_idx);
    if let Some(out) = pai_col {
        *out = ai_col;
    }
    0
}

/// `fkLookupParent`: chamada quando uma linha é inserida ou apagada na tabela filha da chave
/// `p_fkey`. Num UPDATE da tabela filha é chamada duas vezes por linha: uma para "apagar" a
/// linha velha e outra para "inserir" a nova.
///
/// Cada vez gera o código que localiza, na tabela pai, a linha que corresponde à linha
/// inserida ou apagada na filha. Se acha, nada especial. Senão:
///
///   Operação | Tipo da FK | Ação
///   --------------------------------------------------------------------------
///   INSERT      imediata    Incrementa o contador de restrições imediatas.
///   DELETE      imediata    Decrementa o contador de restrições imediatas.
///   INSERT      adiada      Incrementa o contador de restrições adiadas.
///   DELETE      adiada      Decrementa o contador de restrições adiadas.
///
/// Estas operações são as "I.1" e "D.1" do comentário do topo do `fkey.c`.
///
/// `p_tab` é a tabela pai, `p_from` a filha, `p_idx` o índice único nas colunas da chave pai,
/// `ai_col` mapeia as colunas da chave pai às da filha, `reg_data` é o endereço do array com a
/// linha da filha, `n_incr` é o incremento do contador e `is_ignore` finge que `p_tab` só tem
/// valores NULL.
fn fk_lookup_parent(
    db: &mut Connection,
    parse: &mut Parse,
    i_db: i32,
    p_tab: &Rc<Table>,
    p_from: &Table,
    p_idx: Option<&Rc<Index>>,
    p_fkey: &FKey,
    ai_col: &[i32],
    reg_data: i32,
    n_incr: i32,
    is_ignore: bool,
) {
    let n_col = p_fkey.a_col.len();
    get_vdbe(db, parse);
    let i_cur = parse.n_tab - 1; // cursor a usar
    let i_ok = make_label(parse); // salta para cá se achou a chave pai
    let same_table = str_icmp(&p_tab.z_name, &p_fkey.z_from) == 0;

    // Se `n_incr` é negativo, confere em tempo de execução se há restrições pendentes. Se não há,
    // não precisa ver se apagar esta linha resolve alguma violação.
    //
    // Confere também se alguma coluna da chave na linha filha é NULL. Se for, a restrição está
    // satisfeita e não é preciso procurar na pai.
    if n_incr < 0 {
        add_op2(vdbe_of_parse(parse), OP_FKIFZERO as i32, p_fkey.is_deferred as i32, i_ok);
    }
    for i in 0..n_col {
        let i_reg =
            table_column_to_storage(p_from, ai_col[i] as i16) as i32 + reg_data + 1;
        add_op2(vdbe_of_parse(parse), OP_ISNULL as i32, i_reg, i_ok);
    }

    if !is_ignore {
        match p_idx {
            None => {
                // Sem índice, a chave pai é a INTEGER PRIMARY KEY da tabela pai.
                let reg_temp = get_temp_reg(parse);

                // `OP_MustBeInt` converte o valor da chave filha para inteiro (aplica a
                // afinidade da chave pai). Se falha, não há chave pai que case. Antes copia o
                // valor, senão a coluna da filha ganharia afinidade INTEGER, o que pode estar
                // errado.
                let src = table_column_to_storage(p_from, ai_col[0] as i16) as i32 + 1 + reg_data;
                add_op2(vdbe_of_parse(parse), OP_SCOPY as i32, src, reg_temp);
                let i_must_be_int = add_op2(vdbe_of_parse(parse), OP_MUSTBEINT as i32, reg_temp, 0);

                // Se a pai é a mesma tabela da filha e vai incrementar o contador (um INSERT),
                // confere se a linha inserida casa com ela mesma. Se casa, não incrementa.
                if same_table && n_incr == 1 {
                    let v = vdbe_of_parse(parse);
                    add_op3(v, OP_EQ as i32, reg_data, i_ok, reg_temp);
                    change_p5(v, SQLITE_NOTNULL as u16);
                }

                open_table(db, parse, i_cur, i_db, p_tab, OP_OPENREAD);
                let v = vdbe_of_parse(parse);
                add_op3(v, OP_NOTEXISTS as i32, i_cur, 0, reg_temp);
                vdbe_goto(v, i_ok);
                let a = v.n_op() - 2;
                jump_here(v, a);
                jump_here(v, i_must_be_int);
                release_temp_reg(parse, reg_temp);
            }
            Some(p_idx) => {
                let reg_temp = get_temp_range(parse, n_col as i32);

                add_op3(vdbe_of_parse(parse), OP_OPENREAD as i32, i_cur, p_idx.tnum as i32, i_db);
                set_p4_key_info(parse, db, p_idx);
                for i in 0..n_col {
                    let src =
                        table_column_to_storage(p_from, ai_col[i] as i16) as i32 + 1 + reg_data;
                    add_op2(vdbe_of_parse(parse), OP_COPY as i32, src, reg_temp + i as i32);
                }

                // Se a pai é a mesma tabela da filha e vai incrementar o contador (um INSERT),
                // confere se a linha inserida casa com ela mesma. Se casa, não incrementa.
                //
                // Se algum valor da chave pai é NULL, a linha não pode casar consigo mesma;
                // `SQLITE_JUMPIFNULL` garante o `OP_Found` se algum valor é NULL (a esta altura
                // se sabe que nenhum valor da chave filha é).
                if same_table && n_incr == 1 {
                    let i_jump = current_addr(parse) + n_col as i32 + 1;
                    for i in 0..n_col {
                        let i_child =
                            table_column_to_storage(p_from, ai_col[i] as i16) as i32 + 1 + reg_data;
                        let mut i_parent = 1 + reg_data;
                        i_parent += table_column_to_storage(p_tab, p_idx.ai_column[i]) as i32;
                        debug_assert!(p_idx.ai_column[i] >= 0);
                        debug_assert!(ai_col[i] != p_tab.i_p_key as i32);
                        if p_idx.ai_column[i] == p_tab.i_p_key {
                            // A chave pai é composta e inclui a coluna IPK.
                            i_parent = reg_data;
                        }
                        let v = vdbe_of_parse(parse);
                        add_op3(v, OP_NE as i32, i_child, i_jump, i_parent);
                        change_p5(v, SQLITE_JUMPIFNULL as u16);
                    }
                    vdbe_goto(vdbe_of_parse(parse), i_ok);
                }

                let z_aff = {
                    let mut z = index_affinity_str(p_idx, p_tab);
                    z.truncate(n_col);
                    z
                };
                let v = vdbe_of_parse(parse);
                add_op4(v, OP_AFFINITY as i32, reg_temp, n_col as i32, 0, P4::Text(z_aff));
                add_op4_int(v, OP_FOUND as i32, i_cur, i_ok, reg_temp, n_col as i32);
                release_temp_range(parse, reg_temp, n_col as i32);
            }
        }
    }

    if p_fkey.is_deferred == 0
        && (db.flags & SQLITE_DEFER_FKS) == 0
        && parse.p_toplevel.is_none()
        && parse.is_multi_write == 0
    {
        // Caso especial: um INSERT de uma só linha levanta a restrição na hora em vez de
        // incrementar um contador. É preciso porque o código gerado não abre transação de
        // comando.
        debug_assert!(n_incr == 1);
        halt_constraint(
            parse,
            SQLITE_CONSTRAINT_FOREIGNKEY,
            OE_ABORT as i32,
            None,
            P4_STATIC,
            P5_CONSTRAINTFK,
        );
    } else {
        if n_incr > 0 && p_fkey.is_deferred == 0 {
            may_abort(parse);
        }
        add_op2(vdbe_of_parse(parse), OP_FKCOUNTER as i32, p_fkey.is_deferred as i32, n_incr);
    }

    resolve_label(parse, db, i_ok);
    add_op1(vdbe_of_parse(parse), OP_CLOSE as i32, i_cur);
}

// ---------------------------------------------------------------------------------------------
// chunk 001: exprTableRegister, fkScanChildren, FkDropTable
// ---------------------------------------------------------------------------------------------

/// `exprTableRegister`: uma expressão que se refere ao registrador que corresponde à coluna
/// `i_col` da tabela `p_tab`. `reg_base` é o primeiro de um array de registradores com os dados
/// de `p_tab`: `reg_base` guarda o rowid, `reg_base+1` a primeira coluna e assim por diante.
fn expr_table_register(
    db: &Connection,
    p_tab: &Table,
    reg_base: i32,
    i_col: i16,
) -> Option<Box<Expr>> {
    let mut p_expr = expr(TK_REGISTER as i32, None);
    let mut z_coll: Option<Vec<u8>> = None;
    if let Some(e) = p_expr.as_deref_mut() {
        if i_col >= 0 && i_col != p_tab.i_p_key {
            let p_col = &p_tab.a_col[i_col as usize];
            e.i_table = reg_base + table_column_to_storage(p_tab, i_col) as i32 + 1;
            e.aff_expr = p_col.affinity;
            z_coll = Some(match column_coll(p_col) {
                Some(z) => z.to_vec(),
                None => db.p_dflt_coll.as_ref().map(|c| c.name.clone()).unwrap_or_default(),
            });
        } else {
            e.i_table = reg_base;
            e.aff_expr = SQLITE_AFF_INTEGER;
        }
    }
    match z_coll {
        Some(z) => expr_add_collate_string(p_expr, &z),
        None => p_expr,
    }
}

/// `exprTableColumn`: uma expressão que se refere à coluna `i_col` da tabela `p_tab`, que tem o
/// cursor `i_cursor`.
fn expr_table_column(p_tab: &Rc<Table>, i_cursor: i32, i_col: i16) -> Option<Box<Expr>> {
    let mut p_expr = expr(TK_COLUMN as i32, None);
    if let Some(e) = p_expr.as_deref_mut() {
        e.y = ExprY::Tab(Some(TabRef::Rc(Rc::clone(p_tab))));
        e.i_table = i_cursor;
        e.i_column = i_col as i32;
    }
    p_expr
}

/// `fkScanChildren`: gera o código executado quando uma linha é apagada da tabela pai da chave
/// `p_fkey` e, se `p_fkey` é adiada, quando uma linha é inserida nela. Num UPDATE de SQL pode ser
/// chamada duas vezes: uma para "apagar" a linha velha e outra para "inserir" a nova.
///
/// `n_incr` é -1 ao inserir uma linha (pode diminuir o número de violações) ou +1 ao apagar (pode
/// aumentar).
///
/// O código gerado percorre as linhas da tabela filha que correspondem à linha pai apagada ou
/// inserida. Para cada filha achada:
///
///   Operação | Tipo da FK | Ação
///   --------------------------------------------------------------------------
///   DELETE      imediata    Incrementa o contador de restrições imediatas.
///   INSERT      imediata    Decrementa o contador de restrições imediatas.
///   DELETE      adiada      Incrementa o contador de restrições adiadas.
///   INSERT      adiada      Decrementa o contador de restrições adiadas.
///
/// Estas operações são as "I.2" e "D.2" do comentário do topo do `fkey.c`.
fn fk_scan_children(
    db: &mut Connection,
    parse: &mut Parse,
    p_src: &mut SrcList,
    p_tab: &Rc<Table>,
    p_idx: Option<&Rc<Index>>,
    p_fkey: &FKey,
    p_from: &Rc<Table>,
    ai_col: Option<&[i32]>,
    reg_data: i32,
    n_incr: i32,
) {
    let mut p_where: Option<Box<Expr>> = None; // WHERE do percurso
    let mut i_fk_if_zero: i32 = 0; // endereço do OP_FkIfZero
    get_vdbe(db, parse);

    debug_assert!(p_idx.map_or(true, |i| i.n_key_col as usize == p_fkey.a_col.len()));
    debug_assert!(p_idx.is_some() || p_fkey.a_col.len() == 1);
    debug_assert!(p_idx.is_some() || p_tab.has_rowid());

    if n_incr < 0 {
        i_fk_if_zero = add_op2(vdbe_of_parse(parse), OP_FKIFZERO as i32, p_fkey.is_deferred as i32, 0);
    }

    // Cria uma expressão como
    //
    //   <chave-pai1> = <chave-filha1> AND <chave-pai2> = <chave-filha2> ...
    //
    // A colação da comparação é a das colunas da chave pai, e a afinidade da coluna da chave pai
    // se aplica a cada valor da chave filha antes de comparar.
    for i in 0..p_fkey.a_col.len() {
        let i_col_p: i16 = p_idx.map_or(-1, |ix| ix.ai_column[i]);
        let p_left = expr_table_register(db, p_tab, reg_data, i_col_p);
        let i_col: i32 = match ai_col {
            Some(a) => a[i],
            None => p_fkey.a_col[0].i_from,
        };
        debug_assert!(i_col >= 0);
        let z_col = col_name(&p_from.a_col[i_col as usize]).to_vec();
        let p_right = expr(TK_ID as i32, Some(&z_col));
        let p_eq = p_expr(db, parse, TK_EQ as i32, p_left, p_right);
        p_where = expr_and(db, parse, p_where, p_eq);
    }

    // Se a filha é a mesma tabela da pai, acrescenta termos ao WHERE que impedem que esta linha
    // seja percorrida:
    //
    //     $current_rowid!=rowid
    //     NOT( $current_a==a AND $current_b==b AND ... )
    //
    // A primeira forma serve a tabelas rowid; a segunda a WITHOUT ROWID, em que a chave a usar é
    // a *pai* (a,b,...), pois os valores já estão em registradores.
    if str_icmp(&p_tab.z_name, &p_fkey.z_from) == 0 && n_incr > 0 {
        let p_ne: Option<Box<Expr>>;
        if p_tab.has_rowid() {
            let p_left = expr_table_register(db, p_tab, reg_data, -1);
            let p_right = expr_table_column(p_tab, p_src.a[0].i_cursor, -1);
            p_ne = p_expr(db, parse, TK_NE as i32, p_left, p_right);
        } else {
            let mut p_all: Option<Box<Expr>> = None;
            let Some(p_idx) = p_idx else {
                debug_assert!(false);
                return;
            };
            for i in 0..p_idx.n_key_col as usize {
                let i_col = p_idx.ai_column[i];
                debug_assert!(i_col >= 0);
                let p_left = expr_table_register(db, p_tab, reg_data, i_col);
                let z = col_name(&p_tab.a_col[i_col as usize]).to_vec();
                let p_right = expr(TK_ID as i32, Some(&z));
                let p_eq = p_expr(db, parse, TK_IS as i32, p_left, p_right);
                p_all = expr_and(db, parse, p_all, p_eq);
            }
            p_ne = p_expr(db, parse, TK_NOT as i32, p_all, None);
        }
        p_where = expr_and(db, parse, p_where, p_ne);
    }

    // Resolve as referências do WHERE.
    {
        let mut s_name_context = name_context_new();
        s_name_context.p_src_list = Some(&mut *p_src);
        resolve_expr_names(db, parse, &mut s_name_context, p_where.as_deref_mut());
    }

    // Gera o VDBE que percorre as entradas de `p_src` que casam com o WHERE. Para cada linha
    // achada, incrementa o contador de chaves estrangeiras adiado ou imediato.
    if parse.n_err == 0 {
        let p_w_info = where_begin(db, parse, p_src, p_where.as_deref_mut(), None, None, None, 0, 0);
        add_op2(vdbe_of_parse(parse), OP_FKCOUNTER as i32, p_fkey.is_deferred as i32, n_incr);
        if let Some(wi) = p_w_info {
            where_end(db, parse, p_src, wi);
        }
    }

    // Limpa o WHERE montado acima (o `Drop`).
    drop(p_where);
    if i_fk_if_zero != 0 {
        jump_here_or_pop_inst(vdbe_of_parse(parse), i_fk_if_zero);
    }
}

/// `sqlite3FkClearTriggerCache`: limpa o cache `apTrigger[]` dos gatilhos CASCADE de todas as
/// chaves estrangeiras de um banco, o que é preciso quando o esquema muda. O cache não existe
/// aqui (ver o cabeçalho do módulo): o gatilho de ação vive só no `Parse.p_trigger_prg` do
/// comando em análise, que some com ele, então não há o que limpar.
pub fn fk_clear_trigger_cache(_db: &mut Connection, _i_db: i32) {}

/// `sqlite3FkDropTable`: gera o código executado quando a tabela `p_tab` é apagada. A `SrcList`
/// `p_name` tem um só item, que se resolve para `p_tab`.
///
/// Normalmente não é preciso código. Mas se (a) a tabela é pai de uma restrição, ou (b) é filha
/// de uma restrição adiada e se descobre em tempo de execução que há violações adiadas pendentes,
/// o equivalente a "DELETE FROM <tbl>" roda antes de apagar a tabela do banco. Os gatilhos ficam
/// desligados nesse DELETE, mas as ações de chave estrangeira não.
pub fn fk_drop_table(db: &mut Connection, parse: &mut Parse, p_name: &SrcList, p_tab: &Table) {
    if (db.flags & SQLITE_FOREIGN_KEYS) != 0 && p_tab.is_ordinary_table() {
        let mut i_skip: i32 = 0;
        get_vdbe(db, parse); // o VDBE já foi alocado

        if fk_references(db, p_tab).is_empty() {
            // Procura uma restrição adiada de que esta tabela é filha. Se não acha, volta sem
            // gerar código. Se acha, pula o DELETE inteiro se na execução não há restrições
            // adiadas pendentes.
            let found = fk_of_table(p_tab)
                .iter()
                .any(|p| p.is_deferred != 0 || (db.flags & SQLITE_DEFER_FKS) != 0);
            if !found {
                return;
            }
            i_skip = make_label(parse);
            add_op2(vdbe_of_parse(parse), OP_FKIFZERO as i32, 1, i_skip);
        }

        parse.disable_triggers = 1;
        delete_from(db, parse, src_list_dup(Some(p_name), 0), None, None, None);
        parse.disable_triggers = 0;

        // Se o DELETE gerou violações imediatas, para o VDBE e devolve o erro aqui, antes de
        // qualquer mudança no esquema: transações de comando não desfazem mudanças de esquema.
        //
        // Com `SQLITE_DeferFKs` isso é desnecessário, pois a transação de comando não é revertida
        // mesmo com violações.
        if (db.flags & SQLITE_DEFER_FKS) == 0 {
            let a = current_addr(parse) + 2;
            add_op2(vdbe_of_parse(parse), OP_FKIFZERO as i32, 0, a);
            halt_constraint(
                parse,
                SQLITE_CONSTRAINT_FOREIGNKEY,
                OE_ABORT as i32,
                None,
                P4_STATIC,
                P5_CONSTRAINTFK,
            );
        }

        if i_skip != 0 {
            resolve_label(parse, db, i_skip);
        }
    }
}

/// `fkChildIsModified`: `p_fkey` é uma chave de que `p_tab` é a filha, e um UPDATE de `p_tab`
/// está em curso. Cada elemento de `a_change` é >= 0 se a coluna é modificada e -1 se não.
/// `b_chng_rowid` é verdadeiro se o UPDATE muda o rowid. Verdadeiro se alguma coluna da chave
/// filha é modificada.
fn fk_child_is_modified(p_tab: &Table, p_fkey: &FKey, a_change: &[i32], b_chng_rowid: i32) -> bool {
    for c in p_fkey.a_col.iter() {
        let i_child_key = c.i_from;
        if a_change[i_child_key as usize] >= 0 {
            return true;
        }
        if i_child_key == p_tab.i_p_key as i32 && b_chng_rowid != 0 {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------------------------
// chunk 002: fkParentIsModified, isSetNullAction, FkCheck, FkOldmask, FkRequired
// ---------------------------------------------------------------------------------------------

/// `fkParentIsModified`: como `fk_child_is_modified`, para uma chave de que `p_tab` é a pai:
/// verdadeiro se alguma coluna da chave pai é modificada.
fn fk_parent_is_modified(p_tab: &Table, p_fkey: &FKey, a_change: &[i32], b_chng_rowid: i32) -> bool {
    for c in p_fkey.a_col.iter() {
        let z_key = c.z_col.as_deref();
        for i_key in 0..p_tab.n_col.max(0) as usize {
            if a_change[i_key] >= 0 || (i_key as i16 == p_tab.i_p_key && b_chng_rowid != 0) {
                let p_col = &p_tab.a_col[i_key];
                match z_key {
                    Some(z) => {
                        if 0 == str_icmp(col_name(p_col), z) {
                            return true;
                        }
                    }
                    None => {
                        if (p_col.col_flags & COLFLAG_PRIMKEY) != 0 {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// `isSetNullAction`: verdadeiro se o `Parse` está gerando um gatilho que na verdade é uma ação
/// "SET NULL" da chave `p_fkey`.
fn is_set_null_action(parse: &Parse, p_tab: &Table, p_fkey: &FKey) -> bool {
    let p_top = parse.toplevel();
    if let Some(prg) = p_top.p_trigger_prg.last() {
        if let Some(p) = prg.p_trigger.as_ref() {
            if p.z_name.is_empty() && !p.z_table.is_empty() {
                for i in 0..2usize {
                    if p.z_table == fk_trigger_key(p_tab, p_fkey, i) && p_fkey.a_action[i] == OE_SET_NULL {
                        // `(db->flags & SQLITE_FkNoAction)==0` vale aqui.
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// `sqlite3FkCheck`: chamada ao inserir, apagar ou atualizar uma linha da tabela `p_tab` para
/// gerar o código VDBE do processamento de chaves estrangeiras.
///
/// Num DELETE, `reg_old` é o primeiro de um array de (`n_col`+1) registradores com o rowid da
/// linha apagada e depois cada coluna, da esquerda para a direita; `reg_new` é zero. Num INSERT,
/// `reg_old` é zero e `reg_new` o primeiro registrador de um array de (`n_col`+1) com a linha
/// nova. Num UPDATE a função é chamada duas vezes: antes de apagar o registro original, com a
/// convenção do DELETE, e depois de apagá-lo mas antes de inserir o novo, com a do INSERT.
pub fn fk_check(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    reg_old: i32,
    reg_new: i32,
    a_change: Option<&[i32]>,
    b_chng_rowid: i32,
) {
    let is_ignore_errors = parse.disable_triggers != 0;

    // Exatamente um entre `reg_old` e `reg_new` é diferente de zero.
    debug_assert!((reg_old == 0) != (reg_new == 0));

    // Sem chaves estrangeiras habilitadas, é um no-op.
    if (db.flags & SQLITE_FOREIGN_KEYS) == 0 {
        return;
    }
    if !p_tab.is_ordinary_table() {
        return;
    }

    let i_db = schema_to_index(db, p_tab.p_schema);
    let z_db = db.dbs[i_db as usize].z_db_s_name.clone();

    // Percorre as restrições de que `p_tab` é a tabela filha (a que contém a definição).
    let child_fkeys: Vec<Rc<FKey>> = fk_of_table(p_tab).to_vec();
    for p_fkey in child_fkeys.iter() {
        let mut p_idx: Option<Rc<Index>> = None; // índice nas colunas da chave em `p_to`
        let mut ai_free: Option<Vec<i32>> = None;
        let mut b_ignore = false;

        if let Some(ac) = a_change {
            if str_icmp(&p_tab.z_name, &p_fkey.z_to) != 0
                && !fk_child_is_modified(p_tab, p_fkey, ac, b_chng_rowid)
            {
                continue;
            }
        }

        // Acha a tabela pai da chave e um índice único nas colunas da chave pai. Se algum dos dois
        // não existe, grava um erro em `parse` e volta cedo.
        let p_to: Option<Rc<Table>> = if parse.disable_triggers != 0 {
            find_table(db, &p_fkey.z_to, Some(&z_db))
        } else {
            locate_table(db, parse, 0, &p_fkey.z_to, Some(&z_db))
        };
        let located = match p_to.as_deref() {
            Some(t) => fk_locate_index(db, parse, t, p_fkey, &mut p_idx, Some(&mut ai_free)) == 0,
            None => false,
        };
        if !located {
            debug_assert!(!is_ignore_errors || (reg_old != 0 && reg_new == 0));
            if !is_ignore_errors || db.malloc_failed != 0 {
                return;
            }
            if p_to.is_none() {
                // Com `is_ignore_errors`, uma tabela está sendo apagada. O SQLite roda um
                // "DELETE FROM xxx" na tabela antes de apagá-la, para conferir as chaves
                // estrangeiras. Se a tabela pai de uma restrição da tabela corrente não existe, se
                // comporta como se ela estivesse vazia: decrementa o contador de FK relevante
                // para cada linha da tabela corrente com chaves não nulas.
                get_vdbe(db, parse);
                let i_jump = current_addr(parse) + p_fkey.a_col.len() as i32 + 1;
                for c in p_fkey.a_col.iter() {
                    let i_from_col = c.i_from;
                    let i_reg =
                        table_column_to_storage(p_tab, i_from_col as i16) as i32 + reg_old + 1;
                    add_op2(vdbe_of_parse(parse), OP_ISNULL as i32, i_reg, i_jump);
                }
                add_op2(vdbe_of_parse(parse), OP_FKCOUNTER as i32, p_fkey.is_deferred as i32, -1);
            }
            continue;
        }
        let p_to = p_to.expect("pTo");
        debug_assert!(p_fkey.a_col.len() == 1 || (ai_free.is_some() && p_idx.is_some()));

        let mut ai_col: Vec<i32> = match ai_free {
            Some(a) => a,
            None => vec![p_fkey.a_col[0].i_from],
        };
        for i in 0..p_fkey.a_col.len() {
            if ai_col[i] == p_tab.i_p_key as i32 {
                ai_col[i] = -1;
            }
            debug_assert!(p_idx.as_ref().map_or(true, |ix| ix.ai_column[i] >= 0));
            // Pede permissão para ler as colunas da chave pai. Se o callback de autorização devolve
            // `SQLITE_IGNORE`, se comporta como se os valores lidos da pai fossem NULL.
            if db.x_auth.is_some() {
                let z_col: Vec<u8> = {
                    let k = match p_idx.as_ref() {
                        Some(ix) => ix.ai_column[i],
                        None => p_to.i_p_key,
                    };
                    col_name(&p_to.a_col[k as usize]).to_vec()
                };
                let rcauth = auth_read_col(db, parse, &p_to.z_name, &z_col, i_db);
                b_ignore = rcauth == SQLITE_IGNORE;
            }
        }

        // Pega uma trava de leitura consultiva (cache compartilhado) na tabela pai. Aloca um
        // cursor para procurar o índice único das colunas da chave pai.
        table_lock(db, parse, i_db, p_to.tnum, false, &p_to.z_name);
        parse.n_tab += 1;

        if reg_old != 0 {
            // Uma linha sai da filha. Procura a pai: se não existe, tirar a linha filha resolve
            // uma violação pendente.
            fk_lookup_parent(
                db, parse, i_db, &p_to, p_tab, p_idx.as_ref(), p_fkey, &ai_col, reg_old, -1,
                b_ignore,
            );
        }
        if reg_new != 0 && !is_set_null_action(parse, p_tab, p_fkey) {
            // Uma linha entra na filha. Se a pai não existe, a linha nova viola a restrição.
            //
            // Se isto roda como parte de um programa de gatilho que é na verdade uma ação "SET
            // NULL" desta mesma chave, omite a busca: todos os valores da chave filha são NULL, e
            // então a linha nova não viola nada.
            fk_lookup_parent(
                db, parse, i_db, &p_to, p_tab, p_idx.as_ref(), p_fkey, &ai_col, reg_new, 1,
                b_ignore,
            );
        }
    }

    // Percorre as restrições que se referem a esta tabela (as restrições "filhas").
    for p_fkey in fk_references(db, p_tab).iter() {
        let mut p_idx: Option<Rc<Index>> = None; // índice da chave de `p_fkey`
        let mut ai_col: Option<Vec<i32>> = None;

        if let Some(ac) = a_change {
            if !fk_parent_is_modified(p_tab, p_fkey, ac, b_chng_rowid) {
                continue;
            }
        }

        if p_fkey.is_deferred == 0
            && (db.flags & SQLITE_DEFER_FKS) == 0
            && parse.p_toplevel.is_none()
            && parse.is_multi_write == 0
        {
            debug_assert!(reg_old == 0 && reg_new != 0);
            // Inserir uma só linha numa tabela pai não causa (nem conserta) uma violação
            // imediata. Não faz nada.
            continue;
        }

        if fk_locate_index(db, parse, p_tab, p_fkey, &mut p_idx, Some(&mut ai_col)) != 0 {
            if !is_ignore_errors || db.malloc_failed != 0 {
                return;
            }
            continue;
        }
        debug_assert!(ai_col.is_some() || p_fkey.a_col.len() == 1);

        // Cria uma `SrcList` com a tabela filha, de que `sqlite3WhereBegin` precisa.
        let Some(p_from) = fk_from_table(db, p_tab, p_fkey) else {
            continue;
        };
        let mut p_src = src_list_append(db, parse, None, None, None);
        if let Some(src) = p_src.as_deref_mut() {
            src.a[0].p_tab = Some(Rc::clone(&p_from));
            src.a[0].z_name = Some(p_from.z_name.clone());
            src.a[0].i_cursor = parse.n_tab;
            parse.n_tab += 1;

            if reg_new != 0 {
                fk_scan_children(
                    db, parse, src, p_tab, p_idx.as_ref(), p_fkey, &p_from, ai_col.as_deref(),
                    reg_new, -1,
                );
            }
            if reg_old != 0 {
                let mut e_action = p_fkey.a_action[(a_change.is_some()) as usize];
                if (db.flags & SQLITE_FK_NO_ACTION) != 0 {
                    e_action = OE_NONE;
                }

                fk_scan_children(
                    db, parse, src, p_tab, p_idx.as_ref(), p_fkey, &p_from, ai_col.as_deref(),
                    reg_old, 1,
                );
                // Se a restrição é adiada, ou se vale uma ação CASCADE ou SET NULL, qualquer
                // violação causada ao tirar a chave pai é corrigida pelo gatilho da ação. Então
                // não liga a flag "may-abort" neste caso.
                //
                // Nota 1: com "ON UPDATE CASCADE", a flag acaba ligada de qualquer jeito (quando
                // esta função é chamada para o UPDATE dentro do gatilho da ação).
                //
                // Nota 2: à primeira vista parece que bastaria omitir todas as varreduras de
                // `OP_FkCounter` com CASCADE ou SET NULL. O problema aparece se a ação dispara
                // outros gatilhos ou regras de ação da filha: os contadores ficam errados se
                // alguma varredura foi omitida.
                if p_fkey.is_deferred == 0 && e_action != OE_CASCADE && e_action != OE_SET_NULL {
                    may_abort(parse);
                }
            }
        }
    }
}

/// `sqlite3FkOldmask`: chamada antes de gerar o código de um UPDATE ou DELETE de uma linha de
/// `p_tab`. Devolve a máscara das colunas da linha velha de que o processamento de chaves
/// estrangeiras precisa.
pub fn fk_oldmask(db: &mut Connection, parse: &mut Parse, p_tab: &Table) -> u32 {
    let mut mask: u32 = 0;
    if (db.flags & SQLITE_FOREIGN_KEYS) != 0 && p_tab.is_ordinary_table() {
        for p in fk_of_table(p_tab).iter() {
            for c in p.a_col.iter() {
                mask |= column_mask(c.i_from);
            }
        }
        for p in fk_references(db, p_tab).iter() {
            let mut p_idx: Option<Rc<Index>> = None;
            fk_locate_index(db, parse, p_tab, p, &mut p_idx, None);
            if let Some(ix) = p_idx {
                for i in 0..ix.n_key_col as usize {
                    debug_assert!(ix.ai_column[i] >= 0);
                    mask |= column_mask(ix.ai_column[i] as i32);
                }
            }
        }
    }
    mask
}

/// `sqlite3FkRequired`: chamada antes de gerar o código de um UPDATE ou DELETE de uma linha de
/// `p_tab`. Num DELETE, `a_change` é `None`; num UPDATE, é um array de tamanho N (N o número de
/// colunas de `p_tab`) em que o elemento i vale -1 se a coluna i não é modificada e >= 0 se é.
/// `chng_rowid` é verdadeiro se o UPDATE modifica o rowid.
///
/// Devolve não zero se algum processamento de chaves estrangeiras é necessário e zero se não.
///
/// Num UPDATE devolve 2 se (a) há chaves de que `p_tab` é filha e pai, e se algum processamento
/// de chaves é necessário (mesmo de outra chave), ou (b) o UPDATE modifica chaves pai cuja ação
/// não é "NO ACTION" (CASCADE, SET DEFAULT ou SET NULL). Se algum outro processamento é
/// necessário, devolve 1.
pub fn fk_required(
    db: &mut Connection,
    _parse: &mut Parse,
    p_tab: &Table,
    a_change: Option<&[i32]>,
    chng_rowid: i32,
) -> i32 {
    let mut e_ret: i32 = 1; // valor devolvido se `b_have_fk`
    let mut b_have_fk = false; // se é preciso processar chaves estrangeiras
    if (db.flags & SQLITE_FOREIGN_KEYS) != 0 && p_tab.is_ordinary_table() {
        match a_change {
            None => {
                // Um DELETE. É preciso processar se a tabela é filha ou pai de alguma chave.
                b_have_fk = !fk_references(db, p_tab).is_empty() || !fk_of_table(p_tab).is_empty();
            }
            Some(ac) => {
                // Um UPDATE. Só é preciso se a operação modifica colunas de chave filha ou pai.
                // Confere se alguma coluna de chave filha é modificada.
                for p in fk_of_table(p_tab).iter() {
                    if fk_child_is_modified(p_tab, p, ac, chng_rowid) {
                        if 0 == str_icmp(&p_tab.z_name, &p.z_to) {
                            e_ret = 2;
                        }
                        b_have_fk = true;
                    }
                }

                // Confere se alguma coluna de chave pai é modificada.
                for p in fk_references(db, p_tab).iter() {
                    if fk_parent_is_modified(p_tab, p, ac, chng_rowid) {
                        if (db.flags & SQLITE_FK_NO_ACTION) == 0 && p.a_action[1] != OE_NONE {
                            return 2;
                        }
                        b_have_fk = true;
                    }
                }
            }
        }
    }
    if b_have_fk {
        e_ret
    } else {
        0
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 003: fkActionTrigger, FkActions, FkDelete
// ---------------------------------------------------------------------------------------------

/// `fkActionTrigger`: chamada ao compilar um UPDATE ou DELETE da tabela `p_tab`, que é a pai da
/// chave `p_fkey`. Num UPDATE, `p_changes` é a lista de colunas modificadas; num DELETE é `None`.
///
/// Devolve um gatilho equivalente à ação ON UPDATE ou ON DELETE da chave. Se a ação é "NO
/// ACTION" devolve `None` (essas ações não exigem tratamento do subsistema de gatilhos: o código
/// delas vem de `fk_scan_children`).
///
/// Por exemplo, com `p_fkey` e `p_tab` ("p") em
///
///   CREATE TABLE p(pk PRIMARY KEY);
///   CREATE TABLE c(ck REFERENCES p ON DELETE CASCADE);
///
/// o gatilho devolvido equivale a
///
///   CREATE TRIGGER ... DELETE ON p BEGIN
///     DELETE FROM c WHERE ck = old.pk;
///   END;
///
/// O C guarda o gatilho em cache no `FKey`; aqui ele é refeito a cada chamada e identificado por
/// `Trigger.z_table` (ver o cabeçalho do módulo).
fn fk_action_trigger(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    p_fkey: &FKey,
    p_changes: Option<&ExprList>,
) -> Option<Rc<Trigger>> {
    let i_action = p_changes.is_some() as usize; // 1 para UPDATE, 0 para DELETE
    let mut action = p_fkey.a_action[i_action]; // OE_None, OE_Cascade etc.
    if (db.flags & SQLITE_FK_NO_ACTION) != 0 {
        action = OE_NONE;
    }
    if action == OE_RESTRICT && (db.flags & SQLITE_DEFER_FKS) != 0 {
        return None;
    }
    if action == OE_NONE {
        return None;
    }

    let mut p_idx: Option<Rc<Index>> = None; // índice da chave pai
    let mut ai_col: Option<Vec<i32>> = None; // colunas filhas para pai
    let mut p_where: Option<Box<Expr>> = None; // WHERE do passo do gatilho
    let mut p_list: Option<Box<ExprList>> = None; // lista de mudanças do ON UPDATE CASCADE
    let mut p_when: Option<Box<Expr>> = None; // WHEN do gatilho

    if fk_locate_index(db, parse, p_tab, p_fkey, &mut p_idx, Some(&mut ai_col)) != 0 {
        return None;
    }
    debug_assert!(ai_col.is_some() || p_fkey.a_col.len() == 1);
    let p_from = fk_from_table(db, p_tab, p_fkey)?;

    let t_old = Token { z: b"old".to_vec(), i_ofst: -1 }; // token literal "old"
    let t_new = Token { z: b"new".to_vec(), i_ofst: -1 }; // token literal "new"
    for i in 0..p_fkey.a_col.len() {
        let i_from_col: i32 = match ai_col.as_deref() {
            Some(a) => a[i],
            None => p_fkey.a_col[0].i_from,
        };
        debug_assert!(i_from_col >= 0);
        debug_assert!(p_idx.is_some() || (p_tab.i_p_key >= 0 && p_tab.i_p_key < p_tab.n_col));
        debug_assert!(p_idx.as_ref().map_or(true, |ix| ix.ai_column[i] >= 0));
        let k = match p_idx.as_ref() {
            Some(ix) => ix.ai_column[i],
            None => p_tab.i_p_key,
        };
        let t_to_col = Token { z: col_name(&p_tab.a_col[k as usize]).to_vec(), i_ofst: -1 };
        let t_from_col =
            Token { z: col_name(&p_from.a_col[i_from_col as usize]).to_vec(), i_ofst: -1 };

        // Cria a expressão "OLD.zToCol = zFromCol". O termo "OLD.zToCol" tem de ficar à esquerda
        // do `=`, para valerem a afinidade e a colação da tabela pai na comparação.
        let p_old_to = {
            let l = expr_alloc(TK_ID as i32, Some(&t_old), 0);
            let r = expr_alloc(TK_ID as i32, Some(&t_to_col), 0);
            p_expr(db, parse, TK_DOT as i32, l, r)
        };
        let p_from_id = expr_alloc(TK_ID as i32, Some(&t_from_col), 0);
        let p_eq = p_expr(db, parse, TK_EQ as i32, p_old_to, p_from_id);
        p_where = expr_and(db, parse, p_where, p_eq);

        // No ON UPDATE, monta o próximo termo do WHEN. O WHEN final fica
        //
        //    WHEN NOT(old.col1 IS new.col1 AND ... AND old.colN IS new.colN)
        if p_changes.is_some() {
            let l = {
                let a = expr_alloc(TK_ID as i32, Some(&t_old), 0);
                let b = expr_alloc(TK_ID as i32, Some(&t_to_col), 0);
                p_expr(db, parse, TK_DOT as i32, a, b)
            };
            let r = {
                let a = expr_alloc(TK_ID as i32, Some(&t_new), 0);
                let b = expr_alloc(TK_ID as i32, Some(&t_to_col), 0);
                p_expr(db, parse, TK_DOT as i32, a, b)
            };
            let p_eq = p_expr(db, parse, TK_IS as i32, l, r);
            p_when = expr_and(db, parse, p_when, p_eq);
        }

        if action != OE_RESTRICT && (action != OE_CASCADE || p_changes.is_some()) {
            let p_new: Option<Box<Expr>>;
            if action == OE_CASCADE {
                let a = expr_alloc(TK_ID as i32, Some(&t_new), 0);
                let b = expr_alloc(TK_ID as i32, Some(&t_to_col), 0);
                p_new = p_expr(db, parse, TK_DOT as i32, a, b);
            } else if action == OE_SET_DFLT {
                let p_col = &p_from.a_col[i_from_col as usize];
                let p_dflt: Option<&Expr> = if (p_col.col_flags & COLFLAG_GENERATED) != 0 {
                    None
                } else {
                    column_expr(&p_from, p_col)
                };
                p_new = match p_dflt {
                    Some(d) => expr_dup(Some(d), 0),
                    None => expr_alloc(TK_NULL as i32, None, 0),
                };
            } else {
                p_new = expr_alloc(TK_NULL as i32, None, 0);
            }
            p_list = expr_list_append(p_list, p_new);
            expr_list_set_name(parse, p_list.as_deref_mut(), &t_from_col, 0);
        }
    }

    let z_from = p_from.z_name.clone();
    let mut p_select = None;
    if action == OE_RESTRICT {
        let i_db = schema_to_index(db, p_tab.p_schema);
        let mut p_raise = expr(TK_RAISE as i32, Some(b"FOREIGN KEY constraint failed"));
        if let Some(r) = p_raise.as_deref_mut() {
            r.aff_expr = OE_ABORT;
        }
        let mut p_src = src_list_append(db, parse, None, None, None);
        if let Some(src) = p_src.as_deref_mut() {
            debug_assert!(src.a.len() == 1);
            src.a[0].z_name = Some(z_from.clone());
            src.a[0].z_database = Some(db.dbs[i_db as usize].z_db_s_name.clone());
        }
        p_select = select_new(
            parse,
            expr_list_append(None, p_raise),
            p_src,
            p_where.take(),
            None,
            None,
            None,
            0,
            None,
        );
    }

    let mut p_step = TriggerStep::default(); // o único passo do programa
    p_step.z_target = Some(z_from);
    p_step.p_where = p_where;
    p_step.p_expr_list = p_list;
    p_step.p_select = p_select;
    let mut p_trigger = Trigger::default();
    if p_when.is_some() {
        p_trigger.p_when = p_expr(db, parse, TK_NOT as i32, p_when, None);
    }
    p_step.op = match action {
        OE_RESTRICT => TK_SELECT,
        OE_CASCADE if p_changes.is_none() => TK_DELETE,
        _ => TK_UPDATE,
    };
    p_trigger.step_list = vec![p_step];
    p_trigger.p_schema = p_tab.p_schema;
    p_trigger.p_tab_schema = p_tab.p_schema;
    p_trigger.op = if p_changes.is_some() { TK_UPDATE } else { TK_DELETE };
    p_trigger.z_table = fk_trigger_key(p_tab, p_fkey, i_action);
    Some(Rc::new(p_trigger))
}

/// `sqlite3FkActions`: chamada ao apagar ou atualizar uma linha para implementar as ações
/// CASCADE, SET NULL ou SET DEFAULT que forem necessárias.
pub fn fk_actions(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    p_changes: Option<&ExprList>,
    reg_old: i32,
    a_change: Option<&[i32]>,
    b_chng_rowid: i32,
) {
    // Com chaves estrangeiras habilitadas, percorre as chaves que se referem a `p_tab`. Se há uma
    // ação para a operação (UPDATE ou DELETE), chama o subprograma do gatilho associado.
    if (db.flags & SQLITE_FOREIGN_KEYS) != 0 {
        for p_fkey in fk_references(db, p_tab).iter() {
            if a_change.map_or(true, |ac| fk_parent_is_modified(p_tab, p_fkey, ac, b_chng_rowid)) {
                if let Some(p_act) = fk_action_trigger(db, parse, p_tab, p_fkey, p_changes) {
                    code_row_trigger_direct(db, parse, &p_act, p_tab, reg_old, OE_ABORT as i32, 0);
                }
            }
        }
    }
}

/// `sqlite3FkDelete`: libera a memória das definições de chaves estrangeiras da tabela `p_tab`.
/// Tira as chaves apagadas de `Schema.fkey_hash`. Os gatilhos de ação (`apTrigger`) não existem
/// aqui, então só a tabela hash é desfeita.
pub fn fk_delete(db: &mut Connection, p_tab: &Table) {
    debug_assert!(p_tab.is_ordinary_table());
    let i_db = schema_to_index(db, p_tab.p_schema);
    if i_db < 0 || (i_db as usize) >= db.dbs.len() {
        return;
    }
    for p_fkey in fk_of_table(p_tab).iter() {
        // Tira a chave da tabela hash `fkey_hash`.
        let hash = &mut db.dbs[i_db as usize].schema.fkey_hash;
        let mut now_empty = false;
        if let Some(list) = hash_find_mut(hash, &p_fkey.z_to) {
            list.retain(|k| !Rc::ptr_eq(k, p_fkey));
            now_empty = list.is_empty();
        }
        if now_empty {
            hash_insert(hash, &p_fkey.z_to, None);
        }

        // EV: R-30323-21917 Cada restrição de chave estrangeira do SQLite é imediata ou adiada.
        debug_assert!(p_fkey.is_deferred == 0 || p_fkey.is_deferred == 1);
    }
}
