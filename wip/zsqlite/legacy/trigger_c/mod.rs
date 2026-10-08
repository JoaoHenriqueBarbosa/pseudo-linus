// Mesclado das partes traduzidas de trigger_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_002.rs ----

// Modelo adotado neste trecho (o integrador precisa casar com os cabeçalhos):
// - `Parse` é compartilhado (`pToplevel`, `NameContext.pParse`), então as funções
//   recebem `&ParseRef` (`Rc<RefCell<Parse>>`) e só seguram `borrow()` em escopos
//   curtos, nunca durante uma chamada que receba o mesmo `ParseRef`.
// - Listas e árvores de dono único (`SrcList`, `ExprList`, `Expr`, `Select`) são
//   `Option<Box<T>>`; os callbacks do `Walker` recebem `&mut` porque mutam o nó.
// - Os códigos `TK_*` são `u8`, como o campo `op` do C.

/// Retorna um trigger existente na tabela pTab que seja capaz de disparar em
/// operações do tipo op (TK_DELETE, TK_INSERT, TK_UPDATE).
/// Se pChanges não for None, o trigger retornado é um trigger UPDATE que será
/// disparado quando qualquer uma das colunas listadas em pChanges for modificada.
///
/// Se pMask não for None, ela é preenchida com uma máscara indicando o tipo de
/// triggers retornados (TRIGGER_BEFORE e/ou TRIGGER_AFTER).
pub fn triggers_exist(
    p_parse: &ParseRef,
    p_tab: &TableRef,
    op: u8,
    p_changes: Option<&ExprList>,
    p_mask: Option<&mut i32>,
) -> Option<TriggerRef> {
    let db = p_parse.borrow().db.upgrade().unwrap();
    let no_trigger = p_tab.borrow().p_trigger.is_none() && !temp_triggers_exist(&db.borrow());
    if no_trigger || p_parse.borrow().disable_triggers != 0 {
        if let Some(mask) = p_mask {
            *mask = 0;
        }
        return None;
    }
    triggers_really_exist(p_parse, p_tab, op, p_changes, p_mask)
}

/// Converte a string pStep->zTarget em uma SrcList e a retorna.
///
/// Esta rotina adiciona um nome de banco específico, quando necessário, ao alvo
/// ao formar a SrcList. Isso impede que um trigger de um banco se refira a um
/// alvo de outro banco. A exceção é quando o trigger está em TEMP, caso em que
/// ele pode se referir a qualquer outro banco.
pub fn trigger_step_src(p_parse: &ParseRef, p_step: &TriggerStep) -> Option<Box<SrcList>> {
    let db = p_parse.borrow().db.upgrade().unwrap();
    let z_name = db_str_dup(&db, p_step.z_target.as_deref());
    let p_src = src_list_append(p_parse, None, None, None);
    assert!(p_src.is_none() || p_src.as_ref().unwrap().n_src == 1);
    assert!(z_name.is_some() || p_src.is_none());
    match p_src {
        Some(mut src) => {
            let p_schema = p_step.p_trig.upgrade().unwrap().borrow().p_schema.clone();
            src.a[0].z_name = z_name;
            if !Rc::ptr_eq(&p_schema, &db.borrow().a_db[1].p_schema) {
                src.a[0].p_schema = Some(p_schema);
            }
            let mut p_src = Some(src);
            if let Some(p_from) = p_step.p_from.as_deref() {
                let mut p_dup = src_list_dup(&db, Some(p_from), 0);
                let nested = p_dup.as_ref().map_or(false, |d| d.n_src > 1);
                if nested && !in_rename_object(&p_parse.borrow()) {
                    let p_subquery = select_new(
                        p_parse, None, p_dup, None, None, None, None, SF_NESTEDFROM, None,
                    );
                    let as_token = Token { n: 0, z: Vec::new() };
                    p_dup = src_list_append_from_term(
                        p_parse, None, None, None, &as_token, p_subquery, None,
                    );
                }
                p_src = src_list_append_list(p_parse, p_src, p_dup);
            }
            p_src
        }
        None => {
            // zName é solto ao sair do escopo (sqlite3DbFree)
            drop(z_name);
            None
        }
    }
}

