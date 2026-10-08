// Mesclado das partes traduzidas de vtab_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Contexto de construção de tabela virtual. Antes de um `xCreate()` ou
/// `xConnect()` ser invocado, `db.p_vtab_ctx` aponta para uma instância desta
/// estrutura. Ela é usada por `sqlite3_declare_vtab()` e `sqlite3_vtab_config()`,
/// que só são chamadas de dentro de `xCreate` e `xConnect`.
pub struct VtabCtx {
    /// A tabela virtual sendo construída.
    pub p_vtable: VTableRef,
    /// O objeto `Table` ao qual a tabela virtual pertence.
    pub p_tab: TableRef,
    /// Contexto pai (se houver).
    pub p_prior: Option<VtabCtxRef>,
    /// Verdadeiro depois que `sqlite3_declare_vtab()` foi chamada.
    pub b_declared: bool,
}

pub type VtabCtxRef = Rc<RefCell<VtabCtx>>;

/// Construir e instalar um objeto `Module` para uma tabela virtual. Quando esta
/// rotina é chamada, é garantido que todos os travamentos apropriados estão
/// mantidos e que o módulo ainda não faz parte da conexão.
///
/// Se já existe um módulo com `z_name`, ele é substituído pelo novo. Se
/// `p_module` é `None`, o módulo `z_name` é removido, se existir.
pub fn vtab_create_module(
    db: &Sqlite3Ref,
    z_name: &[u8],
    p_module: Option<Rc<Sqlite3Module>>,
    p_aux: CallbackArg,
    x_destroy: Option<ModuleDestroyFn>,
) -> Option<ModuleRef> {
    let mut p_mod = p_module.map(|p_module| {
        Rc::new(RefCell::new(Module {
            z_name: z_name.to_vec(),
            p_module,
            p_aux,
            x_destroy,
            p_epo_tab: None,
            n_ref_module: 1,
        }))
    });
    let p_del = hash_insert(&mut db.borrow_mut().a_module, z_name, p_mod.clone());
    if let Some(p_del) = p_del {
        if p_mod.as_ref().is_some_and(|m| Rc::ptr_eq(m, &p_del)) {
            // O hash devolveu o próprio módulo novo: falha de memória na inserção.
            oom_fault(db);
            p_mod = None;
        } else {
            vtab_eponymous_table_clear(db, &p_del);
            vtab_module_unref(db, &p_del);
        }
    }
    p_mod
}

/// A função que realmente cria um módulo novo. Implementa as interfaces
/// `sqlite3_create_module()` e `sqlite3_create_module_v2()`.
fn create_module(
    db: &Sqlite3Ref,
    z_name: &[u8],
    p_module: Option<Rc<Sqlite3Module>>,
    p_aux: CallbackArg,
    x_destroy: Option<ModuleDestroyFn>,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mutex = db.borrow().mutex.clone();

    mutex_enter(&mutex);
    let _ = vtab_create_module(db, z_name, p_module, p_aux.clone(), x_destroy.clone());
    rc = api_exit(db, rc);
    if rc != SQLITE_OK {
        if let Some(x_destroy) = &x_destroy {
            x_destroy(&p_aux);
        }
    }
    mutex_leave(&mutex);
    rc
}

/// Função da API externa usada para criar um novo módulo de tabela virtual.
pub fn api_create_module(
    db: &Sqlite3Ref,
    z_name: &[u8],
    p_module: Option<Rc<Sqlite3Module>>,
    p_aux: CallbackArg,
) -> i32 {
    create_module(db, z_name, p_module, p_aux, None)
}

/// Função da API externa usada para criar um novo módulo de tabela virtual,
/// com destrutor.
pub fn api_create_module_v2(
    db: &Sqlite3Ref,
    z_name: &[u8],
    p_module: Option<Rc<Sqlite3Module>>,
    p_aux: CallbackArg,
    x_destroy: Option<ModuleDestroyFn>,
) -> i32 {
    create_module(db, z_name, p_module, p_aux, x_destroy)
}

/// API externa para remover todos os módulos de tabela virtual, exceto os
/// nomeados na lista `az_names`.
pub fn api_drop_modules(db: &Sqlite3Ref, az_names: Option<&[&[u8]]>) -> i32 {
    // O C guarda o próximo elemento antes de remover o atual: iterar sobre um
    // instantâneo da lista, na mesma ordem, tem o mesmo efeito.
    let modules = hash_values(&db.borrow().a_module);
    for p_mod in modules {
        let z_mod_name = p_mod.borrow().z_name.clone();
        if let Some(az_names) = az_names {
            if az_names.iter().any(|z| *z == z_mod_name.as_slice()) {
                continue;
            }
        }
        create_module(db, &z_mod_name, None, None, None);
    }
    SQLITE_OK
}

/// Decrementar a contagem de referências de um objeto `Module`. Destruir o
/// módulo quando a contagem chega a zero.
pub fn vtab_module_unref(_db: &Sqlite3Ref, p_mod: &ModuleRef) {
    let n_ref_module = {
        let mut m = p_mod.borrow_mut();
        debug_assert!(m.n_ref_module > 0);
        m.n_ref_module -= 1;
        m.n_ref_module
    };
    if n_ref_module == 0 {
        let (x_destroy, p_aux) = {
            let m = p_mod.borrow();
            (m.x_destroy.clone(), m.p_aux.clone())
        };
        if let Some(x_destroy) = x_destroy {
            x_destroy(&p_aux);
        }
        debug_assert!(p_mod.borrow().p_epo_tab.is_none());
    }
}

/// Travar a tabela virtual para que ela não possa ser desconectada. Os travamentos
/// se aninham. Todo travamento deve ter um destravamento correspondente. Se um
/// destravamento for omitido, ocorrerão vazamentos de recursos.
///
/// Se uma desconexão for tentada enquanto a tabela virtual está travada, a
/// desconexão é adiada até que todos os travamentos sejam removidos.
pub fn vtab_lock(p_vtab: &VTableRef) {
    p_vtab.borrow_mut().n_ref += 1;
}

/// Diz se o objeto `VTable` é o que a conexão `db` usa para acessar a tabela.
fn vtable_is_for(p_vtab: &VTableRef, db: &Sqlite3Ref) -> bool {
    p_vtab.borrow().db.ptr_eq(&Rc::downgrade(db))
}

/// `p_tab` representa uma tabela virtual. Devolve o objeto `VTable` usado pela
/// conexão `db` para acessar essa tabela virtual, se um foi criado, ou `None`.
pub fn get_vtable(db: &Sqlite3Ref, p_tab: &TableRef) -> Option<VTableRef> {
    debug_assert!(is_virtual(&p_tab.borrow()));
    let mut p_vtab = vtab_info(&p_tab.borrow()).p.clone();
    while let Some(v) = p_vtab {
        if vtable_is_for(&v, db) {
            return Some(v);
        }
        let p_next = v.borrow().p_next.clone();
        p_vtab = p_next;
    }
    None
}

