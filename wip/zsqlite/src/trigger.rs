//! `trigger.c`: chunks `trigger_c.000` a `trigger_c.004` do SQLite 3.46.1. Gatilhos: construção
//! (`CREATE TRIGGER`), remoção, consulta (`sqlite3TriggersExist`), geração do subprograma de cada
//! gatilho de linha (`sqlite3CodeRowTrigger`) e o código em linha do RETURNING.
//!
//! Convenções (as mesmas de `delete.rs`, `insert.rs` e `build.rs`, ver CONVENTIONS.md):
//!
//! - Funções recebem `(db: &mut Connection, parse: &mut Parse, ...)`; o `pParse->db` some.
//! - A lista de gatilhos que o C passa como `Trigger*` (encadeada por `pNext`) é `&[Rc<Trigger>]`
//!   (ou `Vec<Rc<Trigger>>` no retorno), e o ponteiro nulo é a fatia vazia. A ordem é a do C: os
//!   gatilhos TEMP primeiro, depois `Table.p_trigger`.
//! - `TriggerStep.p_next` só existe enquanto o analisador monta a lista (`trigger_cmd_list`);
//!   `finish_trigger` a desfaz no `Vec` de `Trigger.step_list`. Os spans do SQL (`zStart`,
//!   `zEnd`) são deslocamentos em `Parse.z_sql`.
//! - O gatilho de um RETURNING é `Rc<Trigger>` imutável no esquema TEMP, mas o estado vivo (o
//!   `op`, o `tr_tm`, a tabela) mora em `Parse.p_returning.ret_trig`, que `trigger_list` e
//!   `triggers_exist` atualizam e de onde devolvem uma cópia nova a cada chamada. A identidade do
//!   gatilho é o nome (`Returning.z_name`).
//! - Gatilho sem nome (`Trigger.z_name` vazio) é o `zName==NULL` do C: uma ação de chave
//!   estrangeira montada por `fkey.rs`.
//! - Subprogramas: `codeRowTrigger` move o `Parse` de nível mais alto para dentro do `Parse` do
//!   subprograma (`child.p_toplevel`) e o devolve no fim, como descreve `connection.rs`. Se o
//!   `Parse` corrente já é um subprograma, o de nível mais alto é tirado dele (`take`) e devolvido
//!   no fim, para que `Parse::toplevel` valha sempre o de cima.
//! - `SQLITE_ENABLE_EXPLAIN_COMMENTS` está desligada no Debian: `VdbeComment` e `onErrorText`
//!   não existem.

use std::rc::Rc;

use crate::alter::{rename_token_map, rename_token_remap};
use crate::attach::{db_is_named, fix_expr, fix_init, fix_src_list, fix_trigger_step};
use crate::auth::auth_check;
use crate::build::{
    check_object_name, name_from_token, nested_parse, schema_table, text_arg, token_arg,
    two_part_name,
};
use crate::build2::{change_cookie, has_explicit_nulls, read_only_shadow_tables, shadow_table_name};
use crate::build3::{
    begin_write_operation, code_verify_named_schema, code_verify_schema, id_list_index,
    src_item_arg, src_list_append, src_list_append_from_term, src_list_append_list,
};
use crate::connection::{Connection, Parse, TriggerPrg};
use crate::consts::{
    ENAME_NAME, EP_VAR_SELECT, EXPRDUP_REDUCE, LEGACY_SCHEMA_TABLE, NC_UBASEREG, OE_DEFAULT,
    OMIT_TEMPDB, OP_HALT, OP_INSERT, OP_MAKERECORD, OP_NEWROWID, OP_PROGRAM, OP_REALAFFINITY,
    OP_RESETCOUNT, OP_TRACE, OP_DROPTRIGGER, SF_CORRELATED, SF_NESTEDFROM, SQLITE_AFF_REAL,
    SQLITE_CREATE_TEMP_TRIGGER, SQLITE_CREATE_TRIGGER, SQLITE_DELETE, SQLITE_DROP_TEMP_TRIGGER,
    SQLITE_DROP_TRIGGER, SQLITE_ENABLE_TRIGGER, SQLITE_INSERT, SQLITE_JUMPIFNULL, SQLITE_OK,
    SQLITE_REC_TRIGGERS, SRT_DISCARD, TF_SHADOW, TK_ASTERISK, TK_BEFORE, TK_DELETE, TK_DOT, TK_ID,
    TK_INSERT, TK_INSTEAD, TK_RETURNING, TK_SELECT, TK_UPDATE, TRIGGER_AFTER, TRIGGER_BEFORE,
    WRC_CONTINUE,
};
use crate::ctype::is_space;
use crate::delete::{delete_from, src_list_lookup};
use crate::expr::{
    expr, expr_affinity, expr_dup, expr_list_append, expr_list_dup, id_list_dup, select_dup,
    src_list_dup,
};
use crate::expr_code2::{expr_code_factorable, expr_if_false};
use crate::hash::{hash_find, hash_find_mut, hash_first, hash_insert, hash_iter};
use crate::insert::insert;
use crate::parse_reduce::sql_span;
use crate::prepare::{parse_object_init, parse_object_reset, read_schema, schema_to_index};
use crate::printf::{mprintf, PrintfArg};
use crate::resolve::{name_context_new, resolve_expr_list_names, resolve_expr_names};
use crate::select::{generate_column_names, get_vdbe, select_dest_init, select_new};
use crate::select3::{select, select_prep};
use crate::sqlite_int::{
    Expr, ExprList, ExprX, IdList, NcU, Select, SelectDest, SrcItem, SrcList, Table, Token,
    Trigger, TriggerStep, Upsert, Walker,
};
use crate::update::update;
use crate::upsert::upsert_dup;
use crate::util::{dequote, error_msg, oom_fault, str_icmp, strlen30, strnicmp};
use crate::vdbe_types::{SubProgram, P4};
use crate::vdbeaux::{
    add_op0, add_op1, add_op3, add_op4, add_parse_schema_op, change_p4, change_p5,
    link_sub_program, make_label, resolve_label, take_op_array, vdbe_of_parse,
};
use crate::vdbeaux2::vdbe_delete;
use crate::walker::{expr_walk_noop, select_walk_noop, walk_expr_list};

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------

/// O nome de uma coluna: `Column.z_cn_name` guarda `nome\0[tipo\0][colação\0]` e o `zCnName` do C
/// lê só até o primeiro NUL.
pub(crate) fn col_name(p_col: &crate::sqlite_int::Column) -> &[u8] {
    &p_col.z_cn_name[..strlen30(&p_col.z_cn_name) as usize]
}

/// O que `getRowTrigger` e `codeRowTrigger` devolvem do `TriggerPrg`: o subprograma (ainda
/// `None` se o gatilho está em geração ou se não houve VDBE) e as máscaras de colunas.
struct PrgInfo {
    /// O subprograma gerado.
    p_program: Option<Rc<SubProgram>>,
    /// `TriggerPrg.aColmask`: colunas `old.*` e `new.*` acessadas.
    a_colmask: [u32; 2],
}

/// A identidade do gatilho para `SubProgram.token` (o `(void*)pTrigger` do C).
fn trigger_token(p_trigger: &Rc<Trigger>) -> usize {
    Rc::as_ptr(p_trigger) as usize
}

// ---------------------------------------------------------------------------------------------
// chunk 000: lista de gatilhos, BeginTrigger, FinishTrigger
// ---------------------------------------------------------------------------------------------

/// `sqlite3TriggerList`: dada a tabela `p_tab`, devolve todos os gatilhos ligados a ela.
///
/// Os gatilhos de `p_tab` que estão no mesmo banco que ela já estão em `p_tab.p_trigger`, mas
/// pode haver outros em TEMP. Esta rotina antepõe os gatilhos TEMP de `p_tab` à lista (na ordem
/// inversa da iteração da tabela hash, como o C, que os prefixa um a um) e devolve a lista
/// combinada.
///
/// O gatilho do RETURNING do comando em análise vive em `Parse.p_returning`; aqui ele ganha, na
/// primeira vez que aparece, a tabela e o esquema da tabela `p_tab`.
pub fn trigger_list(db: &Connection, parse: &mut Parse, p_tab: &Table) -> Vec<Rc<Trigger>> {
    debug_assert!(parse.disable_triggers == 0);
    let tmp_schema = db.dbs[1].schema.id; // esquema TEMP
    let mut temps: Vec<Rc<Trigger>> = Vec::new();
    for (_, p_trig) in hash_iter(&db.dbs[1].schema.trig_hash) {
        // O gatilho de RETURNING no esquema é só o rastro; a cópia viva é a do `Parse`.
        let cand: Rc<Trigger> = if p_trig.b_returning != 0 {
            match parse.toplevel().p_returning.as_deref() {
                Some(r) if r.ret_trig.z_name == p_trig.z_name => Rc::new(r.ret_trig.clone()),
                _ => continue,
            }
        } else {
            Rc::clone(p_trig)
        };
        if cand.p_tab_schema == p_tab.p_schema
            && !cand.z_table.is_empty()
            && str_icmp(&cand.z_table, &p_tab.z_name) == 0
            && (cand.p_tab_schema != tmp_schema || cand.b_returning != 0)
        {
            temps.push(cand);
        } else if cand.op == TK_RETURNING {
            debug_assert!(parse.toplevel().b_returning != 0);
            let Some(r) = parse.toplevel_mut().p_returning.as_deref_mut() else {
                continue;
            };
            r.ret_trig.z_table = p_tab.z_name.clone();
            r.ret_trig.p_tab_schema = p_tab.p_schema;
            temps.push(Rc::new(r.ret_trig.clone()));
        }
    }
    temps.reverse();
    temps.extend(p_tab.p_trigger.iter().cloned());
    temps
}