/// Retorna verdadeiro se o termo pExpr da lista de argumentos da cláusula
/// RETURNING é da forma "*". Dispara um erro se for da forma "table.*".
fn is_asterisk_term(p_parse: &ParseRef, p_term: &Expr) -> bool {
    if p_term.op == TK_ASTERISK {
        return true;
    }
    if p_term.op != TK_DOT {
        return false;
    }
    assert!(p_term.p_right.is_some());
    assert!(p_term.p_left.is_some());
    if p_term.p_right.as_ref().unwrap().op != TK_ASTERISK {
        return false;
    }
    error_msg(p_parse, b"RETURNING may not use \"TABLE.*\" wildcards");
    true
}

/// A lista de entrada pList é a lista de termos de resultado de uma cláusula
/// RETURNING. A tabela da qual estamos retornando é pTab.
///
/// Esta rotina faz uma cópia de pList e, ao mesmo tempo, expande qualquer
/// wildcard "*" para o conjunto completo de colunas de pTab.
fn expand_returning(
    p_parse: &ParseRef,
    p_list: &ExprList,
    p_tab: &TableRef,
) -> Option<Box<ExprList>> {
    let mut p_new: Option<Box<ExprList>> = None;
    let db = p_parse.borrow().db.upgrade().unwrap();

    for i in 0..p_list.a.len() {
        let p_old_expr = match p_list.a[i].p_expr.as_deref() {
            Some(e) => e,
            None => {
                // NEVER(pOldExpr==0)
                continue;
            }
        };
        if is_asterisk_term(p_parse, p_old_expr) {
            let n_col = p_tab.borrow().n_col as usize;
            for jj in 0..n_col {
                let (hidden, z_cn_name) = {
                    let tab = p_tab.borrow();
                    (is_hidden_column(&tab.a_col[jj]), tab.a_col[jj].z_cn_name.clone())
                };
                if hidden {
                    continue;
                }
                let p_new_expr = expr(&db, TK_ID, z_cn_name.as_deref());
                p_new = expr_list_append(p_parse, p_new, p_new_expr);
                if db.borrow().malloc_failed == 0 {
                    let p_item = p_new.as_mut().unwrap().a.last_mut().unwrap();
                    p_item.z_e_name = db_str_dup(&db, z_cn_name.as_deref());
                    p_item.fg.e_e_name = ENAME_NAME;
                }
            }
        } else {
            let p_new_expr = expr_dup(&db, Some(p_old_expr), 0);
            p_new = expr_list_append(p_parse, p_new, p_new_expr);
            if db.borrow().malloc_failed == 0 && always(p_list.a[i].z_e_name.is_some()) {
                let p_item = p_new.as_mut().unwrap().a.last_mut().unwrap();
                p_item.z_e_name = db_str_dup(&db, p_list.a[i].z_e_name.as_deref());
                p_item.fg.e_e_name = p_list.a[i].fg.e_e_name;
            }
        }
    }
    p_new
}

/// Se o nó Expr é uma subconsulta, um operador EXISTS ou um operador IN que usa
/// uma subconsulta, e se a subconsulta é SF_Correlated, marca a expressão como
/// EP_VarSelect.
fn returning_subquery_var_select(_not_used: &mut Walker, p_expr: &mut Expr) -> i32 {
    if expr_use_x_select(p_expr)
        && p_expr
            .x
            .p_select
            .as_ref()
            .map_or(false, |s| (s.sel_flags & SF_CORRELATED) != 0)
    {
        expr_set_property(p_expr, EP_VAR_SELECT);
    }
    WRC_CONTINUE
}

/// Se o SELECT referencia a tabela pWalker->u.pTab, faz duas coisas:
///
///    (1) Marca o SELECT como SF_Correlated.
///    (2) Define pWalker->eCode como não zero, para que o chamador saiba
///        que (1) aconteceu.
fn returning_subquery_correlated(p_walker: &mut Walker, p_select: &mut Select) -> i32 {
    let mut hit = false;
    {
        let p_src = p_select.p_src.as_ref().unwrap();
        for i in 0..p_src.n_src as usize {
            let same = match (&p_src.a[i].p_tab, &p_walker.u.p_tab) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if same {
                hit = true;
                break;
            }
        }
    }
    if hit {
        p_select.sel_flags |= SF_CORRELATED;
        p_walker.e_code = 1;
    }
    WRC_CONTINUE
}