/// Decrementar a contagem de referências de um objeto de tabela virtual. Quando
/// a contagem chega a zero, chamar o método `xDisconnect()` para apagar o objeto.
pub fn vtab_unlock(p_vtab: &VTableRef) {
    let db = p_vtab.borrow().db.upgrade();
    debug_assert!(db.is_some());
    let Some(db) = db else {
        return;
    };
    debug_assert!(p_vtab.borrow().n_ref > 0);
    debug_assert!({
        let e_open_state = db.borrow().e_open_state;
        e_open_state == SQLITE_STATE_OPEN || e_open_state == SQLITE_STATE_ZOMBIE
    });

    let n_ref = {
        let mut v = p_vtab.borrow_mut();
        v.n_ref -= 1;
        v.n_ref
    };
    if n_ref == 0 {
        let (p, p_mod) = {
            let v = p_vtab.borrow();
            (v.p_vtab.clone(), v.p_mod.clone())
        };
        if let Some(p) = p {
            let x_disconnect = p.borrow().p_module.as_ref().and_then(|m| m.x_disconnect.clone());
            if let Some(x_disconnect) = x_disconnect {
                let _ = x_disconnect(&p);
            }
        }
        vtab_module_unref(&db, &p_mod);
    }
}

/// A tabela `p` é virtual. Move todos os elementos da lista `p.u.vtab.p` para as
/// listas `sqlite3.p_disconnect` de suas conexões, para serem desconectados na
/// próxima oportunidade. Exceto que, se `db` não é `None`, a entrada associada à
/// conexão `db` permanece na lista `p.u.vtab.p`.
fn vtab_disconnect_all(db: Option<&Sqlite3Ref>, p: &TableRef) -> Option<VTableRef> {
    let mut p_ret: Option<VTableRef> = None;

    debug_assert!(is_virtual(&p.borrow()));
    let mut p_vtable = vtab_info_mut(&mut p.borrow_mut()).p.take();

    while let Some(v) = p_vtable {
        let (db2, p_next) = {
            let b = v.borrow();
            (b.db.upgrade(), b.p_next.clone())
        };
        debug_assert!(db2.is_some());
        let Some(db2) = db2 else {
            p_vtable = p_next;
            continue;
        };
        if db.is_some_and(|d| Rc::ptr_eq(d, &db2)) {
            p_ret = Some(v.clone());
            vtab_info_mut(&mut p.borrow_mut()).p = p_ret.clone();
            v.borrow_mut().p_next = None;
        } else {
            let p_prev_head = db2.borrow_mut().p_disconnect.replace(v.clone());
            v.borrow_mut().p_next = p_prev_head;
        }
        p_vtable = p_next;
    }

    debug_assert!(db.is_none() || p_ret.is_some());
    p_ret
}

/// A tabela `p` é virtual. Remove da lista encadeada o objeto `VTable` da tabela
/// `p` associado à conexão `db` e decrementa a contagem de referências do
/// `VTable`. Usada ao fechar a conexão `db`, para liberar todos os seus objetos
/// `VTable` sem perturbar o resto do objeto `Schema` (que pode estar em uso por
/// outras conexões de cache compartilhado).
pub fn vtab_disconnect(db: &Sqlite3Ref, p: &TableRef) {
    debug_assert!(is_virtual(&p.borrow()));
    debug_assert!(btree_holds_all_mutexes(db));
    debug_assert!(mutex_held(&db.borrow().mutex));

    let mut p_prev: Option<VTableRef> = None;
    let mut p_cur = vtab_info(&p.borrow()).p.clone();
    while let Some(v) = p_cur {
        let p_next = v.borrow().p_next.clone();
        if vtable_is_for(&v, db) {
            match &p_prev {
                Some(prev) => prev.borrow_mut().p_next = p_next,
                None => vtab_info_mut(&mut p.borrow_mut()).p = p_next,
            }
            vtab_unlock(&v);
            break;
        }
        p_prev = Some(v);
        p_cur = p_next;
    }
}

/// Desconectar todos os objetos de tabela virtual da lista `sqlite3.p_disconnect`.
///
/// Esta função só pode ser chamada quando os mutexes de todos os bancos b-tree
/// compartilhados abertos pela conexão `db` estão em poder do chamador. Isso
/// protege a lista `sqlite3.p_disconnect`, que só é acessada assim:
///
///   1) Por esta função. Nesse caso, todos os mutexes `BtShared` e o mutex do
///      próprio identificador de banco devem estar mantidos.
///
///   2) Por `vtab_disconnect_all()`, quando acrescenta uma entrada `VTable` à
///      lista. Nesse caso o mutex `BtShared` do banco que guarda a tabela virtual
///      está mantido ou, se o banco não é compartilhável, o mutex do
///      identificador de banco está mantido.
///
/// Como resultado, `sqlite3.p_disconnect` não pode ser acessada simultaneamente
/// por várias threads.
pub fn vtab_unlock_list(db: &Sqlite3Ref) {
    debug_assert!(btree_holds_all_mutexes(db));
    debug_assert!(mutex_held(&db.borrow().mutex));

    let mut p = db.borrow_mut().p_disconnect.take();
    while let Some(v) = p {
        let p_next = v.borrow().p_next.clone();
        vtab_unlock(&v);
        p = p_next;
    }
}

/// Limpar toda a informação de tabela virtual do registro `Table`. Chamada, por
/// exemplo, logo antes de apagar o registro `Table`.
///
/// Sendo uma tabela virtual, a estrutura `Table` guarda a cabeça de uma lista
/// encadeada de estruturas `VTable`, cada uma associada a um único usuário
/// `sqlite3*` do schema. A contagem de referências do `VTable` associado à
/// conexão `db` é decrementada de imediato (o que pode levar a `xDisconnect` e à
/// liberação). Os demais `VTable` da lista vão para a lista
/// `sqlite3.p_disconnect` da conexão associada.
pub fn vtab_clear(db: &Sqlite3Ref, p: &TableRef) {
    debug_assert!(is_virtual(&p.borrow()));
    if db.borrow().p_n_bytes_freed.is_none() {
        let _ = vtab_disconnect_all(None, p);
    }
    // Os argumentos do módulo pertencem à tabela: sair do `Vec` os libera.
    vtab_info_mut(&mut p.borrow_mut()).az_arg.clear();
}


// ---- part_001.rs ----

/// Texto SQL coberto pelo token que começa em `z` e tem `n` bytes.
fn parse_text(p_parse: &Parse, z: Option<usize>, n: u32) -> Vec<u8> {
    let z = z.unwrap_or(0);
    p_parse.z_sql[z..z + n as usize].to_vec()
}

/// Acrescenta um novo argumento de módulo a `p_table.u.vtab.az_arg[]`. A string
/// passa a pertencer à tabela e é liberada junto com ela.
fn add_module_argument(p_parse: &mut Parse, p_table: &TableRef, z_arg: Option<Vec<u8>>) {
    debug_assert!(is_virtual(&p_table.borrow()));
    let n_arg = vtab_info(&p_table.borrow()).az_arg.len() as i64;
    let limit = p_parse.db.borrow().a_limit[SQLITE_LIMIT_COLUMN as usize] as i64;
    if n_arg + 3 >= limit {
        let z_name = p_table.borrow().z_name.clone();
        error_msg(p_parse, b"too many columns on %s", &[FmtArg::Text(&z_name)]);
    }
    vtab_info_mut(&mut p_table.borrow_mut()).az_arg.push(z_arg);
}