/// `sqlite3BeginTrigger`: chamada pelo analisador ao ver `CREATE TRIGGER` até o `BEGIN` antes das
/// ações do gatilho. Monta o `Trigger` com o que se sabe e o guarda em `Parse.p_new_trigger`;
/// `finish_trigger` o completa depois de analisadas as ações.
///
/// `p_name1` e `p_name2` são o nome do gatilho; `tr_tm` é `TK_BEFORE`, `TK_AFTER` ou
/// `TK_INSTEAD`; `op` é `TK_INSERT`, `TK_UPDATE` ou `TK_DELETE`; `p_columns` é a lista de
/// "UPDATE OF"; `p_table_name` é a tabela ou view do gatilho; `is_temp` é verdadeiro com TEMP; e
/// `no_err` suprime o erro de gatilho já existente.
pub fn begin_trigger(
    db: &mut Connection,
    parse: &mut Parse,
    p_name1: &Token,
    p_name2: &Token,
    tr_tm: i32,
    op: i32,
    p_columns: Option<Box<IdList>>,
    p_table_name: Option<Box<SrcList>>,
    p_when: Option<Box<Expr>>,
    is_temp: i32,
    no_err: i32,
) {
    let mut p_columns = p_columns;
    let mut p_table_name = p_table_name;
    let mut p_when = p_when;
    let mut tr_tm = tr_tm;
    let mut orphan = false; // o rótulo `trigger_orphan_error`

    debug_assert!(op == TK_INSERT as i32 || op == TK_UPDATE as i32 || op == TK_DELETE as i32);
    debug_assert!(op > 0 && op < 0xff);
    'cleanup: {
        let mut i_db: i32; // o banco em que o gatilho fica
        let p_name: &Token; // o nome sem o banco
        if is_temp != 0 {
            // Com TEMP, o nome do gatilho não pode ser qualificado.
            if !p_name2.z.is_empty() {
                error_msg(db, parse, b"temporary trigger may not have qualified name", &[]);
                break 'cleanup;
            }
            i_db = 1;
            p_name = p_name1;
        } else {
            // Descobre em que banco o gatilho vai ficar.
            match two_part_name(db, parse, p_name1, p_name2) {
                Some((d, t)) => {
                    i_db = d;
                    p_name = t;
                }
                None => break 'cleanup,
            }
        }
        let Some(table_name) = p_table_name.as_deref_mut() else {
            break 'cleanup;
        };
        if db.malloc_failed != 0 {
            break 'cleanup;
        }

        // Um bug antigo do analisador deixava passar
        //
        //    CREATE TRIGGER attached.demo AFTER INSERT ON attached.tab ....
        //                                                 ^^^^^^^^
        //
        // Para manter a compatibilidade, o nome do banco em `table_name` é ignorado ao reler do
        // esquema.
        if db.init.busy != 0 && i_db != 1 {
            table_name.a[0].z_database = None;
        }

        // Se o nome do gatilho não foi qualificado e a tabela é temporária, o gatilho vai para o
        // banco TEMP. Se `src_list_lookup` devolve `None`, a tabela não existe e o erro sai mais
        // abaixo.
        let p_tab = src_list_lookup(db, parse, table_name);
        if db.init.busy == 0
            && p_name2.z.is_empty()
            && p_tab.as_ref().map_or(false, |t| t.p_schema == db.dbs[1].schema.id)
        {
            i_db = 1;
        }

        // Garante que o nome da tabela casa com o do banco e que a tabela existe.
        if db.malloc_failed != 0 {
            break 'cleanup;
        }
        debug_assert!(table_name.a.len() == 1);
        let mut s_fix = fix_init(db, i_db, "trigger", p_name);
        if fix_src_list(db, parse, &mut s_fix, table_name) != 0 {
            break 'cleanup;
        }
        let p_tab = src_list_lookup(db, parse, table_name);
        let Some(p_tab) = p_tab else {
            // A tabela não existe.
            orphan = true;
            break 'cleanup;
        };
        if p_tab.is_virtual() {
            error_msg(db, parse, b"cannot create triggers on virtual tables", &[]);
            orphan = true;
            break 'cleanup;
        }
        if (p_tab.tab_flags & TF_SHADOW) != 0 && read_only_shadow_tables(db) {
            error_msg(db, parse, b"cannot create triggers on shadow tables", &[]);
            orphan = true;
            break 'cleanup;
        }

        // Confere que o nome do gatilho não é reservado e que não existe gatilho com ele.
        let Some(z_name) = name_from_token(Some(p_name)) else {
            break 'cleanup;
        };
        if check_object_name(db, parse, &z_name, b"trigger", &p_tab.z_name) != 0 {
            break 'cleanup;
        }
        if !parse.in_rename_object()
            && hash_find(&db.dbs[i_db as usize].schema.trig_hash, &z_name).is_some()
        {
            if no_err == 0 {
                let arg = token_arg(parse, p_name);
                error_msg(db, parse, b"trigger %T already exists", &[arg]);
            } else {
                debug_assert!(db.init.busy == 0);
                code_verify_schema(db, parse, i_db);
            }
            break 'cleanup;
        }

        // Não se cria gatilho em tabela do sistema.
        if strnicmp(Some(&p_tab.z_name), Some(b"sqlite_"), 7) == 0 {
            error_msg(db, parse, b"cannot create trigger on system table", &[]);
            break 'cleanup;
        }

        // Gatilhos INSTEAD OF só existem em views, e views só aceitam INSTEAD OF.
        if p_tab.is_view() && tr_tm != TK_INSTEAD as i32 {
            let z_tm: &[u8] = if tr_tm == TK_BEFORE as i32 { b"BEFORE" } else { b"AFTER" };
            error_msg(
                db,
                parse,
                b"cannot create %s trigger on view: %S",
                &[text_arg(z_tm), src_item_arg(&table_name.a[0])],
            );
            orphan = true;
            break 'cleanup;
        }
        if !p_tab.is_view() && tr_tm == TK_INSTEAD as i32 {
            error_msg(
                db,
                parse,
                b"cannot create INSTEAD OF trigger on table: %S",
                &[src_item_arg(&table_name.a[0])],
            );
            orphan = true;
            break 'cleanup;
        }

        if !parse.in_rename_object() {
            let i_tab_db = schema_to_index(db, p_tab.p_schema);
            let mut code = SQLITE_CREATE_TRIGGER;
            let z_db = db.dbs[i_tab_db as usize].z_db_s_name.clone();
            let z_db_trig =
                if is_temp != 0 { db.dbs[1].z_db_s_name.clone() } else { z_db.clone() };
            if i_tab_db == 1 || is_temp != 0 {
                code = SQLITE_CREATE_TEMP_TRIGGER;
            }
            if auth_check(db, parse, code, Some(&z_name), Some(&p_tab.z_name), Some(&z_db_trig))
                != 0
            {
                break 'cleanup;
            }
            if auth_check(
                db,
                parse,
                SQLITE_INSERT,
                Some(schema_table(i_tab_db)),
                None,
                Some(&z_db),
            ) != 0
            {
                break 'cleanup;
            }
        }

        // Um gatilho INSTEAD OF só aparece em views e um BEFORE nunca, então todo INSTEAD OF vira
        // BEFORE. Isso simplifica o resto do código.
        if tr_tm == TK_INSTEAD as i32 {
            tr_tm = TK_BEFORE as i32;
        }

        // Monta o objeto `Trigger`.
        let mut p_trigger = Box::new(Trigger::default());
        p_trigger.z_name = z_name;
        p_trigger.z_table = table_name.a[0].z_name.clone().unwrap_or_default();
        p_trigger.p_schema = db.dbs[i_db as usize].schema.id;
        p_trigger.p_tab_schema = p_tab.p_schema;
        p_trigger.op = op as u8;
        p_trigger.tr_tm = if tr_tm == TK_BEFORE as i32 { TRIGGER_BEFORE } else { TRIGGER_AFTER };
        if parse.in_rename_object() {
            let from = table_name.a[0].z_name.as_ref().map_or(0, |z| z.as_ptr() as usize);
            rename_token_remap(parse, p_trigger.z_table.as_ptr() as usize, from);
            p_trigger.p_when = p_when.take();
        } else {
            p_trigger.p_when = expr_dup(p_when.as_deref(), EXPRDUP_REDUCE);
        }
        p_trigger.p_columns = p_columns.take();
        debug_assert!(parse.p_new_trigger.is_none());
        parse.p_new_trigger = Some(p_trigger);
    }

    // `trigger_orphan_error`: o gatilho TEMP em uma tabela que outra conexão apagou.
    if orphan && db.init.i_db == 1 {
        // Ticket #3810. Normalmente, ao apagar uma tabela, todos os gatilhos dela somem. Mas se
        // um gatilho TEMP é criado numa tabela não TEMP e a tabela é apagada por outra conexão,
        // o gatilho não é visível a ela e continua lá: um "gatilho órfão", cuja tabela não
        // existe mais. Ver também https://sqlite.org/forum/forumpost/157dc791df (2020-11-05).
        db.init.orphan_trigger = true;
    }
    // `trigger_cleanup`: o que sobrou (nome, lista da tabela, colunas, WHEN e, se o gatilho não
    // chegou a ser guardado, ele mesmo) é largado pelas variáveis locais.
}