/// Varre a lista de expressões que é o argumento de RETURNING procurando
/// subconsultas que dependem da tabela modificada na instrução que hospeda a
/// cláusula RETURNING (pTab). Marca todas essas subconsultas como SF_Correlated.
/// Se as subconsultas fazem parte de uma expressão, marca a expressão como
/// EP_VarSelect.
///
/// https://sqlite.org/forum/forumpost/2c83569ce8945d39
fn process_returning_subqueries(p_e_list: &mut ExprList, p_tab: &TableRef) {
    let mut w = Walker::default();
    w.x_expr_callback = Some(expr_walk_noop);
    w.x_select_callback = Some(returning_subquery_correlated);
    w.u.p_tab = Some(p_tab.clone());
    walk_expr_list(&mut w, Some(&mut *p_e_list));
    if w.e_code != 0 {
        w.x_expr_callback = Some(returning_subquery_var_select);
        w.x_select_callback = Some(select_walk_noop);
        walk_expr_list(&mut w, Some(&mut *p_e_list));
    }
}

/// Gera código para o trigger RETURNING. Diferentemente dos outros triggers, que
/// invocam um subprograma no bytecode, o código de RETURNING é gerado in-line.
fn code_returning_trigger(
    p_parse: &ParseRef,
    p_trigger: &TriggerRef,
    p_tab: &TableRef,
    reg_in: i32,
) {
    let v = p_parse.borrow().p_vdbe.clone().unwrap();
    let db = p_parse.borrow().db.upgrade().unwrap();

    if p_parse.borrow().b_returning == 0 {
        // Este trigger RETURNING é de outra instrução, pois esta não tem
        // cláusula RETURNING.
        return;
    }
    assert!(db
        .borrow()
        .p_parse
        .as_ref()
        .and_then(|w| w.upgrade())
        .map_or(false, |r| Rc::ptr_eq(&r, p_parse)));
    let p_returning = p_parse.borrow().u1_p_returning.clone().unwrap();
    if !Rc::ptr_eq(&p_returning.borrow().ret_trig, p_trigger) {
        // Este trigger RETURNING é de outra instrução
        return;
    }

    let mut s_select = Select::default();
    let s_from = SrcList {
        n_src: 1,
        a: vec![SrcItem {
            p_tab: Some(p_tab.clone()),
            // tag-20240424-1: o C aponta para o mesmo zName da tabela
            z_name: p_tab.borrow().z_name.clone(),
            i_cursor: -1,
            ..Default::default()
        }],
        ..Default::default()
    };
    s_select.p_e_list = expr_list_dup(&db, p_returning.borrow().p_return_el.as_deref(), 0);
    s_select.p_src = Some(Box::new(s_from));
    select_prep(p_parse, &mut s_select, None);
    if p_parse.borrow().n_err == 0 {
        assert!(db.borrow().malloc_failed == 0);
        generate_column_names(p_parse, &mut s_select);
    }
    drop(s_select.p_e_list.take());
    let mut p_new = {
        let ret = p_returning.borrow();
        expand_returning(p_parse, ret.p_return_el.as_deref().unwrap(), p_tab)
    };
    if p_parse.borrow().n_err == 0 {
        let mut s_nc = NameContext::default();
        if p_returning.borrow().n_ret_col == 0 {
            let n_expr = p_new.as_ref().unwrap().a.len() as i32;
            let mut pp = p_parse.borrow_mut();
            let mut ret = p_returning.borrow_mut();
            ret.n_ret_col = n_expr;
            ret.i_ret_cur = pp.n_tab;
            pp.n_tab += 1;
        }
        s_nc.p_parse = Some(p_parse.clone());
        s_nc.u_nc.i_base_reg = reg_in;
        s_nc.nc_flags = NC_UBASEREG;
        {
            let mut pp = p_parse.borrow_mut();
            pp.e_trigger_op = p_trigger.borrow().op;
            pp.p_trigger_tab = Some(p_tab.clone());
        }
        if resolve_expr_list_names(&mut s_nc, p_new.as_deref_mut()) == SQLITE_OK
            && always(db.borrow().malloc_failed == 0)
        {
            let n_col = p_new.as_ref().unwrap().a.len() as i32;
            let reg = p_parse.borrow().n_mem + 1;
            process_returning_subqueries(p_new.as_mut().unwrap(), p_tab);
            p_parse.borrow_mut().n_mem += n_col + 2;
            p_returning.borrow_mut().i_ret_reg = reg;
            for i in 0..n_col {
                let p_col = &mut p_new.as_mut().unwrap().a[i as usize].p_expr;
                assert!(p_col.is_some()); // por causa de !db->mallocFailed
                expr_code_factorable(p_parse, p_col.as_deref_mut(), reg + i);
                if expr_affinity(p_col.as_deref()) == SQLITE_AFF_REAL {
                    vdbe_add_op1(&mut v.borrow_mut(), OP_REALAFFINITY as i32, reg + i);
                }
            }
            let i_ret_cur = p_returning.borrow().i_ret_cur;
            vdbe_add_op3(&mut v.borrow_mut(), OP_MAKERECORD as i32, reg, n_col, reg + n_col);
            vdbe_add_op2(&mut v.borrow_mut(), OP_NEWROWID as i32, i_ret_cur, reg + n_col + 1);
            vdbe_add_op3(
                &mut v.borrow_mut(),
                OP_INSERT as i32,
                i_ret_cur,
                reg + n_col,
                reg + n_col + 1,
            );
        }
    }
    drop(p_new);
    let mut pp = p_parse.borrow_mut();
    pp.e_trigger_op = 0;
    pp.p_trigger_tab = None;
}

