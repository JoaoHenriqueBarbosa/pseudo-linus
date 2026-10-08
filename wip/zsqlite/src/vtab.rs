//! `vtab.c`: chunks `vtab_c.000` a `vtab_c.003` do SQLite 3.46.1. O código que dá suporte às
//! tabelas virtuais: o registro de módulos (`sqlite3_create_module`), o ciclo de vida do `VTable`
//! (`xCreate`, `xConnect`, `xDisconnect`, `xDestroy`), a pilha de transações (`aVTrans`), os
//! savepoints, a sobrecarga de funções, as tabelas eponímias e a API que os módulos chamam de
//! dentro do construtor (`sqlite3_declare_vtab`, `sqlite3_vtab_config`).
//!
//! DECISÕES DO MODELO V2
//!
//! * O `VTable` mora em `Connection.vtabs` e o esquema é por conexão, então cada `Table` tem no
//!   máximo um `VTable` vivo: a lista `u.vtab.p` do C some. A ligação tabela para `VTable` é a
//!   chave `(schema, z_tab_name, b_eponymous)` guardada no próprio `VTable` (campos acrescentados
//!   a `connection::VTable`; `z_tab_name` vazio quer dizer "ainda não ligado a uma tabela", que é
//!   o estado de um `VTable` durante o construtor e depois de `vtabDisconnectAll`).
//!   [`get_vtable`] procura por essa chave.
//! * `sqlite3VtabInSync` (`nVTrans>0 && aVTrans==0`) não tem o ponteiro nulo para se apoiar: durante
//!   `xSync` e durante os finalizadores, `Connection.a_v_trans` guarda um vetor de um só elemento,
//!   `SYNC_MARK`, e a lista verdadeira fica numa variável local (o mesmo truque do C, que a
//!   zera durante o laço).
//! * A tabela em construção do `xCreate`/`xConnect` é uma cópia mutável (`VtabCtx.p_tab`); o
//!   `sqlite3_declare_vtab` a altera e, no fim do construtor, a cópia vira um `Rc<Table>` novo
//!   que substitui a tabela no esquema (ou em `a_epo_tab`) e no `&mut Rc<Table>` do chamador, como
//!   o C altera a `Table` no lugar.
//! * `sqlite3VtabModuleUnref` não existe: `Module` é `Rc` e o `Drop` roda o `xDestroy`.
//! * `sqlite3_create_module` e `sqlite3_create_module_v2` são a mesma [`create_module`] (o
//!   primeiro só passa `xDestroy` nulo).
//! * `sqlite3VtabUsesAllSchemas` (where.c) vive em `crate::where2`; `sqlite3ReadOnlyShadowTables`
//!   e `sqlite3ShadowTableName` (build.c) vivem em `crate::build2`.

use std::any::Any;
use std::rc::Rc;

use crate::auth::auth_check;
use crate::build::{
    delete_table, find_table, name_from_token, nested_parse, primary_key_index, start_table,
    text_arg, token_arg,
};
use crate::build2::{change_cookie, mark_all_shadow_tables_of, publish_view};
use crate::build3::may_abort;
use crate::connection::{
    Connection, DestroyFn, FuncDef, Module, Parse, UserData, VTable, VTableId, Vtab, VtabCtx,
    VtabModule, PARSE_MODE_DECLARE_VTAB, PARSE_MODE_NORMAL,
};
use crate::consts::{
    COLFLAG_HASTYPE, COLFLAG_HIDDEN, LEGACY_SCHEMA_TABLE, OP_EXPIRE, OP_VCREATE, SAVEPOINT_BEGIN,
    SAVEPOINT_ROLLBACK, SQLITE_ABORT, SQLITE_CREATE_VTABLE, SQLITE_DEFENSIVE, SQLITE_ERROR,
    SQLITE_FAIL, SQLITE_FUNC_EPHEM, SQLITE_IGNORE, SQLITE_LIMIT_COLUMN, SQLITE_LOCKED,
    SQLITE_NOMEM, SQLITE_OK, SQLITE_REPLACE, SQLITE_ROLLBACK, SQLITE_VTABRISK_HIGH,
    SQLITE_VTABRISK_LOW, SQLITE_VTABRISK_NORMAL, SQLITE_VTAB_CONSTRAINT_SUPPORT,
    SQLITE_VTAB_DIRECTONLY, SQLITE_VTAB_INNOCUOUS, SQLITE_VTAB_USES_ALL_SCHEMAS, TABTYP_VTAB,
    TF_EPHEMERAL, TF_EPONYMOUS, TF_HAS_HIDDEN, TF_NO_VISIBLE_ROWID, TF_OOO_HIDDEN,
    TF_WITHOUT_ROWID, TK_COLUMN, TK_CREATE, TK_SPACE, TK_TABLE,
};
use crate::hash::{hash_find, hash_find_mut, hash_insert, hash_iter};
use crate::main::{api_exit, db_printf, error, error_with_msg, misuse_error};
use crate::parse_reduce::sql_span;
use crate::prepare::{parse_object_init, parse_object_reset, run_parser, schema_to_index};
use crate::printf::PrintfArg;
use crate::select::get_vdbe;
use crate::sqlite_int::{Expr, SchemaId, TabRef, Table, TableU, Token, VTabInfo};
use crate::tokenize::get_token;
use crate::util::{at, error_msg, oom_fault, strlen30, strnicmp};
use crate::vdbe_types::Vdbe;
use crate::vdbeaux::{add_op0, add_op2, add_parse_schema_op, load_string};
use crate::vdbeaux2::vdbe_finalize;
use crate::vdbeaux3::vtab_import_errmsg;

/// O único elemento de `Connection.a_v_trans` enquanto o C teria `aVTrans==0` com `nVTrans>0`
/// (dentro de `xSync` e dos finalizadores): é o que [`vtab_in_sync`] reconhece.
const SYNC_MARK: VTableId = VTableId(u32::MAX);

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------

/// Retira a instância `Vtab` do `VTable` `vid` de seu dono, roda `f` com a conexão e a devolve.
/// `None` se o `VTable` ou a instância não existem (o `pVtab==0` do C).
pub(crate) fn with_vtab<R>(
    db: &mut Connection,
    vid: VTableId,
    f: impl FnOnce(&mut Connection, &mut dyn Vtab) -> R,
) -> Option<R> {
    let mut vt = db.vtabs.take(vid.slot())?;
    let r = match vt.p_vtab.take() {
        Some(mut vtab) => {
            let r = f(db, &mut *vtab);
            vt.p_vtab = Some(vtab);
            Some(r)
        }
        None => None,
    };
    db.vtabs.put(vid.slot(), vt);
    r
}