/// Desfaz a lista encadeada de passos (`TriggerStep.p_next`) do analisador no `Vec` de
/// `Trigger.step_list`, na mesma ordem.
fn flatten_steps(p_step_list: Option<Box<TriggerStep>>) -> Vec<TriggerStep> {
    let mut v: Vec<TriggerStep> = Vec::new();
    let mut cur = p_step_list;
    while let Some(mut step) = cur {
        cur = step.p_next.take();
        v.push(*step);
    }
    v
}

/// `sqlite3FinishTrigger`: chamada depois de analisadas todas as ações, para terminar a
/// construção do gatilho em `Parse.p_new_trigger`. `p_all` é o token do `CREATE TRIGGER` inteiro.
pub fn finish_trigger(
    db: &mut Connection,
    parse: &mut Parse,
    p_step_list: Option<Box<TriggerStep>>,
    p_all: &Token,
) {
    let mut p_step_list = p_step_list;
    let mut p_trig = parse.p_new_trigger.take(); // o gatilho que se termina
    'cleanup: {
        if parse.n_err != 0 || p_trig.is_none() {
            break 'cleanup;
        }
        let Some(trig) = p_trig.as_deref_mut() else {
            break 'cleanup;
        };
        let z_name = trig.z_name.clone();
        let i_db = schema_to_index(db, trig.p_schema);
        let name_token = Token { z: z_name.clone(), i_ofst: -1 };
        let mut s_fix = fix_init(db, i_db, "trigger", &name_token);
        if fix_trigger_step(db, parse, &mut s_fix, p_step_list.as_deref_mut()) != 0
            || fix_expr(db, parse, &mut s_fix, trig.p_when.as_deref_mut()) != 0
        {
            break 'cleanup;
        }
        trig.step_list = flatten_steps(p_step_list.take());

        if parse.in_rename_object() {
            debug_assert!(db.init.busy == 0);
            parse.p_new_trigger = p_trig.take();
            break 'cleanup;
        }

        // Se não está inicializando, monta a entrada de sqlite_schema.
        if db.init.busy == 0 {
            // Se é um CREATE TABLE novo, as tabelas sombra são só de leitura e o gatilho altera
            // uma tabela sombra, levanta um erro e não deixa criar o gatilho.
            if read_only_shadow_tables(db) {
                for p_step in trig.step_list.iter() {
                    if let Some(z_target) = p_step.z_target.as_deref() {
                        if shadow_table_name(db, z_target) {
                            error_msg(
                                db,
                                parse,
                                b"trigger \"%s\" may not write to shadow table \"%s\"",
                                &[text_arg(&trig.z_name), text_arg(z_target)],
                            );
                            break 'cleanup;
                        }
                    }
                }
            }

            // Faz uma entrada na tabela sqlite_schema.
            get_vdbe(db, parse);
            if parse.p_vdbe.is_none() {
                break 'cleanup;
            }
            begin_write_operation(db, parse, 0, i_db);
            let fmt: Vec<u8> = [
                b"INSERT INTO %Q.".as_slice(),
                LEGACY_SCHEMA_TABLE,
                b" VALUES('trigger',%Q,%Q,0,'CREATE TRIGGER %q')".as_slice(),
            ]
            .concat();
            let z_db_name = db.dbs[i_db as usize].z_db_s_name.clone();
            nested_parse(
                db,
                parse,
                &fmt,
                &[
                    text_arg(&z_db_name),
                    text_arg(&z_name),
                    text_arg(&trig.z_table),
                    text_arg(&p_all.z),
                ],
            );
            change_cookie(db, parse, i_db);
            let z_where = mprintf(b"type='trigger' AND name='%q'", &[text_arg(&z_name)]);
            add_parse_schema_op(parse, db, i_db, z_where, 0);
        }

        if db.init.busy != 0 {
            let Some(boxed) = p_trig.take() else {
                break 'cleanup;
            };
            let p_link: Rc<Trigger> = Rc::new(*boxed);
            let old = hash_insert(
                &mut db.dbs[i_db as usize].schema.trig_hash,
                &z_name,
                Some(Rc::clone(&p_link)),
            );
            if old.is_some() {
                oom_fault(db);
            } else if p_link.p_schema == p_link.p_tab_schema {
                // O esquema da tabela é o do gatilho (`i_db`): o gatilho vai para o começo da
                // lista da tabela.
                match hash_find_mut(&mut db.dbs[i_db as usize].schema.tbl_hash, &p_link.z_table) {
                    Some(p_tab) => Rc::make_mut(p_tab).p_trigger.insert(0, Rc::clone(&p_link)),
                    None => debug_assert!(false, "tabela do gatilho ausente do esquema"),
                }
            }
        }
    }
    // `triggerfinish_cleanup`: o gatilho que não foi guardado e os passos que sobraram caem aqui.
    debug_assert!(parse.in_rename_object() || parse.p_new_trigger.is_none());
}

// ---------------------------------------------------------------------------------------------
// chunk 001: passos do gatilho, remoção, TriggersExist
// ---------------------------------------------------------------------------------------------

/// `triggerSpanDup`: copia um trecho do SQL (sem os espaços das pontas, como `sqlite3DbSpanDup`)
/// e troca todo caractere de espaço em branco por um espaço comum.
fn trigger_span_dup(parse: &Parse, z_start: i32, z_end: i32) -> Option<Vec<u8>> {
    let z = sql_span(parse, z_start, z_end);
    let mut a = 0usize;
    while a < z.len() && is_space(z[a]) {
        a += 1;
    }
    let mut b = z.len();
    while b > a && is_space(z[b - 1]) {
        b -= 1;
    }
    let mut out = z[a..b].to_vec();
    for c in out.iter_mut() {
        if is_space(*c) {
            *c = b' ';
        }
    }
    Some(out)
}

/// `sqlite3TriggerSelectStep`: transforma um SELECT em um passo de gatilho. O analisador a chama
/// ao achar um SELECT no corpo de um TRIGGER.
pub fn trigger_select_step(
    _db: &mut Connection,
    parse: &mut Parse,
    p_select: Option<Box<Select>>,
    z_start: i32,
    z_end: i32,
) -> Option<Box<TriggerStep>> {
    let mut p_trigger_step = Box::new(TriggerStep::default());
    p_trigger_step.op = TK_SELECT;
    p_trigger_step.p_select = p_select;
    p_trigger_step.orconf = OE_DEFAULT;
    p_trigger_step.z_span = trigger_span_dup(parse, z_start, z_end);
    Some(p_trigger_step)
}

/// `triggerStepAllocate`: aloca um passo novo. O alvo (`z_target`) é o nome do token sem aspas.
/// Devolve `None` se o analisador já tem erro.
fn trigger_step_allocate(
    parse: &mut Parse,
    op: u8,
    p_name: &Token,
    z_start: i32,
    z_end: i32,
) -> Option<Box<TriggerStep>> {
    if parse.n_err != 0 {
        return None;
    }
    let mut p_trigger_step = Box::new(TriggerStep::default());
    let mut z = p_name.z.clone();
    dequote(&mut z);
    let n = strlen30(&z) as usize;
    z.truncate(n);
    p_trigger_step.op = op;
    p_trigger_step.z_span = trigger_span_dup(parse, z_start, z_end);
    p_trigger_step.z_target = Some(z);
    if parse.in_rename_object() {
        let addr = p_trigger_step.z_target.as_ref().map_or(0, |z| z.as_ptr() as usize);
        rename_token_map(parse, addr, p_name);
    }
    Some(p_trigger_step)
}