/// Gera código VDBE para as instruções dentro do corpo de um único trigger.
fn code_trigger_program(
    p_parse: &ParseRef,
    p_step_list: Option<&TriggerStep>,
    orconf: i32,
) -> i32 {
    let v = p_parse.borrow().p_vdbe.clone().unwrap();
    let db = p_parse.borrow().db.upgrade().unwrap();

    assert!(p_parse.borrow().p_trigger_tab.is_some() && p_parse.borrow().p_toplevel.is_some());
    assert!(p_step_list.is_some());

    let mut p_step = p_step_list;
    while let Some(step) = p_step {
        // Descobre a política ON CONFLICT usada neste passo do programa. Se a
        // instrução que disparou o trigger tinha um ON CONFLICT explícito, ele
        // vale; senão vale a política especificada no passo do trigger.
        let e_orconf: u8 = if orconf == OE_DEFAULT as i32 { step.orconf } else { orconf as u8 };
        p_parse.borrow_mut().e_orconf = e_orconf;
        assert!(p_parse.borrow().ok_const_factor == 0);

        if let Some(z_span) = step.z_span.as_ref() {
            let mut msg = b"-- ".to_vec();
            msg.extend_from_slice(z_span);
            vdbe_add_op4(
                &mut v.borrow_mut(),
                OP_TRACE as i32,
                0x7fffffff,
                1,
                0,
                P4::Dynamic(msg),
            );
        }

        match step.op {
            TK_UPDATE => {
                let p_src = trigger_step_src(p_parse, step);
                update(
                    p_parse,
                    p_src,
                    expr_list_dup(&db, step.p_expr_list.as_deref(), 0),
                    expr_dup(&db, step.p_where.as_deref(), 0),
                    e_orconf,
                    None,
                    None,
                    None,
                );
                vdbe_add_op0(&mut v.borrow_mut(), OP_RESETCOUNT as i32);
            }
            TK_INSERT => {
                let p_src = trigger_step_src(p_parse, step);
                insert(
                    p_parse,
                    p_src,
                    select_dup(&db, step.p_select.as_deref(), 0),
                    id_list_dup(&db, step.p_id_list.as_deref()),
                    e_orconf,
                    upsert_dup(&db, step.p_upsert.as_deref()),
                );
                vdbe_add_op0(&mut v.borrow_mut(), OP_RESETCOUNT as i32);
            }
            TK_DELETE => {
                let p_src = trigger_step_src(p_parse, step);
                delete_from(
                    p_parse,
                    p_src,
                    expr_dup(&db, step.p_where.as_deref(), 0),
                    None,
                    None,
                );
                vdbe_add_op0(&mut v.borrow_mut(), OP_RESETCOUNT as i32);
            }
            _ => {
                assert!(step.op == TK_SELECT);
                let mut s_dest = SelectDest::default();
                let mut p_select = select_dup(&db, step.p_select.as_deref(), 0);
                select_dest_init(&mut s_dest, SRT_DISCARD, 0);
                select(p_parse, p_select.as_deref_mut(), &mut s_dest);
                select_delete(&db, p_select);
            }
        }
        p_step = step.p_next.as_deref();
    }
    0
}


// ---- part_003.rs ----