/// O analisador chama esta rotina quando vê pela primeira vez um comando CREATE
/// VIRTUAL TABLE. O nome do módulo já foi analisado, mas a lista opcional de
/// parâmetros que o segue ainda está pendente.
pub fn vtab_begin_parse(
    p_parse: &mut Parse,
    p_name1: &Token,
    p_name2: &Token,
    p_module_name: &Token,
    if_not_exists: i32,
) {
    start_table(p_parse, p_name1, p_name2, 0, 0, 1, if_not_exists);
    let Some(p_table) = p_parse.p_new_table.clone() else {
        return;
    };
    debug_assert!(p_table.borrow().p_index.is_none());
    {
        let mut t = p_table.borrow_mut();
        t.e_tab_type = TABTYP_VTAB;
        // A união `u` passa a ser a variante de tabela virtual, zerada (nArg==0).
        t.u = TableU::VTab(TableVtab::default());
    }

    let z_module = name_from_token(p_parse, p_module_name);
    add_module_argument(p_parse, &p_table, z_module);
    add_module_argument(p_parse, &p_table, None);
    let z_name = p_table.borrow().z_name.clone();
    add_module_argument(p_parse, &p_table, Some(z_name.clone()));
    debug_assert!(
        (p_parse.s_name_token.z == p_name2.z && p_name2.z.is_some())
            || (p_parse.s_name_token.z == p_name1.z && p_name2.z.is_none())
    );
    let module_end = p_module_name.z.unwrap_or(0) + p_module_name.n as usize;
    let name_start = p_parse.s_name_token.z.unwrap_or(0);
    p_parse.s_name_token.n = (module_end - name_start) as u32;

    // Criar uma tabela virtual chama o callback de autorização duas vezes. A
    // primeira, para obter permissão de inserir uma linha em sqlite_schema, já foi
    // feita por start_table(). A segunda, para criar a tabela, é feita agora.
    let az0 = vtab_info(&p_table.borrow()).az_arg.first().cloned().flatten();
    let db = p_parse.db.clone();
    let i_db = schema_to_index(&db, p_table.borrow().p_schema.as_ref());
    debug_assert!(i_db >= 0);
    let z_db_s_name = db.borrow().a_db[i_db as usize].z_db_s_name.clone();
    let _ = auth_check(
        p_parse,
        SQLITE_CREATE_VTABLE,
        Some(&z_name),
        az0.as_deref(),
        Some(&z_db_s_name),
    );
}

/// Esta rotina pega o argumento de módulo que vem se acumulando em `p_parse.s_arg`
/// e o acrescenta à lista de argumentos da tabela virtual em construção em
/// `p_parse.p_new_table`.
fn add_argument_to_vtab(p_parse: &mut Parse) {
    if p_parse.s_arg.z.is_some() {
        if let Some(p_new_table) = p_parse.p_new_table.clone() {
            let z_arg = parse_text(p_parse, p_parse.s_arg.z, p_parse.s_arg.n);
            add_module_argument(p_parse, &p_new_table, Some(z_arg));
        }
    }
}

/// O analisador chama esta rotina depois que o comando CREATE VIRTUAL TABLE foi
/// completamente analisado.
pub fn vtab_finish_parse(p_parse: &mut Parse, p_end: Option<&Token>) {
    let Some(p_tab) = p_parse.p_new_table.clone() else {
        return;
    };
    let db = p_parse.db.clone();

    debug_assert!(is_virtual(&p_tab.borrow()));
    add_argument_to_vtab(p_parse);
    p_parse.s_arg.z = None;
    if vtab_info(&p_tab.borrow()).az_arg.len() < 1 {
        return;
    }

    // Se o comando CREATE VIRTUAL TABLE está sendo digitado pela primeira vez (a
    // tabela virtual está sendo criada agora, e não apenas lida de sqlite_schema),
    // faz o trabalho adicional de inicialização e guarda o texto do comando na
    // tabela sqlite_schema.
    if db.borrow().init.busy == 0 {
        may_abort(p_parse);

        // Calcula o texto completo do comando CREATE VIRTUAL TABLE.
        if let Some(p_end) = p_end {
            let end = p_end.z.unwrap_or(0) + p_end.n as usize;
            let start = p_parse.s_name_token.z.unwrap_or(0);
            p_parse.s_name_token.n = (end - start) as u32;
        }
        let z_name_token = parse_text(p_parse, p_parse.s_name_token.z, p_parse.s_name_token.n);
        let z_stmt = m_printf(&db, b"CREATE VIRTUAL TABLE %T", &[FmtArg::Token(&z_name_token)]);

        // Um espaço para o registro já foi alocado na tabela de schema. Basta
        // atualizá-lo com tudo que foi coletado. O registrador `reg_rowid` guarda o
        // rowid da entrada de sqlite_schema criada para esta tabela virtual por
        // start_table().
        let i_db = schema_to_index(&db, p_tab.borrow().p_schema.as_ref());
        let z_db_s_name = db.borrow().a_db[i_db as usize].z_db_s_name.clone();
        let z_name = p_tab.borrow().z_name.clone();
        let reg_rowid = p_parse.reg_rowid;
        nested_parse(
            p_parse,
            b"UPDATE %Q.sqlite_master SET type='table', name=%Q, tbl_name=%Q, rootpage=0, sql=%Q WHERE rowid=#%d",
            &[
                FmtArg::Q(Some(&z_db_s_name)),
                FmtArg::Q(Some(&z_name)),
                FmtArg::Q(Some(&z_name)),
                FmtArg::Q(Some(&z_stmt)),
                FmtArg::Int(reg_rowid as i64),
            ],
        );
        let v = get_vdbe(p_parse);
        change_cookie(p_parse, i_db);

        vdbe_add_op0(&v, OP_EXPIRE);
        let z_where = m_printf(
            &db,
            b"name=%Q AND sql=%Q",
            &[FmtArg::Q(Some(&z_name)), FmtArg::Q(Some(&z_stmt))],
        );
        vdbe_add_parse_schema_op(&v, i_db, Some(z_where), 0);

        p_parse.n_mem += 1;
        let i_reg = p_parse.n_mem;
        vdbe_load_string(&v, i_reg, &z_name);
        vdbe_add_op2(&v, OP_VCREATE, i_db, i_reg);
    } else {
        // Se estamos relendo a tabela sqlite_schema, cria o registro da tabela em
        // memória.
        let Some(p_schema) = p_tab.borrow().p_schema.clone() else {
            return;
        };
        let z_name = p_tab.borrow().z_name.clone();
        mark_all_shadow_tables_of(&db, &p_tab);
        let p_old = hash_insert(&mut p_schema.borrow_mut().tbl_hash, &z_name, Some(p_tab.clone()));
        if let Some(p_old) = p_old {
            oom_fault(&db);
            debug_assert!(Rc::ptr_eq(&p_tab, &p_old)); // só falha de malloc no hash
            return;
        }
        p_parse.p_new_table = None;
    }
}

/// O analisador chama esta rotina ao ver o primeiro token de um argumento do nome
/// do módulo em um comando CREATE VIRTUAL TABLE.
pub fn vtab_arg_init(p_parse: &mut Parse) {
    add_argument_to_vtab(p_parse);
    p_parse.s_arg.z = None;
    p_parse.s_arg.n = 0;
}

/// O analisador chama esta rotina para cada token depois do primeiro em um
/// argumento do nome do módulo em um comando CREATE VIRTUAL TABLE.
pub fn vtab_arg_extend(p_parse: &mut Parse, p: &Token) {
    let p_arg = &mut p_parse.s_arg;
    match p_arg.z {
        None => {
            p_arg.z = p.z;
            p_arg.n = p.n;
        }
        Some(z) => {
            let p_z = p.z.unwrap_or(0);
            debug_assert!(z <= p_z);
            p_arg.n = (p_z + p.n as usize - z) as u32;
        }
    }
}