/// `sqlite3TriggerInsertStep`: monta um passo de gatilho a partir de um INSERT. O analisador a
/// chama ao ver um INSERT no corpo de um gatilho.
pub fn trigger_insert_step(
    db: &mut Connection,
    parse: &mut Parse,
    p_table_name: &Token,
    p_column: Option<Box<IdList>>,
    p_select: Option<Box<Select>>,
    orconf: i32,
    p_upsert: Option<Box<Upsert>>,
    z_start: i32,
    z_end: i32,
) -> Option<Box<TriggerStep>> {
    let mut p_select = p_select;
    let mut p_trigger_step = trigger_step_allocate(parse, TK_INSERT, p_table_name, z_start, z_end);
    if let Some(step) = p_trigger_step.as_deref_mut() {
        if parse.in_rename_object() {
            step.p_select = p_select.take();
        } else {
            step.p_select = select_dup(p_select.as_deref(), EXPRDUP_REDUCE);
        }
        step.p_id_list = p_column;
        step.orconf = orconf as u8;
        if let Some(up) = p_upsert.as_deref() {
            has_explicit_nulls(db, parse, up.p_upsert_target.as_deref());
        }
        step.p_upsert = p_upsert;
    }
    p_trigger_step
}

/// `sqlite3TriggerUpdateStep`: constrói um passo de gatilho que implementa um UPDATE. O
/// analisador a chama ao ver um UPDATE no corpo de um CREATE TRIGGER.
pub fn trigger_update_step(
    _db: &mut Connection,
    parse: &mut Parse,
    p_table_name: &Token,
    p_from: Option<Box<SrcList>>,
    p_e_list: Option<Box<ExprList>>,
    p_where: Option<Box<Expr>>,
    orconf: i32,
    z_start: i32,
    z_end: i32,
) -> Option<Box<TriggerStep>> {
    let mut p_from = p_from;
    let mut p_e_list = p_e_list;
    let mut p_where = p_where;
    let mut p_trigger_step = trigger_step_allocate(parse, TK_UPDATE, p_table_name, z_start, z_end);
    if let Some(step) = p_trigger_step.as_deref_mut() {
        if parse.in_rename_object() {
            step.p_expr_list = p_e_list.take();
            step.p_where = p_where.take();
            step.p_from = p_from.take();
        } else {
            step.p_expr_list = expr_list_dup(p_e_list.as_deref(), EXPRDUP_REDUCE);
            step.p_where = expr_dup(p_where.as_deref(), EXPRDUP_REDUCE);
            step.p_from = src_list_dup(p_from.as_deref(), EXPRDUP_REDUCE);
        }
        step.orconf = orconf as u8;
    }
    p_trigger_step
}

/// `sqlite3TriggerDeleteStep`: constrói um passo de gatilho que implementa um DELETE. O
/// analisador a chama ao ver um DELETE no corpo de um CREATE TRIGGER.
pub fn trigger_delete_step(
    _db: &mut Connection,
    parse: &mut Parse,
    p_table_name: &Token,
    p_where: Option<Box<Expr>>,
    z_start: i32,
    z_end: i32,
) -> Option<Box<TriggerStep>> {
    let mut p_where = p_where;
    let mut p_trigger_step = trigger_step_allocate(parse, TK_DELETE, p_table_name, z_start, z_end);
    if let Some(step) = p_trigger_step.as_deref_mut() {
        if parse.in_rename_object() {
            step.p_where = p_where.take();
        } else {
            step.p_where = expr_dup(p_where.as_deref(), EXPRDUP_REDUCE);
        }
        step.orconf = OE_DEFAULT;
    }
    p_trigger_step
}

/// `tableOfTrigger`: a tabela em que o gatilho está definido.
fn table_of_trigger(db: &Connection, p_trigger: &Trigger) -> Option<Rc<Table>> {
    let i_db = schema_to_index(db, p_trigger.p_tab_schema);
    if i_db < 0 {
        return None;
    }
    hash_find(&db.dbs[i_db as usize].schema.tbl_hash, &p_trigger.z_table).cloned()
}

/// `sqlite3DropTrigger`: tira um gatilho do esquema do banco. O analisador a chama, então o
/// gatilho é identificado pelo nome; `drop_trigger_ptr` faz o mesmo trabalho a partir do gatilho.
/// A lista `p_name` (um só item) é consumida.
pub fn drop_trigger(
    db: &mut Connection,
    parse: &mut Parse,
    p_name: Option<Box<SrcList>>,
    no_err: i32,
) {
    'cleanup: {
        let Some(p_name) = p_name else {
            break 'cleanup;
        };
        if db.malloc_failed != 0 {
            break 'cleanup;
        }
        if SQLITE_OK != read_schema(db, parse) {
            break 'cleanup;
        }
        debug_assert!(p_name.a.len() == 1);
        let z_db = p_name.a[0].z_database.clone();
        let z_name = p_name.a[0].z_name.clone().unwrap_or_default();
        let mut p_trigger: Option<Rc<Trigger>> = None;
        let mut i = OMIT_TEMPDB as usize;
        while i < db.dbs.len() {
            let j = if i < 2 { i ^ 1 } else { i }; // procura em TEMP antes de MAIN
            if let Some(zd) = z_db.as_deref() {
                if !db_is_named(db, j, zd) {
                    i += 1;
                    continue;
                }
            }
            p_trigger = hash_find(&db.dbs[j].schema.trig_hash, &z_name).cloned();
            if p_trigger.is_some() {
                break;
            }
            i += 1;
        }
        let Some(p_trigger) = p_trigger else {
            if no_err == 0 {
                error_msg(db, parse, b"no such trigger: %S", &[src_item_arg(&p_name.a[0])]);
            } else {
                code_verify_named_schema(db, parse, z_db.as_deref());
            }
            parse.check_schema = 1;
            break 'cleanup;
        };
        drop_trigger_ptr(db, parse, &p_trigger);
    }
}

/// `sqlite3DropTriggerPtr`: tira um gatilho do esquema, dado o ponteiro do gatilho.
pub fn drop_trigger_ptr(db: &mut Connection, parse: &mut Parse, p_trigger: &Trigger) {
    let i_db = schema_to_index(db, p_trigger.p_schema);
    debug_assert!(i_db >= 0 && (i_db as usize) < db.dbs.len());
    let p_table = table_of_trigger(db, p_trigger);
    debug_assert!(
        p_table.as_ref().map_or(false, |t| t.p_schema == p_trigger.p_schema) || i_db == 1
    );
    let z_db = db.dbs[i_db as usize].z_db_s_name.clone();
    if let Some(p_table) = p_table.as_ref() {
        let mut code = SQLITE_DROP_TRIGGER;
        let z_tab = schema_table(i_db);
        if i_db == 1 {
            code = SQLITE_DROP_TEMP_TRIGGER;
        }
        if auth_check(db, parse, code, Some(&p_trigger.z_name), Some(&p_table.z_name), Some(&z_db))
            != 0
            || auth_check(db, parse, SQLITE_DELETE, Some(z_tab), None, Some(&z_db)) != 0
        {
            return;
        }
    }

    // Gera o código que apaga o registro do gatilho no banco.
    get_vdbe(db, parse);
    let fmt: Vec<u8> = [
        b"DELETE FROM %Q.".as_slice(),
        LEGACY_SCHEMA_TABLE,
        b" WHERE name=%Q AND type='trigger'".as_slice(),
    ]
    .concat();
    nested_parse(db, parse, &fmt, &[text_arg(&z_db), text_arg(&p_trigger.z_name)]);
    change_cookie(db, parse, i_db);
    add_op4(
        vdbe_of_parse(parse),
        OP_DROPTRIGGER as i32,
        i_db,
        0,
        0,
        P4::Text(p_trigger.z_name.clone()),
    );
}

/// `sqlite3UnlinkAndDeleteTrigger`: tira um gatilho das tabelas hash da conexão.
pub fn unlink_and_delete_trigger(db: &mut Connection, i_db: usize, z_name: &[u8]) {
    let p_trigger = hash_insert(&mut db.dbs[i_db].schema.trig_hash, z_name, None);
    if let Some(p_trigger) = p_trigger {
        if p_trigger.p_schema == p_trigger.p_tab_schema {
            if let Some(p_tab) =
                hash_find_mut(&mut db.dbs[i_db].schema.tbl_hash, &p_trigger.z_table)
            {
                if p_tab.p_trigger.iter().any(|t| same_trigger(t, &p_trigger)) {
                    Rc::make_mut(p_tab).p_trigger.retain(|t| !same_trigger(t, &p_trigger));
                }
            }
        }
        db.m_db_flags |= crate::consts::DBFLAG_SCHEMA_CHANGE;
    }
}