// Modelo adotado neste trecho: `Parse` compartilhado (`&ParseRef`), `TriggerPrg`
// compartilhado entre a lista `Parse.pTriggerPrg` e o retorno (`TriggerPrgRef`,
// `Rc<RefCell<TriggerPrg>>`, com `p_next: Option<TriggerPrgRef>`), `SubProgram`
// atrás de `Rc<RefCell<_>>`. Os códigos `TK_*` e `TRIGGER_*` são `u8`.
//
// Observações de conformidade:
// - `onErrorText` e todos os `VdbeComment()` só existem sob
//   `SQLITE_ENABLE_EXPLAIN_COMMENTS`, que não está nas opções do Debian 13,
//   então somem (o `VdbeComment` expande para nada).
// - `codeRowTrigger` (static) vira `code_row_trigger_program` porque o nome
//   `code_row_trigger` já é de `sqlite3CodeRowTrigger` (API do mesmo módulo).

/// O contexto de análise pFrom acabou de ser usado para criar um sub-vdbe
/// (programa de trigger). Se ocorreu um erro, transfere a informação de erro de
/// pFrom para pTo.
fn transfer_parse_error(p_to: &ParseRef, p_from: &ParseRef) {
    let mut to = p_to.borrow_mut();
    let mut from = p_from.borrow_mut();
    assert!(from.z_err_msg.is_none() || from.n_err != 0);
    assert!(to.z_err_msg.is_none() || to.n_err != 0);
    if to.n_err == 0 {
        to.z_err_msg = from.z_err_msg.take();
        to.n_err = from.n_err;
        to.rc = from.rc;
    } else {
        // sqlite3DbFree(pFrom->db, pFrom->zErrMsg)
        drop(from.z_err_msg.take());
    }
}