/// Remove o token `hidden` de uma string de tipo de coluna, como o C faz no
/// lugar. Devolve o tipo novo, ou `None` se o token não existe.
fn remove_hidden_token(z_type: &[u8]) -> Option<Vec<u8>> {
    let n_type = z_type.len();
    // O C trabalha numa string terminada em NUL: replica com um NUL no fim.
    let mut buf = z_type.to_vec();
    buf.push(0);
    let mut i = 0;
    while i < n_type {
        if str_n_i_cmp(b"hidden", &buf[i..], 6) == 0
            && (i == 0 || buf[i - 1] == b' ')
            && (buf[i + 6] == 0 || buf[i + 6] == b' ')
        {
            break;
        }
        i += 1;
    }
    if i >= n_type {
        return None;
    }
    let n_del = 6 + if buf[i + 6] != 0 { 1 } else { 0 };
    let mut j = i;
    while j + n_del <= n_type {
        buf[j] = buf[j + n_del];
        j += 1;
    }
    if buf[i] == 0 && i > 0 {
        debug_assert!(buf[i - 1] == b' ');
        buf[i - 1] = 0;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(n_type);
    buf.truncate(end);
    Some(buf)
}

/// Invoca um construtor de tabela virtual (`xCreate` ou `xConnect`). A função a
/// invocar é passada no quarto parâmetro.
fn vtab_call_constructor(
    db: &Sqlite3Ref,
    p_tab: &TableRef,
    p_mod: &ModuleRef,
    x_construct: &VtabConstructFn,
    pz_err: &mut Option<Vec<u8>>,
) -> i32 {
    debug_assert!(is_virtual(&p_tab.borrow()));
    let z_module_name = p_tab.borrow().z_name.clone();

    // Verifica que a tabela virtual já não está sendo inicializada.
    let mut p_ctx = db.borrow().p_vtab_ctx.clone();
    while let Some(c) = p_ctx {
        if Rc::ptr_eq(&c.borrow().p_tab, p_tab) {
            *pz_err = Some(m_printf(
                db,
                b"vtable constructor called recursively: %s",
                &[FmtArg::Text(&z_module_name)],
            ));
            return SQLITE_LOCKED;
        }
        p_ctx = c.borrow().p_prior.clone();
    }

    let p_vtable: VTableRef = Rc::new(RefCell::new(VTable {
        db: Rc::downgrade(db),
        p_mod: p_mod.clone(),
        p_vtab: None,
        n_ref: 0,
        b_constraint: 0,
        b_all_schemas: 0,
        e_vtab_risk: SQLITE_VTABRISK_NORMAL,
        i_savepoint: 0,
        p_next: None,
    }));

    let i_db = schema_to_index(db, p_tab.borrow().p_schema.as_ref());
    let z_db_s_name = db.borrow().a_db[i_db as usize].z_db_s_name.clone();
    let az_arg = {
        let mut t = p_tab.borrow_mut();
        let info = vtab_info_mut(&mut t);
        info.az_arg[1] = Some(z_db_s_name);
        info.az_arg.clone()
    };

    // Invoca o construtor da tabela virtual.
    let s_ctx: VtabCtxRef = Rc::new(RefCell::new(VtabCtx {
        p_vtable: p_vtable.clone(),
        p_tab: p_tab.clone(),
        p_prior: db.borrow().p_vtab_ctx.clone(),
        b_declared: false,
    }));
    db.borrow_mut().p_vtab_ctx = Some(s_ctx.clone());
    p_tab.borrow_mut().n_tab_ref += 1;
    let p_aux = p_mod.borrow().p_aux.clone();
    let mut p_vtab_out: Option<Sqlite3VtabRef> = None;
    let mut z_err: Option<Vec<u8>> = None;
    let mut rc = x_construct(db, &p_aux, &az_arg, &mut p_vtab_out, &mut z_err);
    p_vtable.borrow_mut().p_vtab = p_vtab_out;
    debug_assert!(p_tab.borrow().n_tab_ref > 1 || rc != SQLITE_OK);
    delete_table(db, Some(p_tab.clone()));
    let p_prior = s_ctx.borrow().p_prior.clone();
    db.borrow_mut().p_vtab_ctx = p_prior;
    if rc == SQLITE_NOMEM {
        oom_fault(db);
    }
    debug_assert!(Rc::ptr_eq(&s_ctx.borrow().p_tab, p_tab));

    let p_vtab_new = p_vtable.borrow().p_vtab.clone();
    if rc != SQLITE_OK {
        *pz_err = match z_err {
            None => Some(m_printf(
                db,
                b"vtable constructor failed: %s",
                &[FmtArg::Text(&z_module_name)],
            )),
            // `sqlite3MPrintf("%s", zErr)` apenas copia a mensagem.
            Some(z_err) => Some(z_err),
        };
    } else if let Some(p_vtab) = p_vtab_new {
        // Um construtor correto deve alocar o objeto sqlite3_vtab quando tem êxito.
        let p_module = p_mod.borrow().p_module.clone();
        *p_vtab.borrow_mut() = Sqlite3Vtab {
            p_module: Some(p_module),
            ..Sqlite3Vtab::default()
        };
        p_mod.borrow_mut().n_ref_module += 1;
        p_vtable.borrow_mut().n_ref = 1;
        if !s_ctx.borrow().b_declared {
            *pz_err = Some(m_printf(
                db,
                b"vtable constructor did not declare schema: %s",
                &[FmtArg::Text(&z_module_name)],
            ));
            vtab_unlock(&p_vtable);
            rc = SQLITE_ERROR;
        } else {
            // Se tudo correu como planejado, liga o novo `VTable` à lista que
            // começa em `p_tab.u.vtab.p`. Depois percorre as colunas procurando o
            // token "hidden": se achar, marca COLFLAG_HIDDEN e remove o token do
            // tipo.
            {
                let mut t = p_tab.borrow_mut();
                let info = vtab_info_mut(&mut t);
                p_vtable.borrow_mut().p_next = info.p.take();
                info.p = Some(p_vtable.clone());
            }
            let mut ooo_hidden: u32 = 0;
            let n_col = p_tab.borrow().a_col.len();
            for i_col in 0..n_col {
                let z_type = column_type(&p_tab.borrow().a_col[i_col], b"");
                let mut t = p_tab.borrow_mut();
                if let Some(z_new_type) = remove_hidden_token(&z_type) {
                    column_set_type(&mut t.a_col[i_col], z_new_type);
                    t.a_col[i_col].col_flags |= COLFLAG_HIDDEN;
                    t.tab_flags |= TF_HASHIDDEN;
                    ooo_hidden = TF_OOOHIDDEN;
                } else {
                    t.tab_flags |= ooo_hidden;
                }
            }
        }
    }

    rc
}

/// Esta função é invocada pelo analisador para chamar o método `xConnect()` da
/// tabela virtual `p_tab`. Se ocorre um erro, devolve um código de erro e deixa a
/// mensagem em `p_parse`.
pub fn vtab_call_connect(p_parse: &mut Parse, p_tab: &TableRef) -> i32 {
    let db = p_parse.db.clone();

    debug_assert!(is_virtual(&p_tab.borrow()));
    if get_vtable(&db, p_tab).is_some() {
        return SQLITE_OK;
    }

    // Localiza o módulo de tabela virtual necessário.
    let z_mod = vtab_info(&p_tab.borrow()).az_arg[0].clone().unwrap_or_default();
    let p_mod = hash_find(&db.borrow().a_module, &z_mod);

    let rc;
    match p_mod {
        None => {
            error_msg(p_parse, b"no such module: %s", &[FmtArg::Text(&z_mod)]);
            rc = SQLITE_ERROR;
        }
        Some(p_mod) => {
            let x_connect = p_mod.borrow().p_module.x_connect.clone();
            let mut z_err: Option<Vec<u8>> = None;
            rc = match x_connect {
                Some(x_connect) => vtab_call_constructor(&db, p_tab, &p_mod, &x_connect, &mut z_err),
                None => SQLITE_ERROR, // inalcançável: o C exige xConnect no módulo
            };
            if rc != SQLITE_OK {
                let z_err = z_err.unwrap_or_else(|| b"(null)".to_vec());
                error_msg(p_parse, b"%s", &[FmtArg::Text(&z_err)]);
                p_parse.rc = rc;
            }
        }
    }

    rc
}


// ---- part_002.rs ----

/// Qual método finalizador `call_finaliser` invoca em cada tabela virtual. No C
/// isso é o deslocamento do método na estrutura `sqlite3_module`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VtabFinaliser {
    Rollback,
    Commit,
}