/// A identidade de dois gatilhos (o `p==pTrigger` do C). Gatilhos do esquema são o mesmo `Rc`;
/// as ações de chave estrangeira (sem nome) são refeitas a cada `fk_actions` e se identificam
/// pela chave em `Trigger.z_table` (ver `fkey.rs`), que faz o papel do cache `FKey.apTrigger`.
pub(crate) fn same_trigger(a: &Rc<Trigger>, b: &Rc<Trigger>) -> bool {
    Rc::ptr_eq(a, b)
        || (a.z_name.is_empty()
            && b.z_name.is_empty()
            && !a.z_table.is_empty()
            && a.z_table == b.z_table)
}

/// `checkColumnOverlap`: `p_e_list` é o SET de um UPDATE, cada entrada na forma `<id>=<expr>`.
/// Verdadeiro se alguma entrada tem um `<id>` que casa com um identificador de `p_id_list`.
/// `p_id_list` nulo é curinga e casa com tudo; o mesmo para `p_e_list` nula. Falso só se não há
/// coincidência.
fn check_column_overlap(p_id_list: Option<&IdList>, p_e_list: Option<&ExprList>) -> bool {
    let (Some(id_list), Some(e_list)) = (p_id_list, p_e_list) else {
        return true;
    };
    for item in e_list.a.iter() {
        if id_list_index(id_list, item.z_e_name.as_deref().unwrap_or(&[])) >= 0 {
            return true;
        }
    }
    false
}

/// `tempTriggersExist`: verdadeiro se existe algum gatilho TEMP.
fn temp_triggers_exist(db: &Connection) -> bool {
    hash_first(&db.dbs[1].schema.trig_hash).is_some()
}