/// Cria e popula um novo objeto TriggerPrg com um subprograma que implementa o
/// trigger pTrigger com a política ON CONFLICT orconf.
fn code_row_trigger_program(
    p_parse: &ParseRef,
    p_trigger: &TriggerRef,
    p_tab: &TableRef,
    orconf: i32,
) -> Option<TriggerPrgRef> {
    let p_top = parse_toplevel(p_parse);
    let db = p_parse.borrow().db.upgrade().unwrap();
    let mut i_end_trigger: i32 = 0; // Rótulo para onde pular se WHEN for falso

    assert!(
        p_trigger.borrow().z_name.is_none()
            || Rc::ptr_eq(p_tab, &table_of_trigger(&p_trigger.borrow()))
    );
    assert!(p_top.borrow().p_vdbe.is_some());

    // Aloca os objetos TriggerPrg e SubProgram. Para garantir que sejam liberados
    // se ocorrer um erro, liga-os à lista Parse.pTriggerPrg do Parse de nível
    // superior o quanto antes. (Falha de alocação não existe aqui.)
    let p_program: SubProgramRef = Rc::new(RefCell::new(SubProgram::default()));
    let p_prg: TriggerPrgRef = Rc::new(RefCell::new(TriggerPrg::default()));
    {
        let mut top = p_top.borrow_mut();
        p_prg.borrow_mut().p_next = top.p_trigger_prg.take();
        top.p_trigger_prg = Some(p_prg.clone());
    }
    p_prg.borrow_mut().p_program = Some(p_program.clone());
    {
        let top_vdbe = p_top.borrow().p_vdbe.clone().unwrap();
        vdbe_link_sub_program(&mut top_vdbe.borrow_mut(), &p_program);
    }
    {
        let mut prg = p_prg.borrow_mut();
        prg.p_trigger = Some(p_trigger.clone());
        prg.orconf = orconf;
        prg.a_colmask[0] = 0xffffffff;
        prg.a_colmask[1] = 0xffffffff;
    }

    // Aloca e popula um novo contexto Parse para codificar o subprograma do
    // trigger.
    let s_sub_parse: ParseRef = Rc::new(RefCell::new(Parse::default()));
    parse_object_init(&mut s_sub_parse.borrow_mut(), &db);
    let mut s_nc = NameContext::default();
    s_nc.p_parse = Some(s_sub_parse.clone());
    {
        let trig = p_trigger.borrow();
        let pp = p_parse.borrow();
        let mut sp = s_sub_parse.borrow_mut();
        sp.p_trigger_tab = Some(p_tab.clone());
        sp.p_toplevel = Some(p_top.clone());
        sp.z_auth_context = trig.z_name.clone();
        sp.e_trigger_op = trig.op;
        sp.n_query_loop = pp.n_query_loop;
        sp.prep_flags = pp.prep_flags;
    }

    let v = get_vdbe(&s_sub_parse);
    if let Some(v) = v {
        if let Some(z_name) = p_trigger.borrow().z_name.as_ref() {
            let mut msg = b"-- TRIGGER ".to_vec();
            msg.extend_from_slice(z_name);
            vdbe_change_p4(&mut v.borrow_mut(), -1, P4::Dynamic(msg));
        }

        // Se foi especificada, codifica a cláusula WHEN. Se der falso (ou NULL),
        // o sub-vdbe é parado de imediato pulando para o OP_Halt inserido no fim
        // do programa.
        let has_when = p_trigger.borrow().p_when.is_some();
        if has_when {
            let mut p_when = expr_dup(&db, p_trigger.borrow().p_when.as_deref(), 0);
            if db.borrow().malloc_failed == 0
                && SQLITE_OK == resolve_expr_names(&mut s_nc, p_when.as_deref_mut())
            {
                i_end_trigger = vdbe_make_label(&s_sub_parse);
                expr_if_false(&s_sub_parse, p_when.as_deref_mut(), i_end_trigger, SQLITE_JUMPIFNULL);
            }
            expr_delete(&db, p_when);
        }

        // Codifica o programa do trigger no sub-vdbe.
        code_trigger_program(&s_sub_parse, p_trigger.borrow().step_list.as_deref(), orconf);

        // Insere um OP_Halt no fim do subprograma.
        if i_end_trigger != 0 {
            vdbe_resolve_label(&mut v.borrow_mut(), i_end_trigger);
        }
        vdbe_add_op0(&mut v.borrow_mut(), OP_HALT as i32);
        transfer_parse_error(p_parse, &s_sub_parse);

        if p_parse.borrow().n_err == 0 {
            assert!(db.borrow().malloc_failed == 0);
            let mut n_op: i32 = 0;
            let a_op = vdbe_take_op_array(
                &mut v.borrow_mut(),
                &mut n_op,
                &mut p_top.borrow_mut().n_max_arg,
            );
            let mut prog = p_program.borrow_mut();
            prog.a_op = a_op;
            prog.n_op = n_op;
        }
        {
            let sp = s_sub_parse.borrow();
            let mut prog = p_program.borrow_mut();
            prog.n_mem = sp.n_mem;
            prog.n_csr = sp.n_tab;
            // pProgram->token = (void*)pTrigger: guarda só a identidade do trigger
            prog.token = Rc::as_ptr(p_trigger) as *const () as usize;
            let mut prg = p_prg.borrow_mut();
            prg.a_colmask[0] = sp.oldmask;
            prg.a_colmask[1] = sp.newmask;
        }
        vdbe_delete(v);
    } else {
        transfer_parse_error(p_parse, &s_sub_parse);
    }

    assert!(s_sub_parse.borrow().p_trigger_prg.is_none() && s_sub_parse.borrow().n_max_arg == 0);
    parse_object_reset(&mut s_sub_parse.borrow_mut());
    Some(p_prg)
}

/// Retorna o objeto TriggerPrg com o subprograma do trigger pTrigger com o
/// algoritmo ON CONFLICT padrão orconf. Se esse objeto não existe, um novo é
/// alocado e populado antes de ser retornado.
fn get_row_trigger(
    p_parse: &ParseRef,
    p_trigger: &TriggerRef,
    p_tab: &TableRef,
    orconf: i32,
) -> Option<TriggerPrgRef> {
    let p_root = parse_toplevel(p_parse);

    assert!(
        p_trigger.borrow().z_name.is_none()
            || Rc::ptr_eq(p_tab, &table_of_trigger(&p_trigger.borrow()))
    );

    // Pode ser que este trigger já tenha sido codificado (ou esteja sendo). Se
    // sim, há uma entrada com o campo pTrigger coincidente na lista
    // Parse.pTriggerPrg. Procura essa entrada.
    let mut p_prg = p_root.borrow().p_trigger_prg.clone();
    while let Some(prg) = p_prg {
        let found = {
            let b = prg.borrow();
            b.p_trigger.as_ref().map_or(false, |t| Rc::ptr_eq(t, p_trigger)) && b.orconf == orconf
        };
        if found {
            return Some(prg);
        }
        let next = prg.borrow().p_next.clone();
        p_prg = next;
    }

    // Se não achou um TriggerPrg existente, cria um novo.
    let p_prg = code_row_trigger_program(p_parse, p_trigger, p_tab, orconf);
    let db = p_parse.borrow().db.upgrade().unwrap();
    db.borrow_mut().err_byte_offset = -1;
    p_prg
}