/// Aumenta o arranjo `db.a_v_trans[]` para haver espaço para pelo menos mais uma
/// tabela virtual. Devolve `SQLITE_NOMEM` se uma alocação falha, ou `SQLITE_OK`.
fn grow_v_trans(db: &Sqlite3Ref) -> i32 {
    const ARRAY_INCR: usize = 5;

    // Aumenta o arranjo `sqlite3.a_v_trans` se necessário.
    let mut d = db.borrow_mut();
    if (d.n_v_trans as usize) % ARRAY_INCR == 0 {
        d.a_v_trans.get_or_insert_with(Vec::new).reserve(ARRAY_INCR);
    }
    SQLITE_OK
}

/// Acrescenta a tabela virtual `p_vtab` ao arranjo `sqlite3.a_v_trans[]`. O
/// espaço já deve ter sido reservado com `grow_v_trans()`.
fn add_to_v_trans(db: &Sqlite3Ref, p_vtab: &VTableRef) {
    {
        let mut d = db.borrow_mut();
        d.a_v_trans.get_or_insert_with(Vec::new).push(p_vtab.clone());
        d.n_v_trans += 1;
    }
    vtab_lock(p_vtab);
}

/// Invocada pelo vdbe para chamar o método `xCreate` da tabela virtual `z_tab` do
/// banco `i_db`.
///
/// Se ocorre um erro, `pz_err` recebe a descrição do erro em inglês e um código
/// `SQLITE_XXX` é devolvido.
pub fn vtab_call_create(db: &Sqlite3Ref, i_db: i32, z_tab: &[u8], pz_err: &mut Option<Vec<u8>>) -> i32 {
    let z_db_s_name = db.borrow().a_db[i_db as usize].z_db_s_name.clone();
    let p_tab = find_table(db, z_tab, Some(&z_db_s_name));
    let Some(p_tab) = p_tab else {
        debug_assert!(false);
        return SQLITE_ERROR;
    };
    debug_assert!(is_virtual(&p_tab.borrow()) && vtab_info(&p_tab.borrow()).p.is_none());

    // Localiza o módulo de tabela virtual necessário.
    let z_mod = vtab_info(&p_tab.borrow()).az_arg[0].clone().unwrap_or_default();
    let p_mod = hash_find(&db.borrow().a_module, &z_mod);

    // Se o módulo foi registrado e tem um método Create, invoca-o agora. Se o
    // módulo não foi registrado, devolve um erro. Caso contrário, não faz nada.
    let x_create = p_mod.as_ref().and_then(|m| {
        let module = m.borrow().p_module.clone();
        if module.x_destroy.is_some() {
            module.x_create.clone()
        } else {
            None
        }
    });
    let mut rc;
    match (p_mod, x_create) {
        (Some(p_mod), Some(x_create)) => {
            rc = vtab_call_constructor(db, &p_tab, &p_mod, &x_create, pz_err);
        }
        _ => {
            *pz_err = Some(m_printf(db, b"no such module: %s", &[FmtArg::Text(&z_mod)]));
            rc = SQLITE_ERROR;
        }
    }

    // O método construtor é obrigado a criar um sqlite3_vtab válido se devolve
    // SQLITE_OK.
    if rc == SQLITE_OK {
        if let Some(p_vtable) = get_vtable(db, &p_tab) {
            rc = grow_v_trans(db);
            if rc == SQLITE_OK {
                add_to_v_trans(db, &p_vtable);
            }
        }
    }

    rc
}

/// Usada para definir o schema de uma tabela virtual. Só é válido chamá-la de
/// dentro de `xCreate()` ou `xConnect()` de um módulo de tabela virtual.
pub fn api_declare_vtab(db: &Sqlite3Ref, z_create_table: &[u8]) -> i32 {
    let mut rc = SQLITE_OK;
    const A_KEYWORD: [i32; 2] = [TK_CREATE, TK_TABLE];

    // Verifica que as duas primeiras palavras-chave do comando CREATE TABLE são
    // mesmo "CREATE" e "TABLE". Se não são, `sqlite3_declare_vtab()` está sendo
    // usada de forma errada.
    let mut z = 0usize;
    for &keyword in A_KEYWORD.iter() {
        let mut token_type;
        loop {
            let (n, t) = get_token(&z_create_table[z.min(z_create_table.len())..]);
            z += n;
            token_type = t;
            if token_type != TK_SPACE {
                break;
            }
        }
        if token_type != keyword {
            error_with_msg(db, SQLITE_ERROR, Some(b"syntax error"));
            return SQLITE_ERROR;
        }
    }

    let mutex = db.borrow().mutex.clone();
    mutex_enter(&mutex);
    let p_ctx = db.borrow().p_vtab_ctx.clone();
    let p_ctx = match p_ctx {
        Some(c) if !c.borrow().b_declared => c,
        _ => {
            error(db, SQLITE_MISUSE_BKPT);
            mutex_leave(&mutex);
            return SQLITE_MISUSE_BKPT;
        }
    };

    let p_tab = p_ctx.borrow().p_tab.clone();
    debug_assert!(is_virtual(&p_tab.borrow()));

    let mut s_parse = Parse::default();
    parse_object_init(&mut s_parse, db);
    s_parse.e_parse_mode = PARSE_MODE_DECLARE_VTAB;
    s_parse.disable_triggers = 1;
    // Nunca deveríamos chegar aqui enquanto o schema é carregado. Mesmo assim,
    // defende-se disso (desliga `db.init.busy`) caso um bug apareça.
    debug_assert!(db.borrow().init.busy == 0);
    let init_busy = db.borrow().init.busy;
    db.borrow_mut().init.busy = 0;
    s_parse.n_query_loop = 1;
    if run_parser(&mut s_parse, z_create_table) == SQLITE_OK {
        debug_assert!(s_parse.p_new_table.is_some());
        debug_assert!(!db.borrow().malloc_failed);
        debug_assert!(s_parse.z_err_msg.is_none());
        if let Some(p_new) = s_parse.p_new_table.clone() {
            debug_assert!(is_ordinary_table(&p_new.borrow()));
            if p_tab.borrow().a_col.is_empty() {
                let p_vtable = p_ctx.borrow().p_vtable.clone();
                let has_update = p_vtable.borrow().p_mod.borrow().p_module.x_update.is_some();
                let p_idx;
                {
                    let mut new = p_new.borrow_mut();
                    let mut tab = p_tab.borrow_mut();
                    tab.a_col = std::mem::take(&mut new.a_col);
                    expr_list_delete(db, tab_info_mut(&mut new).p_dflt_list.take());
                    tab.n_col = new.n_col;
                    tab.n_nv_col = new.n_col;
                    tab.tab_flags |= new.tab_flags & (TF_WITHOUTROWID | TF_NOVISIBLEROWID);
                    new.n_col = 0;
                    debug_assert!(tab.p_index.is_none());
                    debug_assert!(has_rowid(&new) || primary_key_index(&new).is_some());
                    if !has_rowid(&new)
                        && has_update
                        && primary_key_index(&new).is_some_and(|i| i.borrow().n_key_col != 1)
                    {
                        // Tabelas virtuais WITHOUT ROWID devem ser somente leitura
                        // (xUpdate==0) ou ter PRIMARY KEY de uma coluna só.
                        rc = SQLITE_ERROR;
                    }
                    p_idx = new.p_index.take();
                }
                if let Some(p_idx) = p_idx {
                    debug_assert!(p_idx.borrow().p_next.is_none());
                    p_idx.borrow_mut().p_table = Rc::downgrade(&p_tab);
                    p_tab.borrow_mut().p_index = Some(p_idx);
                }
            }
            p_ctx.borrow_mut().b_declared = true;
        }
    } else {
        error_with_msg(db, SQLITE_ERROR, s_parse.z_err_msg.as_deref());
        s_parse.z_err_msg = None;
        rc = SQLITE_ERROR;
    }
    s_parse.e_parse_mode = PARSE_MODE_NORMAL;

    if let Some(v) = s_parse.p_vdbe.take() {
        vdbe_finalize(&v);
    }
    delete_table(db, s_parse.p_new_table.take());
    parse_object_reset(&mut s_parse);
    db.borrow_mut().init.busy = init_busy;

    debug_assert!((rc & 0xff) == rc);
    rc = api_exit(db, rc);
    mutex_leave(&mutex);
    rc
}