/// `triggersReallyExist`: devolve a lista de todos os gatilhos da tabela `p_tab` se existe ao
/// menos um que deva disparar numa operação do tipo `op` e, sendo UPDATE, se ao menos uma das
/// colunas de `p_changes` é modificada.
fn triggers_really_exist(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Table,
    op: i32,
    p_changes: Option<&ExprList>,
    p_mask: &mut i32,
) -> Vec<Rc<Trigger>> {
    let mut mask: i32 = 0;
    let mut p_list = trigger_list(db, parse, p_tab);
    if !p_list.is_empty() {
        let n_tab = p_tab.p_trigger.len();
        if (db.flags & SQLITE_ENABLE_TRIGGER) == 0 && n_tab != 0 {
            // `SQLITE_DBCONFIG_ENABLE_TRIGGER` está desligada: só os gatilhos TEMP valem. Trunca
            // a lista para que tenha só eles (ficam à frente de `p_tab.p_trigger`).
            if p_list.len() == n_tab {
                p_list.clear();
                *p_mask = 0;
                return Vec::new();
            }
            let n_temp = p_list.len() - n_tab;
            p_list.truncate(n_temp);
        }
        for idx in 0..p_list.len() {
            let p = Rc::clone(&p_list[idx]);
            if p.op as i32 == op && check_column_overlap(p.p_columns.as_deref(), p_changes) {
                mask |= p.tr_tm as i32;
            } else if p.op == TK_RETURNING {
                // Na primeira vez que um gatilho RETURNING aparece, o `op` diz de que tipo ele
                // deve ser.
                debug_assert!(parse.p_toplevel.is_none());
                let mut t: Trigger = (*p).clone();
                t.op = op as u8;
                if p_tab.is_virtual() {
                    if op != TK_INSERT as i32 {
                        let z_kind: &[u8] =
                            if op == TK_DELETE as i32 { b"DELETE" } else { b"UPDATE" };
                        error_msg(
                            db,
                            parse,
                            b"%s RETURNING is not available on virtual tables",
                            &[text_arg(z_kind)],
                        );
                    }
                    t.tr_tm = TRIGGER_BEFORE;
                } else {
                    t.tr_tm = TRIGGER_AFTER;
                }
                mask |= t.tr_tm as i32;
                if let Some(r) = parse.p_returning.as_deref_mut() {
                    r.ret_trig.op = t.op;
                    r.ret_trig.tr_tm = t.tr_tm;
                }
                p_list[idx] = Rc::new(t);
            } else if p.b_returning != 0
                && p.op == TK_INSERT
                && op == TK_UPDATE as i32
                && parse.p_toplevel.is_none()
            {
                // Dispara também um gatilho RETURNING para um UPSERT.
                mask |= p.tr_tm as i32;
            }
        }
    }
    *p_mask = mask;
    if mask != 0 {
        p_list
    } else {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 002: TriggersExist, TriggerStepSrc, RETURNING, codeTriggerProgram
// ---------------------------------------------------------------------------------------------

/// `sqlite3TriggersExist`: devolve a lista de todos os gatilhos da tabela `p_tab` se ao menos um
/// deve disparar numa operação do tipo `op` (e, num UPDATE, se ao menos uma das colunas de
/// `p_changes` é modificada). `p_mask` recebe a máscara de `TRIGGER_BEFORE|TRIGGER_AFTER`. A
/// lista vazia é o ponteiro nulo do C.
pub fn triggers_exist(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Table,
    op: i32,
    p_changes: Option<&ExprList>,
    p_mask: &mut i32,
) -> Vec<Rc<Trigger>> {
    if (p_tab.p_trigger.is_empty() && !temp_triggers_exist(db)) || parse.disable_triggers != 0 {
        *p_mask = 0;
        return Vec::new();
    }
    triggers_really_exist(db, parse, p_tab, op, p_changes, p_mask)
}

/// `sqlite3TriggerStepSrc`: converte `p_step.z_target` numa `SrcList`. Acrescenta o nome do banco
/// ao alvo quando preciso, para que um gatilho de um banco não se refira a um alvo de outro
/// (exceto o gatilho TEMP, que pode se referir a qualquer um). `p_trig_schema` é o esquema do
/// gatilho dono do passo (o `pStep->pTrig->pSchema` do C).
pub fn trigger_step_src(
    db: &mut Connection,
    parse: &mut Parse,
    p_step: &TriggerStep,
    p_trig_schema: crate::sqlite_int::SchemaId,
) -> Option<Box<SrcList>> {
    let z_name = p_step.z_target.clone();
    let mut p_src = src_list_append(db, parse, None, None, None);
    if let Some(src) = p_src.as_deref_mut() {
        debug_assert!(src.a.len() == 1);
        src.a[0].z_name = z_name;
        if p_trig_schema != db.dbs[1].schema.id {
            src.a[0].p_schema = Some(p_trig_schema);
        }
    }
    if p_src.is_some() {
        if let Some(p_from) = p_step.p_from.as_deref() {
            let mut p_dup = src_list_dup(Some(p_from), 0);
            if p_dup.as_ref().map_or(false, |d| d.a.len() > 1) && !parse.in_rename_object() {
                let p_subquery = select_new(parse, None, p_dup.take(), None, None, None, None, SF_NESTEDFROM, None);
                let as_tok = Token::default();
                p_dup = src_list_append_from_term(
                    db,
                    parse,
                    None,
                    None,
                    None,
                    Some(&as_tok),
                    p_subquery,
                    None,
                );
            }
            p_src = src_list_append_list(db, parse, p_src, p_dup);
        }
    }
    p_src
}

/// `isAsteriskTerm`: verdadeiro se o termo da lista do RETURNING tem a forma "*". Levanta um erro
/// se o termo tem a forma "table.*".
fn is_asterisk_term(db: &mut Connection, parse: &mut Parse, p_term: &Expr) -> bool {
    if p_term.op == TK_ASTERISK {
        return true;
    }
    if p_term.op != TK_DOT {
        return false;
    }
    debug_assert!(p_term.p_right.is_some());
    debug_assert!(p_term.p_left.is_some());
    if p_term.p_right.as_deref().map_or(true, |r| r.op != TK_ASTERISK) {
        return false;
    }
    error_msg(db, parse, b"RETURNING may not use \"TABLE.*\" wildcards", &[]);
    true
}

/// `sqlite3ExpandReturning`: copia a lista `p_list` de termos do RETURNING e expande os "*" para
/// o conjunto completo de colunas de `p_tab`.
fn expand_returning(
    db: &mut Connection,
    parse: &mut Parse,
    p_list: &ExprList,
    p_tab: &Table,
) -> Option<Box<ExprList>> {
    let mut p_new: Option<Box<ExprList>> = None;
    for item in p_list.a.iter() {
        let Some(p_old_expr) = item.p_expr.as_deref() else {
            continue;
        };
        if is_asterisk_term(db, parse, p_old_expr) {
            for p_col in p_tab.a_col.iter() {
                if p_col.is_hidden() {
                    continue;
                }
                let p_new_expr = expr(TK_ID as i32, Some(col_name(p_col)));
                p_new = expr_list_append(p_new, p_new_expr);
                if let Some(last) = p_new.as_deref_mut().and_then(|l| l.a.last_mut()) {
                    last.z_e_name = Some(col_name(p_col).to_vec());
                    last.fg.e_e_name = ENAME_NAME;
                }
            }
        } else {
            let p_new_expr = expr_dup(Some(p_old_expr), 0);
            p_new = expr_list_append(p_new, p_new_expr);
            if item.z_e_name.is_some() {
                if let Some(last) = p_new.as_deref_mut().and_then(|l| l.a.last_mut()) {
                    last.z_e_name = item.z_e_name.clone();
                    last.fg.e_e_name = item.fg.e_e_name;
                }
            }
        }
    }
    p_new
}

/// `sqlite3ReturningSubqueryVarSelect`: se o nó é uma subconsulta, um EXISTS ou um IN com
/// subconsulta, e ela é `SF_Correlated`, marca a expressão como `EP_VarSelect`.
fn returning_subquery_var_select(_w: &mut Walker<Rc<Table>>, p_expr: &mut Expr) -> i32 {
    let correlated = match &p_expr.x {
        ExprX::Select(s) if p_expr.use_x_select() => (s.sel_flags & SF_CORRELATED) != 0,
        _ => false,
    };
    if correlated {
        p_expr.set_property(EP_VAR_SELECT);
    }
    WRC_CONTINUE
}

/// `sqlite3ReturningSubqueryCorrelated`: se o SELECT se refere à tabela `w.u`, (1) marca-o como
/// `SF_Correlated` e (2) põe `e_code` diferente de zero para o chamador saber que (1) ocorreu.
fn returning_subquery_correlated(w: &mut Walker<Rc<Table>>, p_select: &mut Select) -> i32 {
    let Some(p_src) = p_select.p_src.as_deref() else {
        debug_assert!(false);
        return WRC_CONTINUE;
    };
    let hit = p_src
        .a
        .iter()
        .any(|it| it.p_tab.as_ref().map_or(false, |t| Rc::ptr_eq(t, &w.u)));
    if hit {
        p_select.sel_flags |= SF_CORRELATED;
        w.e_code = 1;
    }
    WRC_CONTINUE
}

/// `sqlite3ProcessReturningSubqueries`: percorre a lista de argumentos do RETURNING atrás de
/// subconsultas que dependem da tabela modificada pelo comando (`p_tab`) e marca todas como
/// `SF_Correlated`. Se a subconsulta é parte de uma expressão, marca a expressão como
/// `EP_VarSelect`. https://sqlite.org/forum/forumpost/2c83569ce8945d39
fn process_returning_subqueries(p_e_list: &mut ExprList, p_tab: &Rc<Table>) {
    let mut w: Walker<Rc<Table>> = Walker {
        x_expr_callback: Some(expr_walk_noop::<Rc<Table>>),
        x_select_callback: Some(returning_subquery_correlated),
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: Rc::clone(p_tab),
    };
    walk_expr_list(&mut w, Some(&mut *p_e_list));
    if w.e_code != 0 {
        w.x_expr_callback = Some(returning_subquery_var_select);
        w.x_select_callback = Some(select_walk_noop::<Rc<Table>>);
        walk_expr_list(&mut w, Some(p_e_list));
    }
}

/// `codeReturningTrigger`: gera o código do gatilho de RETURNING. Ao contrário dos outros
/// gatilhos, que chamam um subprograma no bytecode, o código do RETURNING é gerado em linha.
fn code_returning_trigger(
    db: &mut Connection,
    parse: &mut Parse,
    p_trigger: &Trigger,
    p_tab: &Rc<Table>,
    reg_in: i32,
) {
    debug_assert!(parse.p_vdbe.is_some());
    if parse.b_returning == 0 {
        // Este gatilho RETURNING é de outro comando, pois este não tem RETURNING.
        return;
    }
    let (p_return_el, z_ret_name) = match parse.p_returning.as_deref() {
        Some(r) => (r.p_return_el.clone(), r.z_name.clone()),
        None => return,
    };
    if p_trigger.z_name != z_ret_name {
        // Este gatilho RETURNING é de outro comando.
        return;
    }
    let mut s_select = Select::default();
    s_select.p_e_list = expr_list_dup(p_return_el.as_deref(), 0);
    let mut s_from = SrcList::default();
    let mut item = SrcItem::default();
    item.p_tab = Some(Rc::clone(p_tab));
    item.z_name = Some(p_tab.z_name.clone()); // tag-20240424-1
    item.i_cursor = -1;
    s_from.a.push(item);
    s_select.p_src = Some(Box::new(s_from));
    select_prep(db, parse, &mut s_select, None);
    if parse.n_err == 0 {
        debug_assert!(db.malloc_failed == 0);
        generate_column_names(db, parse, &s_select);
    }
    drop(s_select);
    let mut p_new = match p_return_el.as_deref() {
        Some(l) => expand_returning(db, parse, l, p_tab),
        None => None,
    };
    if parse.n_err == 0 {
        let mut s_nc = name_context_new();
        let n_ret_col_now = parse.p_returning.as_deref().map_or(0, |r| r.n_ret_col);
        if n_ret_col_now == 0 {
            let n = p_new.as_deref().map_or(0, |l| l.a.len() as i32);
            let i_ret_cur = parse.n_tab;
            parse.n_tab += 1;
            if let Some(r) = parse.p_returning.as_deref_mut() {
                r.n_ret_col = n;
                r.i_ret_cur = i_ret_cur;
            }
        }
        s_nc.u_nc = NcU::BaseReg(reg_in);
        s_nc.nc_flags = NC_UBASEREG;
        parse.e_trigger_op = p_trigger.op;
        parse.p_trigger_tab = Some(Rc::clone(p_tab));
        if resolve_expr_list_names(db, parse, &mut s_nc, p_new.as_deref_mut()) == SQLITE_OK
            && db.malloc_failed == 0
        {
            let n_col = p_new.as_deref().map_or(0, |l| l.a.len() as i32);
            let reg = parse.n_mem + 1;
            if let Some(l) = p_new.as_deref_mut() {
                process_returning_subqueries(l, p_tab);
            }
            parse.n_mem += n_col + 2;
            let (i_ret_cur, _) = match parse.p_returning.as_deref_mut() {
                Some(r) => {
                    r.i_ret_reg = reg;
                    (r.i_ret_cur, 0)
                }
                None => (0, 0),
            };
            let mut i = 0;
            while i < n_col {
                let mut p_col = p_new
                    .as_deref_mut()
                    .and_then(|l| l.a[i as usize].p_expr.take())
                    .expect("pCol");
                expr_code_factorable(db, parse, &mut p_col, reg + i, None);
                if expr_affinity(&p_col, None) == SQLITE_AFF_REAL {
                    add_op1(vdbe_of_parse(parse), OP_REALAFFINITY as i32, reg + i);
                }
                if let Some(l) = p_new.as_deref_mut() {
                    l.a[i as usize].p_expr = Some(p_col);
                }
                i += 1;
            }
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_MAKERECORD as i32, reg, i, reg + i);
            add_op3(v, OP_NEWROWID as i32, i_ret_cur, reg + i + 1, 0);
            add_op3(v, OP_INSERT as i32, i_ret_cur, reg + i, reg + i + 1);
        }
    }
    drop(p_new);
    parse.e_trigger_op = 0;
    parse.p_trigger_tab = None;
}

/// `codeTriggerProgram`: gera o código VDBE dos comandos do corpo de um único gatilho.
fn code_trigger_program(
    db: &mut Connection,
    parse: &mut Parse,
    p_trigger: &Trigger,
    orconf: i32,
) -> i32 {
    debug_assert!(parse.p_trigger_tab.is_some() && parse.p_toplevel.is_some());
    debug_assert!(!p_trigger.step_list.is_empty());
    debug_assert!(parse.p_vdbe.is_some());
    for p_step in p_trigger.step_list.iter() {
        // Descobre a política ON CONFLICT que este passo vai usar. Se o comando que disparou o
        // gatilho tinha um ON CONFLICT explícito, vale ele; senão, a política do próprio passo.
        // Exemplo:
        //
        //   CREATE TRIGGER AFTER INSERT ON t1 BEGIN;
        //     INSERT OR REPLACE INTO t2 VALUES(new.a, new.b);
        //   END;
        //
        //   INSERT INTO t1 ... ;            -- o insert em t2 usa a política REPLACE
        //   INSERT OR IGNORE INTO t1 ... ;  -- o insert em t2 usa a política IGNORE
        parse.e_orconf = if orconf == OE_DEFAULT as i32 { p_step.orconf } else { orconf as u8 };
        debug_assert!(parse.ok_const_factor == 0);

        if let Some(z_span) = p_step.z_span.as_deref() {
            let z: Vec<u8> = [b"-- ".as_slice(), z_span].concat();
            add_op4(
                vdbe_of_parse(parse),
                OP_TRACE as i32,
                0x7fffffff,
                1,
                0,
                P4::Text(z),
            );
        }

        match p_step.op {
            TK_UPDATE => {
                let p_src = trigger_step_src(db, parse, p_step, p_trigger.p_schema);
                let on_error = parse.e_orconf as i32;
                update(
                    db,
                    parse,
                    p_src,
                    expr_list_dup(p_step.p_expr_list.as_deref(), 0),
                    expr_dup(p_step.p_where.as_deref(), 0),
                    on_error,
                    None,
                    None,
                    None,
                );
                add_op0(vdbe_of_parse(parse), OP_RESETCOUNT as i32);
            }
            TK_INSERT => {
                let p_src = trigger_step_src(db, parse, p_step, p_trigger.p_schema);
                let on_error = parse.e_orconf as i32;
                insert(
                    db,
                    parse,
                    p_src,
                    select_dup(p_step.p_select.as_deref(), 0),
                    id_list_dup(p_step.p_id_list.as_deref()),
                    on_error,
                    upsert_dup(p_step.p_upsert.as_deref()),
                );
                add_op0(vdbe_of_parse(parse), OP_RESETCOUNT as i32);
            }
            TK_DELETE => {
                let p_src = trigger_step_src(db, parse, p_step, p_trigger.p_schema);
                delete_from(db, parse, p_src, expr_dup(p_step.p_where.as_deref(), 0), None, None);
                add_op0(vdbe_of_parse(parse), OP_RESETCOUNT as i32);
            }
            _ => {
                debug_assert!(p_step.op == TK_SELECT);
                let mut s_dest = SelectDest::default();
                let mut p_select = select_dup(p_step.p_select.as_deref(), 0);
                select_dest_init(&mut s_dest, SRT_DISCARD as i32, 0);
                if let Some(s) = p_select.as_deref_mut() {
                    select(db, parse, s, &mut s_dest);
                }
            }
        }
    }
    0
}

// ---------------------------------------------------------------------------------------------
// chunk 003: subprogramas dos gatilhos, CodeRowTrigger, TriggerColmask
// ---------------------------------------------------------------------------------------------

/// `transferParseError`: o `Parse` do subprograma acabou de ser usado. Se houve erro, passa a
/// informação do erro dele (`z_err_msg`, `n_err`, `rc`) para `p_to`.
fn transfer_parse_error(p_to: &mut Parse, err: (Option<Vec<u8>>, i32, i32)) {
    let (z_err_msg, n_err, rc) = err;
    if p_to.n_err == 0 {
        p_to.z_err_msg = z_err_msg;
        p_to.n_err = n_err;
        p_to.rc = rc;
    }
}

/// `codeRowTrigger`: cria e preenche um `TriggerPrg` com o subprograma que implementa o gatilho
/// `p_trigger` com a política ON CONFLICT `orconf`. (O nome do C colide com o de
/// `sqlite3CodeRowTrigger`, por isso o sufixo.)
///
/// O `Parse` de nível mais alto é MOVIDO para dentro do `Parse` do subprograma
/// (`Parse.p_toplevel`) enquanto ele é gerado e devolvido ao lugar de origem no fim.
///
/// O subprograma é registrado no `Vdbe` de nível mais alto como um marcador (`a_op` vazio,
/// `n_csr == -1` e `n_mem` o índice em `Vdbe.p_program`) antes de ser gerado, como o C o liga à
/// lista antes de gerar, para que uma chamada recursiva ao mesmo gatilho (gatilho que dispara a
/// si mesmo) tenha o que referenciar. No fim o marcador dá lugar ao subprograma pronto.
fn code_row_trigger_program(
    db: &mut Connection,
    parse: &mut Parse,
    p_trigger: &Rc<Trigger>,
    p_tab: &Rc<Table>,
    orconf: i32,
) -> PrgInfo {
    debug_assert!(p_trigger.z_name.is_empty() || {
        table_of_trigger(db, p_trigger).map_or(false, |t| Rc::ptr_eq(&t, p_tab))
    });
    let n_query_loop = parse.n_query_loop;
    let prep_flags = parse.prep_flags;

    // Tira o `Parse` de nível mais alto do lugar onde ele mora.
    let parse_is_top = parse.p_toplevel.is_none();
    let mut top: Box<Parse> = if parse_is_top {
        Box::new(std::mem::take(parse))
    } else {
        parse.p_toplevel.take().expect("pToplevel")
    };
    debug_assert!(top.p_vdbe.is_some());

    // Aloca o `TriggerPrg` e liga o marcador do subprograma ao `Vdbe` de nível mais alto.
    let prg_idx = top.p_trigger_prg.len();
    let prog_idx = top.p_vdbe.as_deref().map_or(0, |v| v.p_program.len());
    let token = trigger_token(p_trigger);
    let placeholder = Rc::new(SubProgram {
        n_mem: prog_idx as i32,
        n_csr: -1,
        token,
        ..SubProgram::default()
    });
    if let Some(v) = top.p_vdbe.as_deref_mut() {
        link_sub_program(v, Rc::clone(&placeholder));
    }
    top.p_trigger_prg.push(TriggerPrg {
        p_trigger: Some(Rc::clone(p_trigger)),
        p_program: Some(Rc::clone(&placeholder)),
        orconf,
        a_colmask: [0xffff_ffff, 0xffff_ffff],
    });

    // Aloca e preenche um `Parse` novo para gerar o subprograma.
    let mut s_sub_parse = parse_object_init(db);
    s_sub_parse.p_trigger_tab = Some(Rc::clone(p_tab));
    s_sub_parse.p_toplevel = Some(top);
    s_sub_parse.z_auth_context =
        if p_trigger.z_name.is_empty() { None } else { Some(p_trigger.z_name.clone()) };
    s_sub_parse.e_trigger_op = p_trigger.op;
    s_sub_parse.n_query_loop = n_query_loop;
    s_sub_parse.prep_flags = prep_flags;

    get_vdbe(db, &mut s_sub_parse);
    if !p_trigger.z_name.is_empty() {
        let z = [b"-- TRIGGER ".as_slice(), &p_trigger.z_name].concat();
        change_p4(vdbe_of_parse(&mut s_sub_parse), -1, P4::Text(z));
    }

    // Se há WHEN, gera o código dele. Se ele dá falso (ou NULL), o subprograma termina na hora
    // pulando para o `OP_Halt` do fim.
    let mut i_end_trigger: i32 = 0;
    if let Some(when) = p_trigger.p_when.as_deref() {
        let mut p_when = expr_dup(Some(when), 0);
        let mut s_nc = name_context_new();
        if db.malloc_failed == 0
            && SQLITE_OK == resolve_expr_names(db, &mut s_sub_parse, &mut s_nc, p_when.as_deref_mut())
        {
            i_end_trigger = make_label(&mut s_sub_parse);
            if let Some(w) = p_when.as_deref_mut() {
                expr_if_false(db, &mut s_sub_parse, w, i_end_trigger, SQLITE_JUMPIFNULL as i32, None);
            }
        }
    }

    // Gera o programa do gatilho no sub-VDBE.
    code_trigger_program(db, &mut s_sub_parse, p_trigger, orconf);

    // Põe um `OP_Halt` no fim do subprograma.
    if i_end_trigger != 0 {
        resolve_label(&mut s_sub_parse, db, i_end_trigger);
    }
    add_op0(vdbe_of_parse(&mut s_sub_parse), OP_HALT as i32);

    // Devolve o `Parse` de nível mais alto ao lugar de origem e passa o erro, se houve.
    let top_back: Box<Parse> = s_sub_parse.p_toplevel.take().expect("pToplevel");
    if parse_is_top {
        *parse = *top_back;
    } else {
        parse.p_toplevel = Some(top_back);
    }
    let err = (s_sub_parse.z_err_msg.take(), s_sub_parse.n_err, s_sub_parse.rc);
    transfer_parse_error(parse, err);

    let mut p_program = SubProgram::default();
    if parse.n_err == 0 {
        debug_assert!(db.malloc_failed == 0);
        let mut n_max_arg = parse.toplevel().n_max_arg;
        p_program.a_op = take_op_array(&mut s_sub_parse, &mut n_max_arg);
        parse.toplevel_mut().n_max_arg = n_max_arg;
    }
    p_program.n_mem = s_sub_parse.n_mem;
    p_program.n_csr = s_sub_parse.n_tab;
    p_program.token = token;
    let a_colmask = [s_sub_parse.oldmask, s_sub_parse.newmask];
    if let Some(v) = s_sub_parse.p_vdbe.take() {
        vdbe_delete(*v, db);
    }
    parse_object_reset(db, &mut s_sub_parse);

    // O subprograma pronto toma o lugar do marcador.
    let p_program = Rc::new(p_program);
    let top = parse.toplevel_mut();
    if let Some(v) = top.p_vdbe.as_deref_mut() {
        if prog_idx < v.p_program.len() {
            v.p_program[prog_idx] = Rc::clone(&p_program);
        }
    }
    top.p_trigger_prg[prg_idx].p_program = Some(Rc::clone(&p_program));
    top.p_trigger_prg[prg_idx].a_colmask = a_colmask;
    PrgInfo { p_program: Some(p_program), a_colmask }
}

/// `getRowTrigger`: devolve o `TriggerPrg` com o subprograma do gatilho `p_trigger` com o
/// algoritmo ON CONFLICT padrão `orconf`. Se não existe, aloca e preenche um antes.
fn get_row_trigger(
    db: &mut Connection,
    parse: &mut Parse,
    p_trigger: &Rc<Trigger>,
    p_tab: &Rc<Table>,
    orconf: i32,
) -> PrgInfo {
    // Pode ser que o gatilho já tenha sido gerado (ou esteja sendo). Nesse caso há uma entrada
    // com o mesmo gatilho na lista de `Parse.p_trigger_prg` do nível mais alto.
    let found = parse.toplevel().p_trigger_prg.iter().rev().find(|p| {
        p.orconf == orconf && p.p_trigger.as_ref().map_or(false, |t| same_trigger(t, p_trigger))
    });
    if let Some(p) = found {
        return PrgInfo { p_program: p.p_program.clone(), a_colmask: p.a_colmask };
    }
    // Não achou: cria.
    let prg = code_row_trigger_program(db, parse, p_trigger, p_tab, orconf);
    db.err_byte_offset = -1;
    prg
}

/// `sqlite3CodeRowTriggerDirect`: gera o código do programa do gatilho `p` na tabela `p_tab`.
/// Os parâmetros `reg`, `orconf` e `ignore_jump` são os descritos em `code_row_trigger`.
pub fn code_row_trigger_direct(
    db: &mut Connection,
    parse: &mut Parse,
    p: &Rc<Trigger>,
    p_tab: &Rc<Table>,
    reg: i32,
    orconf: i32,
    ignore_jump: i32,
) {
    get_vdbe(db, parse);
    let p_prg = get_row_trigger(db, parse, p, p_tab, orconf);
    debug_assert!(p_prg.p_program.is_some() || parse.n_err != 0);

    // Gera o `OP_Program` no VDBE pai; o P4 é o sub-VDBE com o programa do gatilho.
    if let Some(prog) = p_prg.p_program {
        let b_recursive = !p.z_name.is_empty() && (db.flags & SQLITE_REC_TRIGGERS) == 0;
        parse.n_mem += 1;
        let n_mem = parse.n_mem;
        let v = vdbe_of_parse(parse);
        add_op4(v, OP_PROGRAM as i32, reg, ignore_jump, n_mem, P4::Subprogram(prog));

        // P5 diferente de zero quando a invocação recursiva do programa é proibida: ocorre se
        // (a) o subprograma é de fato um gatilho, não uma ação de chave estrangeira, e (b) a
        // opção de gatilhos recursivos está desligada.
        change_p5(v, b_recursive as u16);
    }
}

/// `sqlite3CodeRowTrigger`: gera o código dos gatilhos FOR EACH ROW necessários numa operação
/// sobre a tabela `p_tab`. A operação (INSERT, UPDATE ou DELETE) é `op`. `tr_tm` decide se os
/// gatilhos BEFORE ou AFTER são gerados. Num UPDATE, `p_changes` é a lista de colunas
/// modificadas. Se nenhum gatilho dispara no momento e na operação pedidos, não faz nada.
///
/// `reg` é o primeiro de um array de registradores com os valores que substituem as referências
/// `new.*` e `old.*` do programa do gatilho. Sendo N o número de colunas de `p_tab`:
///
///   Registrador    Contém
///   ------------------------------------------------------
///   reg+0          OLD.rowid
///   reg+1          valor OLD.* da coluna mais à esquerda de `p_tab`
///   ...            ...
///   reg+N          valor OLD.* da coluna mais à direita de `p_tab`
///   reg+N+1        NEW.rowid
///   reg+N+2        valor NEW.* da coluna mais à esquerda de `p_tab`
///   ...            ...
///   reg+N+N+1      valor NEW.* da coluna mais à direita de `p_tab`
///
/// Num DELETE os registradores NEW.* nunca são lidos, então o chamador nem os aloca; num INSERT
/// os OLD.* idem, e `reg` não é um registrador legível, mas de `reg+N` a `reg+N+N+1` são.
///
/// `orconf` é o algoritmo ON CONFLICT padrão do programa do gatilho (REPLACE, IGNORE etc.) e
/// `ignore_jump` é a instrução para onde o controle vai se o programa levanta IGNORE.
pub fn code_row_trigger(
    db: &mut Connection,
    parse: &mut Parse,
    p_trigger: &[Rc<Trigger>],
    op: i32,
    p_changes: Option<&ExprList>,
    tr_tm: i32,
    p_tab: &Rc<Table>,
    reg: i32,
    orconf: i32,
    ignore_jump: i32,
) {
    debug_assert!(op == TK_UPDATE as i32 || op == TK_INSERT as i32 || op == TK_DELETE as i32);
    debug_assert!(tr_tm == TRIGGER_BEFORE as i32 || tr_tm == TRIGGER_AFTER as i32);
    debug_assert!((op == TK_UPDATE as i32) == p_changes.is_some());

    for p in p_trigger.iter() {
        // O esquema do gatilho e o da tabela sempre existem: o gatilho está no mesmo esquema da
        // tabela ou é um gatilho TEMP.
        debug_assert!(
            p.p_schema == p.p_tab_schema || p.p_schema == db.dbs[1].schema.id
        );

        // Decide se este gatilho deve ser gerado: (1) ele casa exatamente com o comando DML, ou
        // (2) é um RETURNING de INSERT e se está gerando a parte UPDATE de um UPSERT.
        if (p.op as i32 == op || (p.b_returning != 0 && p.op == TK_INSERT && op == TK_UPDATE as i32))
            && p.tr_tm as i32 == tr_tm
            && check_column_overlap(p.p_columns.as_deref(), p_changes)
        {
            if p.b_returning == 0 {
                code_row_trigger_direct(db, parse, p, p_tab, reg, orconf, ignore_jump);
            } else if parse.p_toplevel.is_none() {
                code_returning_trigger(db, parse, p, p_tab, reg);
            }
        }
    }
}

/// `sqlite3TriggerColmask`: os gatilhos podem acessar valores das pseudotabelas old.* e new.*.
/// Devolve uma máscara de 32 bits com as colunas de old.* ou new.* que os gatilhos de fato usam.
/// O chamador pode usá-la, por exemplo, para não carregar o registro old.* inteiro num UPDATE ou
/// DELETE.
///
/// O bit 0 está ligado se a coluna mais à esquerda pode ser acessada por `[old|new].<col>`, o
/// bit 1 para a segunda, e assim por diante. Se a tabela tem mais de 32 colunas e alguma de
/// índice maior pode ser acessada, devolve 0xffffffff.
///
/// Não é possível saber se old.rowid ou new.rowid são acessados: o chamador sempre supõe que sim.
///
/// `is_new` é 1 para a máscara de new.* e 0 para a de old.*. `tr_tm` é uma máscara com
/// `TRIGGER_BEFORE` e/ou `TRIGGER_AFTER`: os valores acessados pelos gatilhos BEFORE só entram
/// na máscara se o bit `TRIGGER_BEFORE` está em `tr_tm`, e igual para AFTER.
pub fn trigger_colmask(
    db: &mut Connection,
    parse: &mut Parse,
    p_trigger: &[Rc<Trigger>],
    p_changes: Option<&ExprList>,
    is_new: i32,
    tr_tm: i32,
    p_tab: &Rc<Table>,
    orconf: i32,
) -> u32 {
    let op = if p_changes.is_some() { TK_UPDATE } else { TK_DELETE };
    let mut mask: u32 = 0;

    debug_assert!(is_new == 1 || is_new == 0);
    if p_tab.is_view() {
        return 0xffff_ffff;
    }
    for p in p_trigger.iter() {
        if p.op == op
            && (tr_tm & p.tr_tm as i32) != 0
            && check_column_overlap(p.p_columns.as_deref(), p_changes)
        {
            if p.b_returning != 0 {
                mask = 0xffff_ffff;
            } else {
                let p_prg = get_row_trigger(db, parse, p, p_tab, orconf);
                if p_prg.p_program.is_some() {
                    mask |= p_prg.a_colmask[is_new as usize];
                }
            }
        }
    }
    mask
}