/// Gera código para o programa de trigger associado ao trigger p na tabela pTab.
/// Os parâmetros reg, orconf e ignoreJump são os mesmos descritos no cabeçalho
/// de sqlite3CodeRowTrigger().
pub fn code_row_trigger_direct(
    p_parse: &ParseRef,
    p: &TriggerRef,
    p_tab: &TableRef,
    reg: i32,
    orconf: i32,
    ignore_jump: i32,
) {
    let v = get_vdbe(p_parse);
    let p_prg = get_row_trigger(p_parse, p, p_tab, orconf);
    assert!(p_prg.is_some() || p_parse.borrow().n_err != 0);

    // Codifica o opcode OP_Program no VDBE pai. O P4 de OP_Program é o sub-vdbe
    // que contém o programa do trigger.
    if let Some(p_prg) = p_prg {
        let v = v.unwrap();
        let db = p_parse.borrow().db.upgrade().unwrap();
        let b_recursive =
            p.borrow().z_name.is_some() && 0 == (db.borrow().flags & SQLITE_RECTRIGGERS);

        let i_mem = {
            let mut pp = p_parse.borrow_mut();
            pp.n_mem += 1;
            pp.n_mem
        };
        let p_program = p_prg.borrow().p_program.clone().unwrap();
        vdbe_add_op4(
            &mut v.borrow_mut(),
            OP_PROGRAM as i32,
            reg,
            ignore_jump,
            i_mem,
            P4::SubProgram(p_program),
        );

        // Define o operando P5 de OP_Program como não zero se a invocação
        // recursiva deste programa é proibida. Ela é proibida se (a) o
        // subprograma é de fato um trigger, e não uma ação de chave estrangeira,
        // e (b) a flag que habilita triggers recursivos está desligada.
        vdbe_change_p5(&mut v.borrow_mut(), b_recursive as u8);
    }
}

/// Chamada para codificar os triggers FOR EACH ROW necessários para uma operação
/// na tabela pTab. A operação (INSERT, UPDATE ou DELETE) é dada por op. O
/// parâmetro tr_tm determina se os triggers BEFORE ou AFTER são codificados. Se
/// a operação é um UPDATE, pChanges recebe a lista de colunas modificadas.
///
/// Se não há triggers que disparem no momento especificado para a operação
/// especificada em pTab, a função não faz nada.
///
/// O argumento reg é o endereço do primeiro de um array de registradores que
/// contém os valores substituídos nas referências new.* e old.* do programa do
/// trigger. Se N é o número de colunas de pTab (cópia de pTab->nCol):
///
///   reg+0          OLD.rowid
///   reg+1          OLD.* da coluna mais à esquerda de pTab
///   ...
///   reg+N          OLD.* da coluna mais à direita de pTab
///   reg+N+1        NEW.rowid
///   reg+N+2        NEW.* da coluna mais à esquerda de pTab
///   ...
///   reg+N+N+1      NEW.* da coluna mais à direita de pTab
///
/// Em triggers ON DELETE os registradores NEW.* nunca são acessados e não são
/// alocados nem populados pelo chamador. Do mesmo modo, em ON INSERT os
/// registradores OLD.* nunca são acessados; assim, para ON INSERT o valor de reg
/// não é um registrador legível, embora (reg+N) a (reg+N+N+1) sejam.
///
/// O parâmetro orconf é o algoritmo de resolução de conflito padrão do programa
/// do trigger (REPLACE, IGNORE etc.). O parâmetro ignoreJump é a instrução para
/// onde o controle salta se o programa do trigger levanta uma exceção IGNORE.
pub fn code_row_trigger(
    p_parse: &ParseRef,
    p_trigger: &Option<TriggerRef>,
    op: u8,
    p_changes: Option<&ExprList>,
    tr_tm: u8,
    p_tab: &TableRef,
    reg: i32,
    orconf: i32,
    ignore_jump: i32,
) {
    assert!(op == TK_UPDATE || op == TK_INSERT || op == TK_DELETE);
    assert!(tr_tm == TRIGGER_BEFORE || tr_tm == TRIGGER_AFTER);
    assert!((op == TK_UPDATE) == p_changes.is_some());

    let mut p_opt = p_trigger.clone();
    while let Some(p) = p_opt {
        // Verificação: o schema do trigger e o da tabela sempre existem. O
        // trigger tem de estar no mesmo schema da tabela ou ser um trigger TEMP.
        #[cfg(debug_assertions)]
        {
            let db = p_parse.borrow().db.upgrade().unwrap();
            let b = p.borrow();
            assert!(
                Rc::ptr_eq(&b.p_schema, &b.p_tab_schema)
                    || Rc::ptr_eq(&b.p_schema, &db.borrow().a_db[1].p_schema)
            );
        }

        // Decide se este trigger deve ser codificado. Uma de duas escolhas:
        //   1. O trigger casa exatamente com a instrução DML atual
        //   2. É um trigger RETURNING de INSERT, mas estamos na parte UPDATE
        //      de um UPSERT.
        let (should_code, b_returning) = {
            let b = p.borrow();
            let c = (b.op == op || (b.b_returning != 0 && b.op == TK_INSERT && op == TK_UPDATE))
                && b.tr_tm == tr_tm
                && check_column_overlap(b.p_columns.as_deref(), p_changes);
            (c, b.b_returning != 0)
        };
        if should_code {
            if !b_returning {
                code_row_trigger_direct(p_parse, &p, p_tab, reg, orconf, ignore_jump);
            } else {
                let toplevel = is_toplevel(&p_parse.borrow());
                if toplevel {
                    code_returning_trigger(p_parse, &p, p_tab, reg);
                }
            }
        }
        let p_next = p.borrow().p_next.clone();
        p_opt = p_next;
    }
}