/// Invocada pelo vdbe para chamar o método `xDestroy` da tabela virtual `z_tab` do
/// banco `i_db`. Ocorre quando um DROP TABLE a menciona.
///
/// Não faz nada se `z_tab` não é uma tabela virtual.
pub fn vtab_call_destroy(db: &Sqlite3Ref, i_db: i32, z_tab: &[u8]) -> i32 {
    let mut rc = SQLITE_OK;

    let z_db_s_name = db.borrow().a_db[i_db as usize].z_db_s_name.clone();
    let Some(p_tab) = find_table(db, z_tab, Some(&z_db_s_name)) else {
        return rc;
    };
    if is_virtual(&p_tab.borrow()) && vtab_info(&p_tab.borrow()).p.is_some() {
        let mut p = vtab_info(&p_tab.borrow()).p.clone();
        while let Some(v) = p {
            let (p_vtab, p_next) = {
                let b = v.borrow();
                (b.p_vtab.clone(), b.p_next.clone())
            };
            debug_assert!(p_vtab.is_some());
            if p_vtab.is_some_and(|x| x.borrow().n_ref > 0) {
                return SQLITE_LOCKED;
            }
            p = p_next;
        }
        if let Some(p) = vtab_disconnect_all(Some(db), &p_tab) {
            let (p_vtab, p_module) = {
                let b = p.borrow();
                (b.p_vtab.clone(), b.p_mod.borrow().p_module.clone())
            };
            let x_destroy = p_module.x_destroy.clone().or_else(|| p_module.x_disconnect.clone());
            debug_assert!(x_destroy.is_some());
            p_tab.borrow_mut().n_tab_ref += 1;
            if let (Some(x_destroy), Some(p_vtab)) = (x_destroy, p_vtab) {
                rc = x_destroy(&p_vtab);
            }
            // Remove o sqlite3_vtab* do arranjo aVTrans[], se aplicável.
            if rc == SQLITE_OK {
                debug_assert!(
                    vtab_info(&p_tab.borrow()).p.as_ref().is_some_and(|x| Rc::ptr_eq(x, &p))
                        && p.borrow().p_next.is_none()
                );
                p.borrow_mut().p_vtab = None;
                vtab_info_mut(&mut p_tab.borrow_mut()).p = None;
                vtab_unlock(&p);
            }
        }
        delete_table(db, Some(p_tab.clone()));
    }

    rc
}

/// Invoca o método `xRollback` ou `xCommit` de cada tabela virtual do arranjo
/// `sqlite3.a_v_trans`. O método chamado é escolhido pelo segundo argumento. O
/// arranjo é esvaziado depois de invocar os callbacks.
fn call_finaliser(db: &Sqlite3Ref, which: VtabFinaliser) {
    let (a_v_trans, n_v_trans) = {
        let mut d = db.borrow_mut();
        (d.a_v_trans.take(), d.n_v_trans)
    };
    if let Some(a_v_trans) = a_v_trans {
        for p_vtab in a_v_trans.iter().take(n_v_trans as usize) {
            let p = p_vtab.borrow().p_vtab.clone();
            if let Some(p) = p {
                let p_module = p.borrow().p_module.clone();
                let x = p_module.and_then(|m| match which {
                    VtabFinaliser::Rollback => m.x_rollback.clone(),
                    VtabFinaliser::Commit => m.x_commit.clone(),
                });
                if let Some(x) = x {
                    let _ = x(&p);
                }
            }
            p_vtab.borrow_mut().i_savepoint = 0;
            vtab_unlock(p_vtab);
        }
        db.borrow_mut().n_v_trans = 0;
    }
}

/// Invoca o método `xSync` de todas as tabelas virtuais do arranjo
/// `sqlite3.a_v_trans`. Devolve o código do primeiro erro, ou `SQLITE_OK` se todos
/// os `xSync` têm êxito.
///
/// Se há mensagem de erro, ela fica em `p.z_err_msg`.
pub fn vtab_sync(db: &Sqlite3Ref, p: &VdbeRef) -> i32 {
    let mut rc = SQLITE_OK;
    let (a_v_trans, n_v_trans) = {
        let mut d = db.borrow_mut();
        (d.a_v_trans.take(), d.n_v_trans)
    };

    if let Some(list) = &a_v_trans {
        for p_vtab_entry in list.iter().take(n_v_trans as usize) {
            if rc != SQLITE_OK {
                break;
            }
            let p_vtab = p_vtab_entry.borrow().p_vtab.clone();
            if let Some(p_vtab) = p_vtab {
                let x_sync = p_vtab.borrow().p_module.as_ref().and_then(|m| m.x_sync.clone());
                if let Some(x_sync) = x_sync {
                    rc = x_sync(&p_vtab);
                    vtab_import_errmsg(p, &p_vtab);
                }
            }
        }
    }
    db.borrow_mut().a_v_trans = a_v_trans;
    rc
}

/// Invoca o método `xRollback` de todas as tabelas virtuais do arranjo
/// `sqlite3.a_v_trans`. Depois esvazia o arranjo.
pub fn vtab_rollback(db: &Sqlite3Ref) -> i32 {
    call_finaliser(db, VtabFinaliser::Rollback);
    SQLITE_OK
}

/// Invoca o método `xCommit` de todas as tabelas virtuais do arranjo
/// `sqlite3.a_v_trans`. Depois esvazia o arranjo.
pub fn vtab_commit(db: &Sqlite3Ref) -> i32 {
    call_finaliser(db, VtabFinaliser::Commit);
    SQLITE_OK
}