/// Desliga o `VTable` da tabela a que pertencia (o `pTab->u.vtab.p = 0` do C): daqui em diante
/// [`get_vtable`] não o acha mais.
fn vtab_unlink(db: &mut Connection, vid: VTableId) {
    if let Some(vt) = db.vtabs.get_mut(vid.slot()) {
        vt.z_tab_name.clear();
    }
}

/// Grava no esquema (ou em `a_epo_tab`, para a eponímia) a `Table` nova que o construtor
/// produziu, no lugar da que estava lá.
fn publish_table(db: &mut Connection, tab: &Rc<Table>) {
    if (tab.tab_flags & TF_EPONYMOUS) != 0 {
        if let Some(slot) = hash_find_mut(&mut db.a_epo_tab, &tab.z_name) {
            *slot = Rc::clone(tab);
        }
        return;
    }
    let i_db = schema_to_index(db, tab.p_schema);
    publish_view(db, i_db, tab);
}

/// Se o tipo declarado da coluna contém a palavra `hidden` (ignorando maiúsculas, cercada por
/// espaços ou pelas pontas), tira a palavra do texto do tipo, no lugar, e devolve verdadeiro.
/// `z_cn_name` é o `nome\0tipo\0...` da coluna; o deslocamento é o do C (o resto da alocação fica
/// como estava).
fn strip_hidden_token(z_cn_name: &mut [u8]) -> bool {
    let Some(nul) = z_cn_name.iter().position(|&c| c == 0) else {
        return false;
    };
    let t = &mut z_cn_name[nul + 1..];
    let n_type = strlen30(t) as usize;
    let mut i = 0usize;
    while i < n_type {
        if strnicmp(Some(b"hidden".as_slice()), Some(&t[i..]), 6) == 0
            && (i == 0 || t[i - 1] == b' ')
            && (at(t, i + 6) == 0 || at(t, i + 6) == b' ')
        {
            break;
        }
        i += 1;
    }
    if i >= n_type {
        return false;
    }
    let n_del = 6 + usize::from(at(t, i + 6) != 0);
    let mut j = i;
    while j + n_del <= n_type {
        t[j] = at(t, j + n_del);
        j += 1;
    }
    if t[i] == 0 && i > 0 {
        debug_assert!(t[i - 1] == b' ');
        t[i - 1] = 0;
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Módulos
// ---------------------------------------------------------------------------------------------

/// `sqlite3VtabCreateModule`: constrói e instala o `Module` de uma tabela virtual. Se já existe um
/// módulo com `z_name`, ele é substituído; com `p_module` nulo, o módulo `z_name` é apagado.
pub fn vtab_create_module(
    db: &mut Connection,
    z_name: &[u8],
    p_module: Option<Rc<dyn VtabModule>>,
    p_aux: Option<Rc<dyn Any>>,
    x_destroy: Option<DestroyFn>,
) -> Option<Rc<Module>> {
    let p_mod = p_module.map(|m| {
        Rc::new(Module { p_module: m, z_name: z_name.to_vec(), p_aux, x_destroy })
    });
    let p_del = hash_insert(&mut db.a_module, z_name, p_mod.clone());
    if let Some(del) = p_del {
        vtab_eponymous_table_clear(db, &del.z_name);
        // O `sqlite3VtabModuleUnref`: o `Drop` do `Module` roda o `xDestroy` quando some o último
        // `Rc` (os `VTable` que o usam guardam o seu).
        drop(del);
    }
    p_mod
}

/// `createModule`: faz o trabalho de `sqlite3_create_module()` e `sqlite3_create_module_v2()`.
pub fn create_module(
    db: &mut Connection,
    z_name: &[u8],
    p_module: Option<Rc<dyn VtabModule>>,
    p_aux: Option<Rc<dyn Any>>,
    x_destroy: Option<DestroyFn>,
) -> i32 {
    let created = vtab_create_module(db, z_name, p_module, p_aux, x_destroy);
    let rc = api_exit(db, SQLITE_OK);
    if rc != SQLITE_OK {
        // No C o módulo não chega a ser registrado e `xDestroy(pAux)` roda uma vez; aqui o
        // módulo já está registrado e o `xDestroy` está dentro dele, então desfazer o registro
        // faz o `Drop` rodá-lo uma vez.
        if let Some(m) = created {
            let same = hash_find(&db.a_module, z_name).is_some_and(|cur| Rc::ptr_eq(cur, &m));
            if same {
                hash_insert(&mut db.a_module, z_name, None);
            }
        }
    }
    rc
}

/// `sqlite3_drop_modules`: apaga todos os módulos, menos os de `az_names`.
pub fn drop_modules(db: &mut Connection, az_names: Option<&[&[u8]]>) -> i32 {
    let names: Vec<Vec<u8>> = hash_iter(&db.a_module).map(|(_, m)| m.z_name.clone()).collect();
    for name in names {
        if let Some(az) = az_names {
            if az.iter().any(|n| *n == name.as_slice()) {
                continue;
            }
        }
        create_module(db, &name, None, None, None);
    }
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// Travas e desconexão do VTable
// ---------------------------------------------------------------------------------------------

/// `sqlite3VtabLock`: trava a tabela virtual para que não seja desconectada. As travas aninham.
pub fn vtab_lock(db: &mut Connection, vid: VTableId) {
    if let Some(vt) = db.vtabs.get_mut(vid.slot()) {
        vt.n_ref += 1;
    }
}

/// `sqlite3GetVTable`: o `VTable` que a conexão `db` usa para a tabela virtual `p_tab`, se já foi
/// criado.
pub fn get_vtable(db: &Connection, p_tab: &Table) -> Option<VTableId> {
    debug_assert!(p_tab.is_virtual());
    let epo = (p_tab.tab_flags & TF_EPONYMOUS) != 0;
    db.vtabs
        .iter()
        .find(|(_, vt)| {
            vt.schema == p_tab.p_schema && vt.b_eponymous == epo && vt.z_tab_name == p_tab.z_name
        })
        .map(|(id, _)| VTableId::from_slot(id))
}

/// `sqlite3VtabUnlock`: tira uma trava do `VTable`; quando zera, chama o `xDisconnect` e o libera.
pub fn vtab_unlock(db: &mut Connection, vid: VTableId) {
    let Some(vt) = db.vtabs.get_mut(vid.slot()) else {
        return;
    };
    debug_assert!(vt.n_ref > 0);
    vt.n_ref -= 1;
    if vt.n_ref != 0 {
        return;
    }
    if let Some(mut vt) = db.vtabs.remove(vid.slot()) {
        if let Some(mut p) = vt.p_vtab.take() {
            p.disconnect(db);
        }
        // `vt` cai aqui e com ele o `Rc<Module>`: o `sqlite3VtabModuleUnref`.
    }
}

/// `sqlite3VtabDisconnect`: tira o `VTable` de `db` da tabela virtual `p` e larga a trava dele.
/// Serve para fechar a conexão sem mexer no resto do esquema.
pub fn vtab_disconnect(db: &mut Connection, p: &Table) {
    debug_assert!(p.is_virtual());
    if let Some(vid) = get_vtable(db, p) {
        vtab_unlink(db, vid);
        vtab_unlock(db, vid);
    }
}

/// `sqlite3VtabUnlockList`: desconecta todos os `VTable` da lista `p_disconnect`.
pub fn vtab_unlock_list(db: &mut Connection) {
    let list = std::mem::take(&mut db.p_disconnect);
    // O C empilha na frente da lista, então a lista anda do mais novo para o mais velho.
    for vid in list.into_iter().rev() {
        vtab_unlock(db, vid);
    }
}

/// `sqlite3VtabClear`: apaga toda a informação de tabela virtual do registro da tabela, logo
/// antes de a `Table` ser apagada. O `VTable` da conexão vai para `p_disconnect` (o
/// `vtabDisconnectAll(0, p)` do C; no modelo v2 não há `VTable` de outras conexões); o vetor
/// `az_arg` cai com a `Table`.
pub fn vtab_clear(db: &mut Connection, p: &Table) {
    debug_assert!(p.is_virtual());
    if let Some(vid) = get_vtable(db, p) {
        vtab_unlink(db, vid);
        db.p_disconnect.push(vid);
    }
}

// ---------------------------------------------------------------------------------------------
// CREATE VIRTUAL TABLE no analisador
// ---------------------------------------------------------------------------------------------

/// `addModuleArgument`: acrescenta um argumento de módulo a `u.vtab.az_arg` da tabela. O "ponteiro
/// nulo" do C (o `azArg[1]`, que `vtabCallConstructor` preenche) é o vetor vazio.
fn add_module_argument(
    db: &mut Connection,
    parse: &mut Parse,
    p_table: &mut Table,
    z_arg: Vec<u8>,
) {
    debug_assert!(p_table.is_virtual());
    let limit = db.a_limit[SQLITE_LIMIT_COLUMN as usize];
    let n_arg = match &p_table.u {
        TableU::VTab(info) => info.n_arg,
        _ => return,
    };
    if n_arg + 3 >= limit {
        error_msg(db, parse, b"too many columns on %s", &[text_arg(&p_table.z_name)]);
    }
    if let TableU::VTab(info) = &mut p_table.u {
        info.az_arg.push(z_arg);
        info.n_arg += 1;
    }
}

/// `sqlite3VtabBeginParse`: o analisador a chama quando vê o começo de um CREATE VIRTUAL TABLE. O
/// nome do módulo já foi lido; a lista de parâmetros ainda não.
pub fn vtab_begin_parse(
    db: &mut Connection,
    parse: &mut Parse,
    p_name1: &Token,
    p_name2: &Token,
    p_module_name: &Token,
    if_not_exists: i32,
) {
    start_table(db, parse, p_name1, p_name2, 0, 0, 1, if_not_exists);
    let Some(mut p_table) = parse.p_new_table.take() else {
        return;
    };
    debug_assert!(p_table.p_index.is_empty());
    p_table.e_tab_type = TABTYP_VTAB;
    p_table.u = TableU::VTab(VTabInfo::default());

    let z_module = name_from_token(Some(p_module_name)).unwrap_or_default();
    let z_tab = p_table.z_name.clone();
    add_module_argument(db, parse, &mut p_table, z_module.clone());
    add_module_argument(db, parse, &mut p_table, Vec::new());
    add_module_argument(db, parse, &mut p_table, z_tab.clone());
    let start = parse.s_name_token.i_ofst;
    let end = p_module_name.i_ofst + p_module_name.z.len() as i32;
    parse.s_name_token = Token { z: sql_span(parse, start, end), i_ofst: start };

    // Criar uma tabela virtual chama o gancho de autorização duas vezes. A primeira, para
    // permitir o INSERT em sqlite_schema, já foi feita por `start_table`. A segunda, para
    // permitir criar a tabela, é esta.
    let i_db = schema_to_index(db, p_table.p_schema);
    if let Some(z_db) = db.dbs.get(i_db as usize).map(|d| d.z_db_s_name.clone()) {
        auth_check(db, parse, SQLITE_CREATE_VTABLE, Some(&z_tab), Some(&z_module), Some(&z_db));
    }
    parse.p_new_table = Some(p_table);
}

/// `addArgumentToVtab`: pega o argumento de módulo que vem se acumulando em `parse.s_arg` e o
/// acrescenta à lista de argumentos da tabela virtual em construção.
fn add_argument_to_vtab(db: &mut Connection, parse: &mut Parse) {
    if parse.s_arg.z.is_empty() {
        return;
    }
    let Some(mut p_tab) = parse.p_new_table.take() else {
        return;
    };
    let z = parse.s_arg.z.clone();
    add_module_argument(db, parse, &mut p_tab, z);
    parse.p_new_table = Some(p_tab);
}

/// `sqlite3VtabFinishParse`: o analisador a chama depois de ler o CREATE VIRTUAL TABLE inteiro.
pub fn vtab_finish_parse(db: &mut Connection, parse: &mut Parse, p_end: Option<&Token>) {
    if parse.p_new_table.is_none() {
        return;
    }
    debug_assert!(parse.p_new_table.as_ref().is_some_and(|t| t.is_virtual()));
    add_argument_to_vtab(db, parse);
    parse.s_arg.z.clear();
    let Some(p_tab) = parse.p_new_table.take() else {
        return;
    };
    if p_tab.u_vtab().map_or(0, |v| v.n_arg) < 1 {
        parse.p_new_table = Some(p_tab);
        return;
    }

    // Se o CREATE VIRTUAL TABLE está sendo digitado pela primeira vez (e não só lido de
    // sqlite_schema), faltam o trabalho de inicialização e guardar o texto do comando em
    // sqlite_schema.
    if db.init.busy == 0 {
        may_abort(parse);

        // O texto completo do comando CREATE VIRTUAL TABLE.
        if let Some(end) = p_end {
            let start = parse.s_name_token.i_ofst;
            let stop = end.i_ofst + end.z.len() as i32;
            parse.s_name_token = Token { z: sql_span(parse, start, stop), i_ofst: start };
        }
        let name_token = parse.s_name_token.clone();
        let z_stmt = db_printf(db, b"CREATE VIRTUAL TABLE %T", &[token_arg(parse, &name_token)]);

        // Já há uma vaga para o registro em sqlite_schema. Falta atualizá-la com tudo o que foi
        // coletado. O registrador `reg_rowid` guarda o rowid da linha que `start_table` criou.
        let i_db = schema_to_index(db, p_tab.p_schema);
        let z_db = db.dbs.get(i_db as usize).map(|d| d.z_db_s_name.clone()).unwrap_or_default();
        let reg_rowid = parse.reg_rowid;
        let fmt: Vec<u8> = [
            b"UPDATE %Q.".as_slice(),
            LEGACY_SCHEMA_TABLE,
            b" SET type='table', name=%Q, tbl_name=%Q, rootpage=0, sql=%Q WHERE rowid=#%d",
        ]
        .concat();
        nested_parse(
            db,
            parse,
            &fmt,
            &[
                PrintfArg::Text(Some(z_db)),
                PrintfArg::Text(Some(p_tab.z_name.clone())),
                PrintfArg::Text(Some(p_tab.z_name.clone())),
                PrintfArg::Text(z_stmt.clone()),
                PrintfArg::Int(reg_rowid as i64),
            ],
        );
        get_vdbe(db, parse);
        change_cookie(db, parse, i_db);

        add_op0(get_vdbe(db, parse), OP_EXPIRE);
        let z_where = db_printf(
            db,
            b"name=%Q AND sql=%Q",
            &[PrintfArg::Text(Some(p_tab.z_name.clone())), PrintfArg::Text(z_stmt)],
        );
        add_parse_schema_op(parse, db, i_db, z_where, 0);

        parse.n_mem += 1;
        let i_reg = parse.n_mem;
        let v = get_vdbe(db, parse);
        load_string(v, i_reg, &p_tab.z_name);
        add_op2(v, OP_VCREATE, i_db, i_reg);
        parse.p_new_table = Some(p_tab);
    } else {
        // Relendo sqlite_schema: cria o registro da tabela em memória.
        mark_all_shadow_tables_of(db, &p_tab);
        let i_db = schema_to_index(db, p_tab.p_schema);
        let z_name = p_tab.z_name.clone();
        let Some(slot) = db.dbs.get_mut(i_db as usize) else {
            return;
        };
        let p_old = hash_insert(&mut slot.schema.tbl_hash, &z_name, Some(Rc::new(*p_tab)));
        if p_old.is_some() {
            oom_fault(db);
        }
    }
}

/// `sqlite3VtabArgInit`: o analisador a chama quando vê o primeiro token de um argumento do nome
/// do módulo num CREATE VIRTUAL TABLE.
pub fn vtab_arg_init(db: &mut Connection, parse: &mut Parse) {
    add_argument_to_vtab(db, parse);
    parse.s_arg = Token::default();
}

/// `sqlite3VtabArgExtend`: o analisador a chama para cada token depois do primeiro de um
/// argumento do nome do módulo num CREATE VIRTUAL TABLE.
pub fn vtab_arg_extend(parse: &mut Parse, p: &Token) {
    if parse.s_arg.z.is_empty() {
        parse.s_arg = p.clone();
    } else {
        debug_assert!(parse.s_arg.i_ofst <= p.i_ofst);
        let start = parse.s_arg.i_ofst;
        let end = p.i_ofst + p.z.len() as i32;
        parse.s_arg.z = sql_span(parse, start, end);
    }
}

// ---------------------------------------------------------------------------------------------
// Construtores
// ---------------------------------------------------------------------------------------------

/// `vtabCallConstructor`: chama o construtor da tabela virtual, `xCreate` (`create`) ou
/// `xConnect`. A `Table` do esquema é substituída pela que o construtor deixou (colunas
/// declaradas, flags de colunas escondidas), em `p_tab` e no esquema.
fn vtab_call_constructor(
    db: &mut Connection,
    p_tab: &mut Rc<Table>,
    p_mod: &Rc<Module>,
    create: bool,
    pz_err: &mut Option<Vec<u8>>,
) -> i32 {
    debug_assert!(p_tab.is_virtual());

    // Confere que a tabela virtual não está sendo inicializada neste momento.
    let epo = (p_tab.tab_flags & TF_EPONYMOUS) != 0;
    let recursive = db.p_vtab_ctx.iter().any(|c| {
        c.p_tab.as_deref().is_some_and(|t| {
            t.p_schema == p_tab.p_schema
                && t.z_name == p_tab.z_name
                && ((t.tab_flags & TF_EPONYMOUS) != 0) == epo
        })
    });
    if recursive {
        *pz_err = db_printf(db, b"vtable constructor called recursively: %s", &[text_arg(&p_tab.z_name)]);
        return SQLITE_LOCKED;
    }

    let z_module_name = p_tab.z_name.clone();
    let vid = VTableId::from_slot(db.vtabs.insert(VTable {
        p_mod: Rc::clone(p_mod),
        p_vtab: None,
        n_ref: 0,
        n_cursor: 0,
        b_constraint: 0,
        b_all_schemas: 0,
        e_vtab_risk: SQLITE_VTABRISK_NORMAL,
        i_savepoint: 0,
        schema: SchemaId(0),
        z_tab_name: Vec::new(),
        b_eponymous: false,
    }));

    let i_db = schema_to_index(db, p_tab.p_schema);
    let z_db_name = db.dbs.get(i_db as usize).map(|d| d.z_db_s_name.clone()).unwrap_or_default();
    let mut work: Box<Table> = Box::new((**p_tab).clone());
    let argv: Vec<Vec<u8>> = match &mut work.u {
        TableU::VTab(info) => {
            if info.az_arg.len() > 1 {
                info.az_arg[1] = z_db_name;
            }
            info.az_arg.clone()
        }
        _ => Vec::new(),
    };

    // Chama o construtor da tabela virtual.
    db.p_vtab_ctx.push(VtabCtx { p_v_table: vid, p_tab: Some(work), b_declared: false });
    let mut z_err: Option<Vec<u8>> = None;
    let module = Rc::clone(&p_mod.p_module);
    let res = if create {
        module.x_create(db, &p_mod.p_aux, &argv, &mut z_err)
    } else {
        module.x_connect(db, &p_mod.p_aux, &argv, &mut z_err)
    };
    let ctx = db.p_vtab_ctx.pop().expect("VtabCtx empilhado acima");
    let b_declared = ctx.b_declared;
    let mut new_tab: Box<Table> = match ctx.p_tab {
        Some(t) => t,
        None => Box::new((**p_tab).clone()),
    };

    let rc;
    match res {
        Err(e) => {
            rc = e;
            if rc == SQLITE_NOMEM {
                oom_fault(db);
            }
            *pz_err = match z_err {
                None => db_printf(
                    db,
                    b"vtable constructor failed: %s",
                    &[text_arg(&z_module_name)],
                ),
                Some(m) => db_printf(db, b"%s", &[text_arg(&m)]),
            };
            db.vtabs.remove(vid.slot());
        }
        Ok(v) => {
            if let Some(vt) = db.vtabs.get_mut(vid.slot()) {
                vt.p_vtab = Some(v);
                vt.n_ref = 1;
            }
            if !b_declared {
                *pz_err = db_printf(
                    db,
                    b"vtable constructor did not declare schema: %s",
                    &[text_arg(&z_module_name)],
                );
                vtab_unlock(db, vid);
                rc = SQLITE_ERROR;
            } else {
                rc = SQLITE_OK;
                // Deu tudo certo: liga o `VTable` à tabela. Depois percorre as colunas atrás da
                // palavra "hidden" no tipo; achando, liga `COLFLAG_HIDDEN` e tira a palavra do
                // texto do tipo.
                if let Some(vt) = db.vtabs.get_mut(vid.slot()) {
                    vt.schema = new_tab.p_schema;
                    vt.z_tab_name = new_tab.z_name.clone();
                    vt.b_eponymous = (new_tab.tab_flags & TF_EPONYMOUS) != 0;
                }
                let mut ooo_hidden: u32 = 0;
                let n_col = (new_tab.n_col.max(0) as usize).min(new_tab.a_col.len());
                for i_col in 0..n_col {
                    let col = &mut new_tab.a_col[i_col];
                    let hidden = (col.col_flags & COLFLAG_HASTYPE) != 0
                        && strip_hidden_token(&mut col.z_cn_name);
                    if hidden {
                        col.col_flags |= COLFLAG_HIDDEN;
                        new_tab.tab_flags |= TF_HAS_HIDDEN;
                        ooo_hidden = TF_OOO_HIDDEN;
                    } else {
                        new_tab.tab_flags |= ooo_hidden;
                    }
                }
            }
        }
    }

    let new_rc = Rc::new(*new_tab);
    publish_table(db, &new_rc);
    *p_tab = new_rc;
    rc
}

/// `sqlite3VtabCallConnect`: o analisador a chama para rodar o `xConnect` da tabela virtual
/// `p_tab`. Em caso de erro devolve o código e deixa a mensagem em `parse`. `p_tab` passa a ser a
/// tabela atualizada pelo construtor.
pub fn vtab_call_connect(db: &mut Connection, parse: &mut Parse, p_tab: &mut Rc<Table>) -> i32 {
    debug_assert!(p_tab.is_virtual());
    if get_vtable(db, p_tab).is_some() {
        return SQLITE_OK;
    }

    // Acha o módulo da tabela virtual.
    let z_mod = p_tab.u_vtab().and_then(|v| v.az_arg.first()).cloned().unwrap_or_default();
    let Some(p_mod) = hash_find(&db.a_module, &z_mod).cloned() else {
        error_msg(db, parse, b"no such module: %s", &[text_arg(&z_mod)]);
        return SQLITE_ERROR;
    };
    let mut z_err: Option<Vec<u8>> = None;
    let rc = vtab_call_constructor(db, p_tab, &p_mod, false, &mut z_err);
    if rc != SQLITE_OK {
        error_msg(db, parse, b"%s", &[text_arg(&z_err.unwrap_or_default())]);
        parse.rc = rc;
    }
    rc
}

/// `addToVTrans`: acrescenta o `VTable` a `a_v_trans` e o trava.
fn add_to_vtrans(db: &mut Connection, vid: VTableId) {
    db.a_v_trans.push(vid);
    vtab_lock(db, vid);
}

/// `sqlite3VtabCallCreate`: o VDBE a chama para rodar o `xCreate` da tabela virtual `z_tab` do
/// banco `i_db`. Em caso de erro deixa a descrição em `pz_err` e devolve o código.
pub fn vtab_call_create(
    db: &mut Connection,
    i_db: i32,
    z_tab: &[u8],
    pz_err: &mut Option<Vec<u8>>,
) -> i32 {
    let z_db = db.dbs.get(i_db as usize).map(|d| d.z_db_s_name.clone()).unwrap_or_default();
    let Some(mut p_tab) = find_table(db, z_tab, Some(&z_db)) else {
        return SQLITE_ERROR;
    };
    debug_assert!(p_tab.is_virtual() && get_vtable(db, &p_tab).is_none());

    // Acha o módulo da tabela virtual.
    let z_mod = p_tab.u_vtab().and_then(|v| v.az_arg.first()).cloned().unwrap_or_default();
    let p_mod = hash_find(&db.a_module, &z_mod).cloned();

    // Se o módulo está registrado e tem `xCreate` (e `xDestroy`, que acompanha o `xCreate`),
    // chama. Senão, é erro.
    let rc = match p_mod {
        Some(m) if m.p_module.caps().create => vtab_call_constructor(db, &mut p_tab, &m, true, pz_err),
        _ => {
            *pz_err = db_printf(db, b"no such module: %s", &[text_arg(&z_mod)]);
            SQLITE_ERROR
        }
    };

    if rc == SQLITE_OK {
        if let Some(vid) = get_vtable(db, &p_tab) {
            add_to_vtrans(db, vid);
        }
    }
    rc
}

/// `sqlite3_declare_vtab`: define o esquema de uma tabela virtual. Só vale dentro do `xCreate` ou
/// do `xConnect` do módulo.
pub fn declare_vtab(db: &mut Connection, z_create_table: &[u8]) -> i32 {
    // Confere que as duas primeiras palavras do CREATE TABLE são mesmo "CREATE" e "TABLE"; senão
    // a função está sendo mal usada.
    let mut pos = 0usize;
    for kw in [TK_CREATE, TK_TABLE] {
        let mut token_type = 0i32;
        loop {
            pos += get_token(z_create_table.get(pos..).unwrap_or(&[]), &mut token_type) as usize;
            if token_type != TK_SPACE as i32 {
                break;
            }
        }
        if token_type != kw as i32 {
            error_with_msg(db, SQLITE_ERROR, b"syntax error", &[]);
            return SQLITE_ERROR;
        }
    }

    match db.p_vtab_ctx.last() {
        Some(c) if !c.b_declared => {}
        _ => {
            let rc = misuse_error(156235);
            error(db, rc);
            return misuse_error(156237);
        }
    }

    let mut s_parse = parse_object_init(db);
    s_parse.e_parse_mode = PARSE_MODE_DECLARE_VTAB;
    s_parse.disable_triggers = 1;
    // Nunca se chega aqui lendo o esquema; por segurança desliga `init.busy` caso um defeito
    // apareça.
    debug_assert!(db.init.busy == 0);
    let init_busy = db.init.busy;
    db.init.busy = 0;
    s_parse.n_query_loop = 1;
    let mut rc = SQLITE_OK;
    if SQLITE_OK == run_parser(db, &mut s_parse, z_create_table) {
        debug_assert!(s_parse.z_err_msg.is_none());
        let has_update = db
            .p_vtab_ctx
            .last()
            .and_then(|c| db.vtabs.get(c.p_v_table.slot()))
            .is_some_and(|vt| vt.p_mod.p_module.caps().update);
        if let (Some(p_new), Some(ctx)) =
            (s_parse.p_new_table.as_deref_mut(), db.p_vtab_ctx.last_mut())
        {
            debug_assert!(p_new.is_ordinary_table());
            if let Some(p_tab) = ctx.p_tab.as_deref_mut() {
                if p_tab.a_col.is_empty() {
                    p_tab.a_col = std::mem::take(&mut p_new.a_col);
                    if let TableU::Tab(info) = &mut p_new.u {
                        info.p_dflt_list = None;
                    }
                    p_tab.n_col = p_new.n_col;
                    p_tab.n_nv_col = p_new.n_col;
                    p_tab.tab_flags |= p_new.tab_flags & (TF_WITHOUT_ROWID | TF_NO_VISIBLE_ROWID);
                    p_new.n_col = 0;
                    debug_assert!(p_tab.p_index.is_empty());
                    debug_assert!(p_new.has_rowid() || primary_key_index(p_new).is_some());
                    if !p_new.has_rowid()
                        && has_update
                        && primary_key_index(p_new).map_or(true, |i| i.n_key_col != 1)
                    {
                        // As tabelas virtuais WITHOUT ROWID ou são somente leitura (sem
                        // `xUpdate`) ou têm PRIMARY KEY de uma coluna só.
                        rc = SQLITE_ERROR;
                    }
                    p_tab.p_index = std::mem::take(&mut p_new.p_index);
                }
            }
            ctx.b_declared = true;
        }
    } else {
        match s_parse.z_err_msg.take() {
            Some(m) => error_with_msg(db, SQLITE_ERROR, b"%s", &[text_arg(&m)]),
            None => error(db, SQLITE_ERROR),
        }
        rc = SQLITE_ERROR;
    }
    s_parse.e_parse_mode = PARSE_MODE_NORMAL;

    if let Some(v) = s_parse.p_vdbe.take() {
        vdbe_finalize(*v, db);
    }
    if let Some(t) = s_parse.p_new_table.take() {
        delete_table(db, Some(Rc::new(*t)));
    }
    parse_object_reset(db, &mut s_parse);
    db.init.busy = init_busy;

    debug_assert!((rc & 0xff) == rc);
    api_exit(db, rc)
}

/// `sqlite3VtabCallDestroy`: o VDBE a chama para rodar o `xDestroy` da tabela virtual `z_tab` do
/// banco `i_db`, num DROP TABLE. Não faz nada se `z_tab` não é tabela virtual.
pub fn vtab_call_destroy(db: &mut Connection, i_db: i32, z_tab: &[u8]) -> i32 {
    let z_db = db.dbs.get(i_db as usize).map(|d| d.z_db_s_name.clone()).unwrap_or_default();
    let Some(p_tab) = find_table(db, z_tab, Some(&z_db)) else {
        return SQLITE_OK;
    };
    if !p_tab.is_virtual() {
        return SQLITE_OK;
    }
    let Some(vid) = get_vtable(db, &p_tab) else {
        return SQLITE_OK;
    };
    // Cursores abertos: a tabela está em uso.
    let Some((n_cursor, has_destroy)) = db
        .vtabs
        .get(vid.slot())
        .map(|vt| (vt.n_cursor, vt.p_mod.p_module.caps().create))
    else {
        return SQLITE_OK;
    };
    if n_cursor > 0 {
        return SQLITE_LOCKED;
    }
    // O `vtabDisconnectAll(db, pTab)` do C só move para `pDisconnect` os `VTable` de outras
    // conexões; no modelo v2 não existem.
    //
    // Sem `xDestroy` o C usa o `xDisconnect`.
    let rc = with_vtab(db, vid, |db, v| if has_destroy { v.destroy(db) } else { v.disconnect(db) })
        .unwrap_or(SQLITE_ERROR);
    // Tira o `sqlite3_vtab*` de `a_v_trans`, se for o caso.
    if rc == SQLITE_OK {
        if let Some(vt) = db.vtabs.get_mut(vid.slot()) {
            vt.p_vtab = None;
        }
        vtab_unlink(db, vid);
        vtab_unlock(db, vid);
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// Transações e savepoints
// ---------------------------------------------------------------------------------------------

/// `sqlite3VtabInSync`: verdadeiro durante o `xSync` (e os finalizadores) das tabelas virtuais,
/// quando escrever nelas é ilegal.
pub fn vtab_in_sync(db: &Connection) -> bool {
    db.a_v_trans.first() == Some(&SYNC_MARK)
}

/// Qual dos finalizadores `callFinaliser` chama.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Finaliser {
    Rollback,
    Commit,
}

/// `callFinaliser`: chama o `xRollback` ou o `xCommit` de cada tabela virtual de `a_v_trans`, que
/// no fim fica vazio.
fn call_finaliser(db: &mut Connection, which: Finaliser) {
    if db.a_v_trans.is_empty() || vtab_in_sync(db) {
        return;
    }
    let list = std::mem::replace(&mut db.a_v_trans, vec![SYNC_MARK]);
    for vid in list {
        let has = db.vtabs.get(vid.slot()).is_some_and(|vt| {
            let caps = vt.p_mod.p_module.caps();
            vt.p_vtab.is_some()
                && match which {
                    Finaliser::Rollback => caps.rollback,
                    Finaliser::Commit => caps.commit,
                }
        });
        if has {
            with_vtab(db, vid, |db, v| match which {
                Finaliser::Rollback => v.rollback(db),
                Finaliser::Commit => v.commit(db),
            });
        }
        if let Some(vt) = db.vtabs.get_mut(vid.slot()) {
            vt.i_savepoint = 0;
        }
        vtab_unlock(db, vid);
    }
    db.a_v_trans.clear();
}

/// `sqlite3VtabSync`: chama o `xSync` de todas as tabelas virtuais de `a_v_trans`. Devolve o
/// código do primeiro erro; a mensagem, se houver, vai para `p.z_err_msg`.
pub fn vtab_sync(db: &mut Connection, p: &mut Vdbe) -> i32 {
    let mut rc = SQLITE_OK;
    let a_v_trans = std::mem::replace(&mut db.a_v_trans, vec![SYNC_MARK]);
    for &vid in a_v_trans.iter() {
        if rc != SQLITE_OK {
            break;
        }
        let has_sync = db
            .vtabs
            .get(vid.slot())
            .is_some_and(|vt| vt.p_vtab.is_some() && vt.p_mod.p_module.caps().sync);
        if has_sync {
            rc = with_vtab(db, vid, |db, vtab| {
                let r = vtab.sync(db);
                vtab_import_errmsg(p, vtab.err_msg_mut());
                r
            })
            .unwrap_or(SQLITE_ERROR);
        }
    }
    db.a_v_trans = a_v_trans;
    rc
}

/// `sqlite3VtabRollback`: chama o `xRollback` de todas as tabelas virtuais de `a_v_trans` e
/// esvazia a lista.
pub fn vtab_rollback(db: &mut Connection) -> i32 {
    call_finaliser(db, Finaliser::Rollback);
    SQLITE_OK
}

/// `sqlite3VtabCommit`: chama o `xCommit` de todas as tabelas virtuais de `a_v_trans` e esvazia a
/// lista.
pub fn vtab_commit(db: &mut Connection) -> i32 {
    call_finaliser(db, Finaliser::Commit);
    SQLITE_OK
}

/// `sqlite3VtabBegin`: se a tabela virtual tem interface de transação (`xBegin`) e não há uma
/// aberta, chama o `xBegin` agora e a põe em `a_v_trans`.
pub fn vtab_begin(db: &mut Connection, p_vtab: Option<VTableId>) -> i32 {
    // Caso especial: dentro do `xSync` de um módulo é ilegal escrever em tabelas virtuais.
    if vtab_in_sync(db) {
        return SQLITE_LOCKED;
    }
    let Some(vid) = p_vtab else {
        return SQLITE_OK;
    };
    let Some(module) = db.vtabs.get(vid.slot()).map(|vt| Rc::clone(&vt.p_mod.p_module)) else {
        return SQLITE_OK;
    };
    let caps = module.caps();
    let mut rc = SQLITE_OK;
    if caps.begin {
        // Já está em `a_v_trans`: nada a fazer.
        if db.a_v_trans.contains(&vid) {
            return SQLITE_OK;
        }
        // Chama o `xBegin` e, dando certo, põe a tabela em `a_v_trans`.
        rc = with_vtab(db, vid, |db, v| v.begin(db)).unwrap_or(SQLITE_ERROR);
        if rc == SQLITE_OK {
            let i_svpt = db.n_statement + db.n_savepoint;
            add_to_vtrans(db, vid);
            if i_svpt != 0 && caps.savepoint {
                if let Some(vt) = db.vtabs.get_mut(vid.slot()) {
                    vt.i_savepoint = i_svpt;
                }
                rc = with_vtab(db, vid, |db, v| v.savepoint(db, i_svpt - 1)).unwrap_or(SQLITE_ERROR);
            }
        }
    }
    rc
}

/// `sqlite3VtabSavepoint`: chama o `xSavepoint`, `xRollbackTo` ou `xRelease` de todas as tabelas
/// virtuais com transação aberta, passando `i_savepoint`. `op` é `SAVEPOINT_BEGIN`,
/// `SAVEPOINT_ROLLBACK` ou `SAVEPOINT_RELEASE`. O primeiro erro abandona o resto.
pub fn vtab_savepoint(db: &mut Connection, op: i32, i_savepoint: i32) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(op == SAVEPOINT_BEGIN || op == SAVEPOINT_ROLLBACK || op == crate::consts::SAVEPOINT_RELEASE);
    debug_assert!(i_savepoint >= -1);
    if db.a_v_trans.is_empty() || vtab_in_sync(db) {
        return rc;
    }
    let mut i = 0usize;
    while rc == SQLITE_OK && i < db.a_v_trans.len() {
        let vid = db.a_v_trans[i];
        i += 1;
        let Some((has_vtab, module)) = db
            .vtabs
            .get(vid.slot())
            .map(|vt| (vt.p_vtab.is_some(), Rc::clone(&vt.p_mod.p_module)))
        else {
            continue;
        };
        if !has_vtab || module.i_version() < 2 {
            continue;
        }
        let caps = module.caps();
        vtab_lock(db, vid);
        let has_method = match op {
            SAVEPOINT_BEGIN => {
                if let Some(vt) = db.vtabs.get_mut(vid.slot()) {
                    vt.i_savepoint = i_savepoint + 1;
                }
                caps.savepoint
            }
            SAVEPOINT_ROLLBACK => caps.rollback_to,
            _ => caps.release,
        };
        let cur = db.vtabs.get(vid.slot()).map_or(0, |vt| vt.i_savepoint);
        if has_method && cur > i_savepoint {
            let saved_flags = db.flags & SQLITE_DEFENSIVE;
            db.flags &= !SQLITE_DEFENSIVE;
            rc = with_vtab(db, vid, |db, v| match op {
                SAVEPOINT_BEGIN => v.savepoint(db, i_savepoint),
                SAVEPOINT_ROLLBACK => v.rollback_to(db, i_savepoint),
                _ => v.release(db, i_savepoint),
            })
            .unwrap_or(SQLITE_ERROR);
            db.flags |= saved_flags;
        }
        vtab_unlock(db, vid);
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// Sobrecarga de função e escrita
// ---------------------------------------------------------------------------------------------

/// `sqlite3VtabOverloadFunction`: `p_def` é a implementação de uma função e `p_expr` o primeiro
/// argumento dela. Se `p_expr` é uma coluna de tabela virtual, a implementação do módulo pode
/// sobrecarregar a função (MATCH, LIKE, GLOB, REGEXP). Devolve `p_def` (sem mudança) ou uma
/// definição nova marcada `SQLITE_FUNC_EPHEM`. `own` é a tabela dona da árvore, para o caso
/// `TabRef::Own`.
pub fn vtab_overload_function(
    db: &mut Connection,
    p_def: Rc<FuncDef>,
    n_arg: i32,
    p_expr: Option<&Expr>,
    own: Option<&Rc<Table>>,
) -> Rc<FuncDef> {
    // Vê se o operando esquerdo é coluna de tabela virtual.
    let Some(e) = p_expr else {
        return p_def;
    };
    if e.op != TK_COLUMN {
        return p_def;
    }
    let p_tab = match e.y_tab() {
        Some(TabRef::Rc(t)) => t,
        Some(TabRef::Own) => match own {
            Some(t) => t,
            None => return p_def,
        },
        None => return p_def,
    };
    if !p_tab.is_virtual() {
        return p_def;
    }
    let Some(vid) = get_vtable(db, p_tab) else {
        return p_def;
    };
    let can_overload = db
        .vtabs
        .get(vid.slot())
        .is_some_and(|vt| vt.p_vtab.is_some() && vt.p_mod.p_module.caps().find_function);
    if !can_overload {
        return p_def;
    }

    // Chama o `xFindFunction` da tabela virtual para ver se quer sobrecarregar a função. O nome
    // vai sempre em minúsculas, como sempre foi.
    let mut found: Option<(crate::connection::ScalarFn, UserData)> = None;
    let rc = with_vtab(db, vid, |db, v| v.find_function(db, n_arg, &p_def.z_name, &mut found))
        .unwrap_or(0);
    if rc == 0 {
        return p_def;
    }

    // Cria a definição nova, efêmera, da função sobrecarregada.
    let (x_s_func, p_user_data) = match found {
        Some((f, a)) => (Some(f), a),
        None => (None, UserData::None),
    };
    Rc::new(FuncDef {
        n_arg: p_def.n_arg,
        func_flags: p_def.func_flags | SQLITE_FUNC_EPHEM,
        p_user_data,
        x_s_func,
        x_finalize: p_def.x_finalize,
        x_value: p_def.x_value,
        x_inverse: p_def.x_inverse,
        z_name: p_def.z_name.clone(),
        p_destructor: p_def.p_destructor.clone(),
    })
}

/// `sqlite3VtabMakeWritable`: garante que a tabela virtual está em `parse.ap_vtab_lock` (do
/// `Parse` de mais alto nível), para o VDBE gerar um `OP_VBegin` para ela.
pub fn vtab_make_writable(_db: &Connection, parse: &mut Parse, p_tab: &Rc<Table>) {
    debug_assert!(p_tab.is_virtual());
    let p_toplevel = parse.toplevel_mut();
    if p_toplevel.ap_vtab_lock.iter().any(|t| Rc::ptr_eq(t, p_tab)) {
        return;
    }
    p_toplevel.ap_vtab_lock.push(Rc::clone(p_tab));
}

// ---------------------------------------------------------------------------------------------
// Tabelas eponímias
// ---------------------------------------------------------------------------------------------

/// `sqlite3VtabEponymousTableInit`: confere se o módulo `p_mod` pode ter uma tabela virtual
/// eponímia e, podendo, cria uma se ainda não existe. Devolve diferente de zero se a eponímia
/// existe ao voltar ou se a tentativa de criá-la falhou e deixou mensagem em `parse`. Só módulos
/// com `xCreate` e `xConnect` iguais têm eponímia.
pub fn vtab_eponymous_table_init(db: &mut Connection, parse: &mut Parse, p_mod: &Rc<Module>) -> i32 {
    if hash_find(&db.a_epo_tab, &p_mod.z_name).is_some() {
        return 1;
    }
    let caps = p_mod.p_module.caps();
    if caps.create && !p_mod.p_module.create_is_connect() {
        return 0;
    }
    let mut tab = Table::default();
    tab.z_name = p_mod.z_name.clone();
    tab.e_tab_type = TABTYP_VTAB;
    tab.p_schema = db.dbs[0].schema.id;
    tab.i_p_key = -1;
    tab.tab_flags |= TF_EPONYMOUS;
    tab.u = TableU::VTab(VTabInfo::default());
    let z_name = tab.z_name.clone();
    add_module_argument(db, parse, &mut tab, z_name.clone());
    add_module_argument(db, parse, &mut tab, Vec::new());
    add_module_argument(db, parse, &mut tab, z_name);
    let mut p_epo = Rc::new(tab);
    // O `pMod->pEpoTab = pTab` do C vem antes do construtor: um `xConnect` que consulta a própria
    // eponímia a encontra.
    hash_insert(&mut db.a_epo_tab, &p_mod.z_name, Some(Rc::clone(&p_epo)));
    let mut z_err: Option<Vec<u8>> = None;
    let rc = vtab_call_constructor(db, &mut p_epo, p_mod, false, &mut z_err);
    if rc != SQLITE_OK {
        error_msg(db, parse, b"%s", &[text_arg(&z_err.unwrap_or_default())]);
        vtab_eponymous_table_clear(db, &p_mod.z_name);
    }
    1
}

/// `sqlite3VtabEponymousTableClear`: apaga a tabela virtual eponímia do módulo `z_mod_name`, se
/// existe.
pub fn vtab_eponymous_table_clear(db: &mut Connection, z_mod_name: &[u8]) {
    if let Some(mut p_tab) = hash_insert(&mut db.a_epo_tab, z_mod_name, None) {
        // Marca a tabela como efêmera antes de apagá-la, para o `delete_table` saber que ela não
        // está guardada no esquema.
        Rc::make_mut(&mut p_tab).tab_flags |= TF_EPHEMERAL;
        delete_table(db, Some(p_tab));
    }
}

// ---------------------------------------------------------------------------------------------
// API para os módulos
// ---------------------------------------------------------------------------------------------

/// `sqlite3_vtab_on_conflict`: a resolução de conflito ON CONFLICT da atualização de tabela
/// virtual em curso. O resultado é indefinido fora de um `xUpdate`.
pub fn vtab_on_conflict(db: &Connection) -> i32 {
    const A_MAP: [i32; 5] = [SQLITE_ROLLBACK, SQLITE_ABORT, SQLITE_FAIL, SQLITE_IGNORE, SQLITE_REPLACE];
    debug_assert!(db.vtab_on_conflict >= 1 && db.vtab_on_conflict <= 5);
    A_MAP[(db.vtab_on_conflict as usize).saturating_sub(1).min(4)]
}

/// `sqlite3_vtab_config`: chamada de dentro do `xCreate` ou do `xConnect`, informa ao núcleo
/// mais sobre o comportamento da tabela virtual. `arg` é o `int` de
/// `SQLITE_VTAB_CONSTRAINT_SUPPORT`; as outras operações o ignoram.
pub fn vtab_config(db: &mut Connection, op: i32, arg: i32) -> i32 {
    let mut rc = SQLITE_OK;
    match db.p_vtab_ctx.last().map(|c| c.p_v_table) {
        None => rc = misuse_error(156731),
        Some(vid) => match op {
            SQLITE_VTAB_CONSTRAINT_SUPPORT => {
                if let Some(vt) = db.vtabs.get_mut(vid.slot()) {
                    vt.b_constraint = arg as u8;
                }
            }
            SQLITE_VTAB_INNOCUOUS => {
                if let Some(vt) = db.vtabs.get_mut(vid.slot()) {
                    vt.e_vtab_risk = SQLITE_VTABRISK_LOW;
                }
            }
            SQLITE_VTAB_DIRECTONLY => {
                if let Some(vt) = db.vtabs.get_mut(vid.slot()) {
                    vt.e_vtab_risk = SQLITE_VTABRISK_HIGH;
                }
            }
            SQLITE_VTAB_USES_ALL_SCHEMAS => {
                if let Some(vt) = db.vtabs.get_mut(vid.slot()) {
                    vt.b_all_schemas = 1;
                }
            }
            _ => rc = misuse_error(156753),
        },
    }
    if rc != SQLITE_OK {
        error(db, rc);
    }
    rc
}