/// Triggers podem acessar valores guardados nas pseudotabelas old.* ou new.*.
/// Esta função retorna uma máscara de 32 bits que indica quais colunas das
/// tabelas old.* ou new.* são de fato usadas pelos triggers. O chamador pode
/// usar isso, por exemplo, para evitar carregar todo o registro old.* na memória
/// ao executar um UPDATE ou DELETE.
///
/// O bit 0 da máscara é definido se a coluna mais à esquerda pode ser acessada
/// por uma referência [old|new].<col>. O bit 1, se o valor da segunda coluna é
/// necessário, e assim por diante. Se há mais de 32 colunas e pelo menos uma das
/// colunas de índice maior que 32 pode ser acessada, retorna 0xffffffff.
///
/// Não é possível saber se old.rowid ou new.rowid é acessado pelos triggers. O
/// chamador deve sempre assumir que é.
///
/// O parâmetro isNew é 1 ou 0. Se 0, a máscara vale para old.*; se 1, para new.*.
///
/// O parâmetro tr_tm é uma máscara com um ou ambos os bits TRIGGER_BEFORE e
/// TRIGGER_AFTER. Valores acessados por triggers BEFORE só entram na máscara se
/// o bit TRIGGER_BEFORE está em tr_tm; igualmente para AFTER.
pub fn trigger_colmask(
    p_parse: &ParseRef,
    p_trigger: &Option<TriggerRef>,
    p_changes: Option<&ExprList>,
    is_new: i32,
    tr_tm: u8,
    p_tab: &TableRef,
    orconf: i32,
) -> u32 {
    let op: u8 = if p_changes.is_some() { TK_UPDATE } else { TK_DELETE };
    let mut mask: u32 = 0;

    assert!(is_new == 1 || is_new == 0);
    if is_view(&p_tab.borrow()) {
        return 0xffffffff;
    }
    let mut p_opt = p_trigger.clone();
    while let Some(p) = p_opt {
        let (matches, b_returning) = {
            let b = p.borrow();
            let m = b.op == op
                && (tr_tm & b.tr_tm) != 0
                && check_column_overlap(b.p_columns.as_deref(), p_changes);
            (m, b.b_returning != 0)
        };
        if matches {
            if b_returning {
                mask = 0xffffffff;
            } else if let Some(p_prg) = get_row_trigger(p_parse, &p, p_tab, orconf) {
                mask |= p_prg.borrow().a_colmask[is_new as usize];
            }
        }
        let p_next = p.borrow().p_next.clone();
        p_opt = p_next;
    }
    mask
}


// ---- part_004.rs ----