/// Se a tabela virtual `p_vtab` suporta a interface de transação
/// (`xBegin`/`xRollback`/`xCommit` e, opcionalmente, `xSync`) e não há transação
/// aberta, invoca o método `xBegin` agora.
///
/// Se a chamada a `xBegin` tem êxito, coloca o `sqlite3_vtab` no arranjo
/// `sqlite3.a_v_trans`.
pub fn vtab_begin(db: &Sqlite3Ref, p_vtab: Option<&VTableRef>) -> i32 {
    let mut rc = SQLITE_OK;

    // Caso especial: se `db.a_v_trans` é nulo e `db.n_v_trans` é maior que zero,
    // esta função está sendo chamada de dentro de um callback `xSync()` de um
    // módulo virtual. Escrever em tabelas de módulo virtual é ilegal nesse caso,
    // então devolve SQLITE_LOCKED.
    if vtab_in_sync(&db.borrow()) {
        return SQLITE_LOCKED;
    }
    let Some(p_vtab) = p_vtab else {
        return SQLITE_OK;
    };
    let Some(p_sqlite_vtab) = p_vtab.borrow().p_vtab.clone() else {
        return SQLITE_OK;
    };
    let p_module = p_sqlite_vtab.borrow().p_module.clone();
    let Some(p_module) = p_module else {
        return SQLITE_OK;
    };

    if let Some(x_begin) = p_module.x_begin.clone() {
        // Se `p_vtab` já está no arranjo aVTrans, volta cedo.
        {
            let d = db.borrow();
            let n = d.n_v_trans as usize;
            if let Some(list) = &d.a_v_trans {
                if list.iter().take(n).any(|x| Rc::ptr_eq(x, p_vtab)) {
                    return SQLITE_OK;
                }
            }
        }

        // Invoca xBegin. Se tem êxito, acrescenta a tabela virtual ao arranjo
        // sqlite3.a_v_trans[].
        rc = grow_v_trans(db);
        if rc == SQLITE_OK {
            rc = x_begin(&p_sqlite_vtab);
            if rc == SQLITE_OK {
                let i_svpt = {
                    let d = db.borrow();
                    d.n_statement + d.n_savepoint
                };
                add_to_v_trans(db, p_vtab);
                if i_svpt != 0 {
                    if let Some(x_savepoint) = p_module.x_savepoint.clone() {
                        p_vtab.borrow_mut().i_savepoint = i_svpt;
                        rc = x_savepoint(&p_sqlite_vtab, i_svpt - 1);
                    }
                }
            }
        }
    }
    rc
}


// ---- part_003.rs ----

/// Invoca o método `xSavepoint`, `xRollbackTo` ou `xRelease` de todas as tabelas
/// virtuais que têm uma transação aberta, passando `i_savepoint` como segundo
/// argumento do método.
///
/// Se `op` é `SAVEPOINT_BEGIN`, invoca `xSavepoint`. Se é `SAVEPOINT_ROLLBACK`,
/// invoca `xRollbackTo`. Caso contrário (`SAVEPOINT_RELEASE`), invoca `xRelease` de
/// cada tabela virtual com transação aberta.
///
/// Se algum método devolve código diferente de `SQLITE_OK`, o processamento é
/// abandonado e o erro é devolvido de imediato ao chamador. Se todas as chamadas
/// têm êxito, devolve `SQLITE_OK`.
pub fn vtab_savepoint(db: &Sqlite3Ref, op: i32, i_savepoint: i32) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!(op == SAVEPOINT_RELEASE || op == SAVEPOINT_ROLLBACK || op == SAVEPOINT_BEGIN);
    debug_assert!(i_savepoint >= -1);
    if db.borrow().a_v_trans.is_some() {
        let mut i = 0usize;
        while rc == SQLITE_OK && (i as i32) < db.borrow().n_v_trans {
            let p_vtab = db.borrow().a_v_trans.as_ref().and_then(|a| a.get(i).cloned());
            i += 1;
            let Some(p_vtab) = p_vtab else {
                break;
            };
            let p_mod = p_vtab.borrow().p_mod.borrow().p_module.clone();
            let p_sqlite_vtab = p_vtab.borrow().p_vtab.clone();
            if let Some(p_sqlite_vtab) = p_sqlite_vtab {
                if p_mod.i_version >= 2 {
                    vtab_lock(&p_vtab);
                    let x_method = match op {
                        SAVEPOINT_BEGIN => {
                            p_vtab.borrow_mut().i_savepoint = i_savepoint + 1;
                            p_mod.x_savepoint.clone()
                        }
                        SAVEPOINT_ROLLBACK => p_mod.x_rollback_to.clone(),
                        _ => p_mod.x_release.clone(),
                    };
                    if let Some(x_method) = x_method {
                        if p_vtab.borrow().i_savepoint > i_savepoint {
                            let saved_flags = db.borrow().flags & SQLITE_DEFENSIVE;
                            db.borrow_mut().flags &= !SQLITE_DEFENSIVE;
                            rc = x_method(&p_sqlite_vtab, i_savepoint);
                            db.borrow_mut().flags |= saved_flags;
                        }
                    }
                    vtab_unlock(&p_vtab);
                }
            }
        }
    }
    rc
}

/// O primeiro parâmetro (`p_def`) é uma implementação de função. O segundo
/// (`p_expr`) é o primeiro argumento dessa função. Se `p_expr` é uma coluna de
/// uma tabela virtual, deixa a implementação da tabela virtual ter a chance de
/// sobrecarregar a função.
///
/// Esta rotina permite que implementações de tabela virtual sobrecarreguem os
/// operadores MATCH, LIKE, GLOB e REGEXP.
///
/// Devolve o argumento `p_def` (sem mudança) ou uma nova estrutura `FuncDef`
/// marcada como efêmera com `SQLITE_FUNC_EPHEM`.
pub fn vtab_overload_function(
    db: &Sqlite3Ref,
    p_def: &FuncDefRef,
    n_arg: i32,
    p_expr: Option<&ExprRef>,
) -> FuncDefRef {
    // Verifica se o operando esquerdo é uma coluna de uma tabela virtual.
    let Some(p_expr) = p_expr else {
        return p_def.clone();
    };
    if p_expr.borrow().op != TK_COLUMN {
        return p_def.clone();
    }
    debug_assert!(expr_use_y_tab(&p_expr.borrow()));
    let Some(p_tab) = p_expr.borrow().y.p_tab.clone() else {
        return p_def.clone();
    };
    if !is_virtual(&p_tab.borrow()) {
        return p_def.clone();
    }
    let p_vtable = get_vtable(db, &p_tab);
    let p_vtab = p_vtable.and_then(|v| {
        let p_vtab = v.borrow().p_vtab.clone();
        p_vtab
    });
    debug_assert!(p_vtab.is_some());
    let Some(p_vtab) = p_vtab else {
        return p_def.clone();
    };
    let p_module = p_vtab.borrow().p_module.clone();
    debug_assert!(p_module.is_some());
    let Some(x_find_function) = p_module.and_then(|m| m.x_find_function.clone()) else {
        return p_def.clone();
    };

    // Chama xFindFunction na implementação da tabela virtual para ver se ela quer
    // sobrecarregar esta função.
    //
    // Embora não documentado, xFindFunction sempre foi invocado com o nome da
    // função todo em minúsculas. Mantém-se a tradição para evitar qualquer chance
    // de incompatibilidade.
    let z_name = p_def.borrow().z_name.clone();
    let mut x_s_func: Option<SFuncFn> = None;
    let mut p_arg: CallbackArg = None;
    let rc = x_find_function(&p_vtab, n_arg, &z_name, &mut x_s_func, &mut p_arg);
    if rc == 0 {
        return p_def.clone();
    }

    // Cria uma definição de função efêmera nova para a função sobrecarregada.
    let mut p_new = p_def.borrow().clone();
    p_new.z_name = z_name;
    p_new.x_s_func = x_s_func;
    p_new.p_user_data = p_arg;
    p_new.func_flags |= SQLITE_FUNC_EPHEM;
    Rc::new(RefCell::new(p_new))
}

/// Garante que a tabela virtual `p_tab` está no arranjo `p_parse.ap_vtab_lock[]`,
/// para que um OP_VBegin seja gerado para ela. Acrescenta `p_tab` ao arranjo se
/// faltar. Se `p_tab` já está no arranjo, não faz nada.
pub fn vtab_make_writable(p_parse: &mut Parse, p_tab: &TableRef) {
    debug_assert!(is_virtual(&p_tab.borrow()));
    with_parse_toplevel(p_parse, |p_toplevel: &mut Parse| {
        if p_toplevel.ap_vtab_lock.iter().any(|t| Rc::ptr_eq(t, p_tab)) {
            return;
        }
        p_toplevel.ap_vtab_lock.push(p_tab.clone());
        p_toplevel.n_vtab_lock = p_toplevel.ap_vtab_lock.len() as i32;
    });
}

/// Verifica se o módulo de tabela virtual `p_mod` pode ter uma instância de tabela
/// virtual epônima. Se pode, cria uma caso ainda não exista. Devolve diferente de
/// zero se a instância epônima existe quando a rotina retorna, ou se a tentativa
/// de criá-la falhou e uma mensagem de erro ficou em `p_parse`.
///
/// Uma instância epônima é a que tem o nome do módulo e, principalmente, não
/// precisa de CREATE VIRTUAL TABLE para existir. Instâncias epônimas sempre
/// existem. Não podem receber DROP.
///
/// Qualquer módulo cujos `xConnect` e `xCreate` são o mesmo método pode ter uma
/// instância epônima.
pub fn vtab_eponymous_table_init(p_parse: &mut Parse, p_mod: &ModuleRef) -> i32 {
    let p_module = p_mod.borrow().p_module.clone();
    let db = p_parse.db.clone();
    if p_mod.borrow().p_epo_tab.is_some() {
        return 1;
    }
    // `xCreate != xConnect`: a identidade das funções é a do `Rc` compartilhado.
    let create_differs = match (&p_module.x_create, &p_module.x_connect) {
        (Some(c), Some(k)) => !Rc::ptr_eq(c, k),
        (Some(_), None) => true,
        _ => false,
    };
    if create_differs {
        return 0;
    }
    let Some(x_connect) = p_module.x_connect.clone() else {
        return 0; // inalcançável: o C exige xConnect no módulo
    };
    let z_name = p_mod.borrow().z_name.clone();
    let mut tab = Table::default();
    tab.z_name = z_name.clone();
    tab.n_tab_ref = 1;
    tab.e_tab_type = TABTYP_VTAB;
    tab.p_schema = db.borrow().a_db[0].p_schema.clone();
    tab.u = TableU::VTab(TableVtab::default());
    tab.i_p_key = -1;
    tab.tab_flags |= TF_EPONYMOUS;
    let p_tab: TableRef = Rc::new(RefCell::new(tab));
    p_mod.borrow_mut().p_epo_tab = Some(p_tab.clone());
    add_module_argument(p_parse, &p_tab, Some(z_name.clone()));
    add_module_argument(p_parse, &p_tab, None);
    add_module_argument(p_parse, &p_tab, Some(z_name));
    let mut z_err: Option<Vec<u8>> = None;
    let rc = vtab_call_constructor(&db, &p_tab, p_mod, &x_connect, &mut z_err);
    if rc != 0 {
        let z_err = z_err.unwrap_or_else(|| b"(null)".to_vec());
        error_msg(p_parse, b"%s", &[FmtArg::Text(&z_err)]);
        vtab_eponymous_table_clear(&db, p_mod);
    }
    1
}

/// Apaga a instância de tabela virtual epônima associada ao módulo `p_mod`, se
/// existir.
pub fn vtab_eponymous_table_clear(db: &Sqlite3Ref, p_mod: &ModuleRef) {
    let p_tab = p_mod.borrow().p_epo_tab.clone();
    if let Some(p_tab) = p_tab {
        // Marca a tabela como efêmera antes de apagá-la, para que delete_table()
        // saiba que ela não está guardada no schema.
        p_tab.borrow_mut().tab_flags |= TF_EPHEMERAL;
        delete_table(db, Some(p_tab));
        p_mod.borrow_mut().p_epo_tab = None;
    }
}

/// Devolve o modo de resolução ON CONFLICT em vigor para a operação de atualização
/// de tabela virtual em andamento.
///
/// Os resultados são indefinidos se não for chamada de dentro de um método
/// `xUpdate`.
pub fn api_vtab_on_conflict(db: &Sqlite3Ref) -> i32 {
    const A_MAP: [i32; 5] = [
        SQLITE_ROLLBACK,
        SQLITE_ABORT,
        SQLITE_FAIL,
        SQLITE_IGNORE,
        SQLITE_REPLACE,
    ];
    debug_assert!(OE_ROLLBACK == 1 && OE_ABORT == 2 && OE_FAIL == 3);
    debug_assert!(OE_IGNORE == 4 && OE_REPLACE == 5);
    let v = db.borrow().vtab_on_conflict as usize;
    debug_assert!((1..=5).contains(&v));
    A_MAP[v - 1]
}

/// Chamada de dentro de `xCreate()` ou `xConnect()` para dar ao núcleo do SQLite
/// informação adicional sobre o comportamento da tabela virtual implementada.
///
/// O C recebe argumentos variádicos; só `SQLITE_VTAB_CONSTRAINT_SUPPORT` lê um
/// inteiro, passado em `arg` (ignorado nas demais operações).
pub fn api_vtab_config(db: &Sqlite3Ref, op: i32, arg: i32) -> i32 {
    let mut rc = SQLITE_OK;

    let mutex = db.borrow().mutex.clone();
    mutex_enter(&mutex);
    let p = db.borrow().p_vtab_ctx.clone();
    match p {
        None => {
            rc = SQLITE_MISUSE_BKPT;
        }
        Some(p) => {
            debug_assert!(is_virtual(&p.borrow().p_tab.borrow()));
            let p_vtable = p.borrow().p_vtable.clone();
            match op {
                SQLITE_VTAB_CONSTRAINT_SUPPORT => {
                    p_vtable.borrow_mut().b_constraint = arg as u8;
                }
                SQLITE_VTAB_INNOCUOUS => {
                    p_vtable.borrow_mut().e_vtab_risk = SQLITE_VTABRISK_LOW;
                }
                SQLITE_VTAB_DIRECTONLY => {
                    p_vtable.borrow_mut().e_vtab_risk = SQLITE_VTABRISK_HIGH;
                }
                SQLITE_VTAB_USES_ALL_SCHEMAS => {
                    p_vtable.borrow_mut().b_all_schemas = 1;
                }
                _ => {
                    rc = SQLITE_MISUSE_BKPT;
                }
            }
        }
    }

    if rc != SQLITE_OK {
        error(db, rc);
    }
    mutex_leave(&mutex);
    rc
}

