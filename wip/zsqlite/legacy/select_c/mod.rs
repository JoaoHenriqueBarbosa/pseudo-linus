// Mesclado das partes traduzidas de select_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Registra como processar a palavra-chave DISTINCT, para simplificar a passagem dessa informação
/// à rotina `select_inner_loop()`.
#[derive(Default)]
pub struct DistinctCtx {
    /// 0: não é distinct. 1: DISTINCT. 2: DISTINCT e ORDER BY.
    pub is_tnct: u8,
    /// Um dos operadores WHERE_DISTINCT_*.
    pub e_tnct_type: u8,
    /// Tabela efêmera usada no processamento do DISTINCT.
    pub tab_tnct: i32,
    /// Endereço do opcode OP_OpenEphemeral de `tab_tnct`.
    pub addr_tnct: i32,
}

/// Registra informações sobre a cláusula ORDER BY (ou GROUP BY) da consulta que está sendo
/// codificada.
///
/// O vetor `aDefer[]` da otimização de referências do sorter só existe sob
/// SQLITE_ENABLE_SORTER_REFERENCES, que o Debian não liga; o campo some, assim como
/// `addrPush`/`addrPushEnd` (SQLITE_ENABLE_STMT_SCANSTATUS).
#[derive(Default)]
pub struct SortCtx {
    /// A cláusula ORDER BY (ou GROUP BY).
    pub p_order_by: Option<Box<ExprList>>,
    /// Número de termos do ORDER BY satisfeitos por índices.
    pub n_ob_sat: i32,
    /// Número do cursor do sorter.
    pub i_e_cursor: i32,
    /// Registrador com o endereço de retorno da saída em bloco.
    pub reg_return: i32,
    /// Rótulo inicial da subrotina de saída em bloco.
    pub label_bk_out: i32,
    /// Endereço do OP_SorterOpen ou OP_OpenEphemeral.
    pub addr_sort_index: i32,
    /// Salta para cá ao terminar, por exemplo quando o LIMIT é atingido.
    pub label_done: i32,
    /// Salta para cá quando o sorter está cheio.
    pub label_ob_lopt: i32,
    /// Zero ou mais bits SORTFLAG_*.
    pub sort_flags: u8,
    /// Informação de carga adiada de linhas, ou nenhuma.
    pub p_deferred_row_load: Option<Box<RowLoadInfo>>,
}

/// Usa SorterOpen em vez de OpenEphemeral.
pub const SORTFLAG_USESORTER: u8 = 0x01;

/// Apaga todo o conteúdo de uma estrutura Select. Em C a estrutura em si é liberada conforme
/// `b_free`; aqui o dono do `Box` a libera ao sair de escopo, e `b_free` só é mantido pela
/// fidelidade da assinatura (se for 0, o primeiro Select fica com os campos vazios mas vivo).
/// A cadeia `p_prior` é sempre consumida.
pub fn clear_select(db: &Sqlite3Ref, p: &mut Select, b_free: i32) {
    let _ = b_free;
    let mut p_prior = p.p_prior.take();
    clear_select_fields(db, p);
    while let Some(mut cur) = p_prior {
        p_prior = cur.p_prior.take();
        clear_select_fields(db, &mut cur);
        // `cur` é liberado aqui (bFree passa a 1 depois da primeira volta).
    }
}

/// Esvazia os campos de um único Select, sem tocar na cadeia `p_prior`.
fn clear_select_fields(db: &Sqlite3Ref, p: &mut Select) {
    expr_list_delete(db, p.p_elist.take());
    src_list_delete(db, p.p_src.take());
    expr_delete(db, p.p_where.take());
    expr_list_delete(db, p.p_group_by.take());
    expr_delete(db, p.p_having.take());
    expr_list_delete(db, p.p_order_by.take());
    expr_delete(db, p.p_limit.take());
    if ok_if_always_true(p.p_with.is_some()) {
        with_delete(db, p.p_with.take());
    }
    if ok_if_always_true(p.p_win_defn.is_some()) {
        window_list_delete(db, p.p_win_defn.take());
    }
    while let Some(w) = p.p_win.clone() {
        window_unlink_from_select(&mut p.p_win, &w);
    }
}

/// Inicializa uma estrutura SelectDest.
pub fn select_dest_init(p_dest: &mut SelectDest, e_dest: i32, i_parm: i32) {
    p_dest.e_dest = e_dest as u8;
    p_dest.i_sdparm = i_parm;
    p_dest.i_sdparm2 = 0;
    p_dest.z_aff_sdst = Vec::new();
    p_dest.i_sdst = 0;
    p_dest.n_sdst = 0;
}

/// Aloca uma nova estrutura Select e a devolve. Devolve `None` se faltar memória (nesse caso
/// tudo o que foi recebido já foi liberado).
pub fn select_new(
    p_parse: &mut Parse,
    p_e_list: Option<Box<ExprList>>,
    p_src: Option<Box<SrcList>>,
    p_where: Option<Box<Expr>>,
    p_group_by: Option<Box<ExprList>>,
    p_having: Option<Box<Expr>>,
    p_order_by: Option<Box<ExprList>>,
    sel_flags: u32,
    p_limit: Option<Box<Expr>>,
) -> Option<Box<Select>> {
    let db = p_parse
        .db
        .upgrade()
        .expect("a conexão precisa estar viva enquanto o Parse existe");
    let mut p_e_list = p_e_list;
    if p_e_list.is_none() {
        let p_star = expr(&db.borrow(), TK_ASTERISK as i32, None);
        p_e_list = expr_list_append(p_parse, None, p_star);
    }
    p_parse.n_select += 1;
    let mut p_src = p_src;
    if p_src.is_none() {
        // sqlite3DbMallocZero(sizeof(*pSrc)): SrcList zerado, n_src == 0.
        p_src = Some(Box::new(SrcList::default()));
    }
    let p_new = Box::new(Select {
        p_elist: p_e_list,
        op: TK_SELECT as u8,
        sel_flags,
        i_limit: 0,
        i_offset: 0,
        sel_id: p_parse.n_select as u32,
        addr_open_ephm: [-1, -1],
        n_select_row: 0,
        p_src,
        p_where,
        p_group_by,
        p_having,
        p_order_by,
        p_prior: None,
        p_next: None,
        p_limit,
        p_with: None,
        p_win: None,
        p_win_defn: None,
    });
    let mut p_new = p_new;
    if db.borrow().malloc_failed != 0 {
        clear_select(&db, &mut p_new, 1);
        None
    } else {
        debug_assert!(p_new.p_src.is_some() || p_parse.n_err > 0);
        Some(p_new)
    }
}

/// Apaga a estrutura Select dada e todas as suas subestruturas.
pub fn select_delete(db: &Sqlite3Ref, p: Option<Box<Select>>) {
    if let Some(mut s) = p {
        clear_select(db, &mut s, 1);
    }
}

/// Variante genérica de `select_delete()`, usada como destrutor registrado.
pub fn select_delete_generic(db: &Sqlite3Ref, p: Option<Box<Select>>) {
    if let Some(mut s) = p {
        clear_select(db, &mut s, 1);
    }
}

/// Devolve o SELECT mais à direita de um composto, seguindo `p_next`.
pub fn find_rightmost(p: &mut Select) -> &mut Select {
    let mut cur = p;
    while cur.p_next.is_some() {
        cur = cur.p_next.as_mut().unwrap();
    }
    cur
}

/// Dados 1 a 3 identificadores antes da palavra JOIN, determina o tipo do join e o devolve como
/// máscara de bits (JT_INNER, JT_CROSS, JT_OUTER, JT_NATURAL, JT_LEFT, JT_RIGHT). Um full outer
/// join é JT_LEFT|JT_RIGHT. Se o tipo é ilegal ou não suportado, ainda devolve um tipo mas
/// registra um erro em `p_parse`.
///
/// Combinações válidas (pA pB pC -> valor):
///
///   CROSS                  -> JT_CROSS
///   INNER                  -> JT_INNER
///   LEFT [OUTER]           -> JT_LEFT|JT_OUTER
///   RIGHT [OUTER]          -> JT_RIGHT|JT_OUTER
///   FULL [OUTER]           -> JT_LEFT|JT_RIGHT|JT_OUTER
///   NATURAL INNER          -> JT_NATURAL|JT_INNER
///   NATURAL LEFT [OUTER]   -> JT_NATURAL|JT_LEFT|JT_OUTER
///   NATURAL RIGHT [OUTER]  -> JT_NATURAL|JT_RIGHT|JT_OUTER
///   NATURAL FULL [OUTER]   -> JT_NATURAL|JT_LEFT|JT_RIGHT
///
/// Por compatibilidade histórica o SQLite aceita também combinações sem sentido (INNER CROSS JOIN,
/// OUTER LEFT JOIN, LEFT NATURAL JOIN, LEFT RIGHT JOIN, CROSS CROSS CROSS JOIN...). As únicas
/// restrições: INNER e CROSS não aparecem junto de OUTER, LEFT, RIGHT ou FULL, e OUTER exige
/// LEFT, RIGHT ou FULL.
pub fn join_type(
    p_parse: &mut Parse,
    p_a: Option<&Token>,
    p_b: Option<&Token>,
    p_c: Option<&Token>,
) -> i32 {
    let mut jointype: i32 = 0;
    //                              0123456789 123456789 123456789 123
    const Z_KEY_TEXT: &[u8] = b"naturaleftouterightfullinnercross";
    // (início do texto em Z_KEY_TEXT, tamanho da palavra, máscara do join)
    const A_KEYWORD: [(u8, u8, u8); 7] = [
        /* (0) natural */ (0, 7, JT_NATURAL),
        /* (1) left    */ (6, 4, JT_LEFT | JT_OUTER),
        /* (2) outer   */ (10, 5, JT_OUTER),
        /* (3) right   */ (14, 5, JT_RIGHT | JT_OUTER),
        /* (4) full    */ (19, 4, JT_LEFT | JT_RIGHT | JT_OUTER),
        /* (5) inner   */ (23, 5, JT_INNER),
        /* (6) cross   */ (28, 5, JT_INNER | JT_CROSS),
    ];
    let ap_all = [p_a, p_b, p_c];
    for slot in ap_all.iter() {
        let p = match slot {
            Some(p) => p,
            None => break,
        };
        let mut j = 0usize;
        while j < A_KEYWORD.len() {
            let (start, n_char, code) = A_KEYWORD[j];
            if p.n == n_char as u32
                && str_n_i_cmp(&p.z, &Z_KEY_TEXT[start as usize..], p.n as i32) == 0
            {
                jointype |= code as i32;
                break;
            }
            j += 1;
        }
        if j >= A_KEYWORD.len() {
            jointype |= JT_ERROR as i32;
            break;
        }
    }
    if (jointype & (JT_INNER | JT_OUTER) as i32) == (JT_INNER | JT_OUTER) as i32
        || (jointype & JT_ERROR as i32) != 0
        || (jointype & (JT_OUTER | JT_LEFT | JT_RIGHT) as i32) == JT_OUTER as i32
    {
        let z_sp1: &[u8] = if p_b.is_none() { b"" } else { b" " };
        let z_sp2: &[u8] = if p_c.is_none() { b"" } else { b" " };
        error_msg(
            p_parse,
            b"unknown join type: %T%s%T%s%T",
            &[
                PrintfArg::Token(p_a),
                PrintfArg::Str(z_sp1),
                PrintfArg::Token(p_b),
                PrintfArg::Str(z_sp2),
                PrintfArg::Token(p_c),
            ],
        );
        jointype = JT_INNER as i32;
    }
    jointype
}

/// Devolve o índice de uma coluna numa tabela, ou -1 se a tabela não a contém.
pub fn column_index(p_tab: &Table, z_col: &[u8]) -> i32 {
    let h = str_i_hash(Some(z_col));
    for i in 0..p_tab.n_col as usize {
        let p_col = &p_tab.a_col[i];
        if p_col.h_name == h && str_i_cmp(&p_col.z_cn_name, z_col) == 0 {
            return i as i32;
        }
    }
    -1
}

/// Marca uma coluna de resultado de subconsulta como usada.
pub fn src_item_column_used(p_item: &mut SrcItem, i_col: i32) {
    debug_assert!((p_item.fg.is_nested_from as i32) == is_nested_from(p_item.p_select.as_deref()) as i32);
    if p_item.fg.is_nested_from != 0 {
        debug_assert!(p_item.p_select.is_some());
        let p_results = p_item
            .p_select
            .as_mut()
            .unwrap()
            .p_elist
            .as_mut()
            .expect("o resultado de um SELECT aninhado precisa ter lista");
        debug_assert!(i_col >= 0 && i_col < p_results.n_expr);
        p_results.a[i_col as usize].fg.b_used = 1;
    }
}

/// Procura nas tabelas `i_start..=i_end` (inclusive) de `p_src` uma tabela com a coluna `z_col`,
/// da esquerda para a direita, e vale a primeira que casar. Se achou, grava em `pi_tab` e `pi_col`
/// o índice da tabela e o da coluna (só quando ambos são `Some`) e devolve verdadeiro; senão
/// devolve falso.
pub fn table_and_column_index(
    p_src: &mut SrcList,
    i_start: i32,
    i_end: i32,
    z_col: &[u8],
    pi_tab: Option<&mut i32>,
    pi_col: Option<&mut i32>,
    b_ignore_hidden: i32,
) -> i32 {
    debug_assert!(i_end < p_src.n_src);
    debug_assert!(i_start >= 0);
    debug_assert!(pi_tab.is_none() == pi_col.is_none()); // ambos ou nenhum são nulos

    let mut pi_tab = pi_tab;
    let mut pi_col = pi_col;
    let mut i = i_start;
    while i <= i_end {
        let i_col = column_index(
            &p_src.a[i as usize]
                .p_tab
                .as_ref()
                .expect("item do FROM sem tabela")
                .borrow(),
            z_col,
        );
        if i_col >= 0
            && (b_ignore_hidden == 0
                || !is_hidden_column(
                    &p_src.a[i as usize].p_tab.as_ref().unwrap().borrow().a_col[i_col as usize],
                ))
        {
            if let (Some(pt), Some(pc)) = (pi_tab.as_deref_mut(), pi_col.as_deref_mut()) {
                src_item_column_used(&mut p_src.a[i as usize], i_col);
                *pt = i;
                *pc = i_col;
            }
            return 1;
        }
        i += 1;
    }
    0
}


// ---- part_001.rs ----

/// Liga a propriedade EP_OuterON (ou EP_InnerON) em todos os termos da expressão dada e define
/// `Expr.w.i_join` como `i_table` em cada termo.
///
/// A propriedade EP_OuterON marca termos que fazem parte da restrição de join especificada na
/// cláusula ON ou USING, e não da cláusula WHERE mais geral. Esses termos são movidos para o WHERE
/// durante o processamento do join, mas é preciso lembrar que se originaram do ON ou USING.
///
/// `Expr.w.i_join` diz ao processamento do WHERE que a expressão depende da tabela `w.i_join`
/// mesmo que ela não seja citada explicitamente. Isso é necessário em casos como:
///
///    SELECT * FROM t1 LEFT JOIN t2 ON t1.a=t2.b AND t1.x=5
///
/// O WHERE precisa adiar o tratamento do termo t1.x=5 até depois do laço de t2. Assim, uma linha
/// NULL de t2 é inserida sempre que t1.x!=5. Se o termo fosse tratado logo após o laço de t1, as
/// linhas com t1.x!=5 nunca apareceriam na saída, o que está incorreto.
pub fn set_join_expr(mut p: Option<&mut Expr>, i_table: i32, join_flag: u32) {
    debug_assert!(join_flag == EP_OUTER_ON || join_flag == EP_INNER_ON);
    while let Some(e) = p {
        expr_set_property(e, join_flag);
        debug_assert!(!expr_has_property(e, EP_TOKEN_ONLY | EP_REDUCED));
        // ExprSetVVAProperty(p, EP_NoReduce) só existe sob SQLITE_DEBUG e some.
        e.w.i_join = i_table;
        if e.op == TK_FUNCTION {
            debug_assert!(expr_use_x_list(e));
            if let Some(p_list) = e.x.p_list.as_mut() {
                for i in 0..p_list.n_expr as usize {
                    set_join_expr(p_list.a[i].p_expr.as_deref_mut(), i_table, join_flag);
                }
            }
        }
        set_join_expr(e.p_left.as_deref_mut(), i_table, join_flag);
        p = e.p_right.as_deref_mut();
    }
}

/// Desfaz o trabalho de `set_join_expr()`. Usado quando um LEFT JOIN é simplificado num JOIN
/// comum e quando uma expressão ON é empurrada para o WHERE de uma subconsulta.
///
/// Converte cada termo marcado com EP_OuterON e `w.i_join==i_table` num termo comum, sem a marca
/// EP_OuterON. Se `i_table<0`, apenas limpa todas as marcas EP_OuterON e EP_InnerON da árvore.
///
/// Se `nullable` é verdadeiro, a expressão pode avaliar para NULL mesmo sendo referência a uma
/// coluna NOT NULL (por exemplo, quando a tabela está do lado esquerdo de um RIGHT JOIN); nesse
/// caso o bit EP_CanBeNull não é removido. Ver o tópico do fórum
/// https://sqlite.org/forum/forumpost/b40696f50145d21c
pub fn unset_join_expr(mut p: Option<&mut Expr>, i_table: i32, nullable: i32) {
    while let Some(e) = p {
        if i_table < 0 || (expr_has_property(e, EP_OUTER_ON) && e.w.i_join == i_table) {
            expr_clear_property(e, EP_OUTER_ON | EP_INNER_ON);
            if i_table >= 0 {
                expr_set_property(e, EP_INNER_ON);
            }
        }
        if e.op == TK_COLUMN && e.i_table == i_table && nullable == 0 {
            expr_clear_property(e, EP_CAN_BE_NULL);
        }
        if e.op == TK_FUNCTION {
            debug_assert!(expr_use_x_list(e));
            debug_assert!(e.p_left.is_none());
            if let Some(p_list) = e.x.p_list.as_mut() {
                for i in 0..p_list.n_expr as usize {
                    unset_join_expr(p_list.a[i].p_expr.as_deref_mut(), i_table, nullable);
                }
            }
        }
        unset_join_expr(e.p_left.as_deref_mut(), i_table, nullable);
        p = e.p_right.as_deref_mut();
    }
}

/// Processa a informação de join de um SELECT.
///
///   * Um join NATURAL é convertido num join USING. Depois disso só é preciso pensar em USING.
///
///   * As cláusulas ON e USING geram termos extras na cláusula WHERE para impor as restrições
///     especificadas. Esses termos recebem EP_OuterON ou EP_InnerON para sabermos que se
///     originaram do ON/USING.
///
/// Os termos do FROM estão em `Select.p_src`. A tabela mais à esquerda é a primeira entrada e a
/// mais à direita é a última. O operador de join fica na entrada da direita: a entrada 1 contém o
/// operador do join entre as entradas 0 e 1. Qualquer ON ou USING do join também fica na entrada
/// da direita.
///
/// Devolve o número de erros encontrados.
pub fn process_join(p_parse: &mut Parse, p: &mut Select) -> i32 {
    // O `p_src` sai do Select durante o processamento, para podermos alterar `p.p_where` e as
    // entradas do FROM ao mesmo tempo. Volta ao Select em todos os caminhos de saída.
    let mut p_src = p
        .p_src
        .take()
        .expect("o Select de um join precisa ter a cláusula FROM");
    // Dobra os '%' do nome, porque a mensagem passa pelo formatador de error_msg.
    let escape = |z: &[u8]| -> Vec<u8> {
        z.iter()
            .flat_map(|&b| if b == b'%' { vec![b'%', b'%'] } else { vec![b] })
            .collect()
    };
    let rc = 'done: {
        for i in 0..p_src.n_src - 1 {
            let li = i as usize;
            let ri = li + 1;
            let p_right_tab = p_src.a[ri].p_tab.clone();

            if never(p_src.a[li].p_tab.is_none() || p_right_tab.is_none()) {
                continue;
            }
            let p_right_tab = p_right_tab.unwrap();
            let join_type: u32 = if (p_src.a[ri].fg.jointype & JT_OUTER) != 0 {
                EP_OUTER_ON
            } else {
                EP_INNER_ON
            };

            // Se é um join NATURAL, sintetiza uma cláusula USING apropriada para dizer quais
            // colunas devem ser unidas.
            if (p_src.a[ri].fg.jointype & JT_NATURAL) != 0 {
                let mut p_using: Option<Box<IdList>> = None;
                if p_src.a[ri].fg.is_using != 0 || matches!(p_src.a[ri].u3, SrcItemU3::On(_)) {
                    error_msg(
                        p_parse,
                        Some(b"a NATURAL join may not have an ON or USING clause"),
                    );
                    break 'done 1;
                }
                let n_col = p_right_tab.borrow().n_col as usize;
                for j in 0..n_col {
                    // Nome da coluna na tabela da direita.
                    let z_name = {
                        let tab = p_right_tab.borrow();
                        if is_hidden_column(&tab.a_col[j]) {
                            continue;
                        }
                        tab.a_col[j].z_cn_name.clone()
                    };
                    if table_and_column_index(&mut p_src, 0, i, &z_name, None, None, 1) != 0 {
                        p_using = id_list_append(p_parse, p_using, &Token::default());
                        if let Some(using) = p_using.as_mut() {
                            debug_assert!(using.n_id > 0);
                            debug_assert!(using.a[(using.n_id - 1) as usize].z_name.is_empty());
                            let db = p_parse
                                .db
                                .upgrade()
                                .expect("a conexão precisa estar viva enquanto o Parse existe");
                            using.a[(using.n_id - 1) as usize].z_name =
                                db_str_dup(&mut db.borrow_mut(), &z_name);
                        }
                    }
                }
                if let Some(using) = p_using {
                    let p_right = &mut p_src.a[ri];
                    p_right.fg.is_using = 1;
                    p_right.fg.is_synth_using = 1;
                    p_right.u3 = SrcItemU3::Using(using);
                }
                if p_parse.n_err != 0 {
                    break 'done 1;
                }
            }

            // Cria termos extras no WHERE para cada coluna nomeada no USING. Exemplo: se as
            // tabelas unidas são A e B e o USING cita X, Y e Z, acrescenta ao WHERE
            // A.X=B.X AND A.Y=B.Y AND A.Z=B.Z. Informa erro se alguma coluna do USING não
            // existe nas duas tabelas.
            if p_src.a[ri].fg.is_using != 0 {
                debug_assert!(matches!(p_src.a[ri].u3, SrcItemU3::Using(_)));
                let n_id = if let SrcItemU3::Using(p_list) = &p_src.a[ri].u3 {
                    p_list.n_id
                } else {
                    0
                };
                let db = p_parse
                    .db
                    .upgrade()
                    .expect("a conexão precisa estar viva enquanto o Parse existe");
                for j in 0..n_id as usize {
                    // Nome do termo na cláusula USING.
                    let z_name = if let SrcItemU3::Using(p_list) = &p_src.a[ri].u3 {
                        p_list.a[j].z_name.clone()
                    } else {
                        Vec::new()
                    };
                    // Tabela da esquerda com coluna de mesmo nome e número dessa coluna.
                    let mut i_left: i32 = 0;
                    let mut i_left_col: i32 = 0;
                    let i_right_col = column_index(&p_right_tab.borrow(), &z_name);
                    let b_synth_using = p_src.a[ri].fg.is_synth_using as i32;
                    if i_right_col < 0
                        || table_and_column_index(
                            &mut p_src,
                            0,
                            i,
                            &z_name,
                            Some(&mut i_left),
                            Some(&mut i_left_col),
                            b_synth_using,
                        ) == 0
                    {
                        let z_msg = [
                            &b"cannot join using column "[..],
                            &escape(&z_name)[..],
                            &b" - column not present in both tables"[..],
                        ]
                        .concat();
                        error_msg(p_parse, Some(&z_msg));
                        break 'done 1;
                    }
                    // Referência à coluna do lado ESQUERDO do join.
                    let mut p_e1 = create_column_expr(
                        &mut db.borrow_mut(),
                        &p_src,
                        i_left as usize,
                        i_left_col,
                    );
                    src_item_column_used(&mut p_src.a[i_left as usize], i_left_col);
                    if (p_src.a[0].fg.jointype & JT_LTORJ) != 0 {
                        // Este ramo roda se a consulta tem um ou mais RIGHT ou FULL JOIN. Se só
                        // uma tabela do lado esquerdo contém a coluna zName, o ramo não faz
                        // nada. Mas se há duas ou mais tabelas à esquerda, monta uma função
                        // coalesce() que reúne todas elas. Informa erro se mais de uma dessas
                        // referências a zName não está também numa cláusula USING anterior.
                        //
                        // O certo seria informar erro se houver duas ou mais referências fora do
                        // USING a zName à esquerda de um INNER ou LEFT JOIN. Mas versões antigas
                        // do SQLite não fazem isso, então se evita criar um erro novo para não
                        // quebrar aplicações legadas.
                        let mut p_func_args: Option<Box<ExprList>> = None; // Argumentos do coalesce()
                        let tk_coalesce = Token {
                            z: b"coalesce".to_vec(),
                            n: 8,
                        };
                        while table_and_column_index(
                            &mut p_src,
                            i_left + 1,
                            i,
                            &z_name,
                            Some(&mut i_left),
                            Some(&mut i_left_col),
                            b_synth_using,
                        ) != 0
                        {
                            let ambiguous = p_src.a[i_left as usize].fg.is_using == 0
                                || match &p_src.a[i_left as usize].u3 {
                                    SrcItemU3::Using(p_using) => {
                                        id_list_index(p_using, &z_name) < 0
                                    }
                                    _ => true,
                                };
                            if ambiguous {
                                let z_msg = [
                                    &b"ambiguous reference to "[..],
                                    &escape(&z_name)[..],
                                    &b" in USING()"[..],
                                ]
                                .concat();
                                error_msg(p_parse, Some(&z_msg));
                                break;
                            }
                            p_func_args = expr_list_append(p_parse, p_func_args, p_e1.take());
                            p_e1 = create_column_expr(
                                &mut db.borrow_mut(),
                                &p_src,
                                i_left as usize,
                                i_left_col,
                            );
                            src_item_column_used(&mut p_src.a[i_left as usize], i_left_col);
                        }
                        if p_func_args.is_some() {
                            p_func_args = expr_list_append(p_parse, p_func_args, p_e1.take());
                            p_e1 = expr_function(p_parse, p_func_args, &tk_coalesce, 0);
                        }
                    }
                    // Referência à coluna do lado DIREITO do join.
                    let p_e2 = create_column_expr(
                        &mut db.borrow_mut(),
                        &p_src,
                        ri,
                        i_right_col,
                    );
                    src_item_column_used(&mut p_src.a[ri], i_right_col);
                    // O i_table de pE2 é lido antes de a expressão ser entregue ao pEq.
                    let e2_table = p_e2.as_ref().map(|e| e.i_table);
                    // Restrição de igualdade pE1 == pE2.
                    let mut p_eq = p_expr(p_parse, TK_EQ as i32, p_e1, p_e2);
                    debug_assert!(e2_table.is_some() || p_eq.is_none());
                    if let Some(eq) = p_eq.as_deref_mut() {
                        expr_set_property(eq, join_type);
                        debug_assert!(!expr_has_property(eq, EP_TOKEN_ONLY | EP_REDUCED));
                        eq.w.i_join = e2_table.unwrap_or(0);
                    }
                    p.p_where = expr_and(p_parse, p.p_where.take(), p_eq);
                }
            }
            // Acrescenta a cláusula ON ao fim do WHERE, ligada por um AND.
            else if matches!(p_src.a[ri].u3, SrcItemU3::On(_)) {
                // O `pRight->u3.pOn = 0` do C vira a troca do ON por SrcItemU3::None.
                if let SrcItemU3::On(mut p_on) =
                    std::mem::replace(&mut p_src.a[ri].u3, SrcItemU3::None)
                {
                    set_join_expr(Some(&mut p_on), p_src.a[ri].i_cursor, join_type);
                    p.p_where = expr_and(p_parse, p.p_where.take(), Some(p_on));
                    p_src.a[ri].fg.is_on = 1;
                }
            }
        }
        0
    };
    p.p_src = Some(p_src);
    rc
}

/// Guarda a informação (além de p_parse e p_select) necessária para carregar a próxima linha de
/// resultado que vai para o sorter. Sem SQLITE_ENABLE_SORTER_REFERENCES (opção que o Debian não
/// liga), os campos `pExtra` e `regExtraResult` não existem.
#[derive(Clone, Default)]
pub struct RowLoadInfo {
    /// Guarda os resultados neste arranjo de registros.
    pub reg_result: i32,
    /// Argumento de flags de `expr_code_expr_list()`.
    pub ecel_flags: u8,
}

/// Faz o trabalho de carregar os dados da consulta num arranjo de registros para que possam ser
/// adicionados ao sorter.
pub fn inner_loop_load_row(p_parse: &mut Parse, p_select: &Select, p_info: &RowLoadInfo) {
    expr_code_expr_list(
        p_parse,
        p_select
            .p_elist
            .as_deref()
            .expect("o Select precisa ter a lista de resultado"),
        p_info.reg_result,
        0,
        p_info.ecel_flags,
    );
}

/// Gera o OP_MakeRecord que produz a entrada a ser adicionada ao sorter.
///
/// Devolve o registro em que o resultado é guardado.
pub fn make_sorter_record(
    p_parse: &mut Parse,
    p_sort: &SortCtx,
    p_select: &Select,
    reg_base: i32,
    n_base: i32,
) -> i32 {
    let n_ob_sat = p_sort.n_ob_sat;
    let v = p_parse
        .p_vdbe
        .clone()
        .expect("o Parse precisa ter um Vdbe em construção");
    p_parse.n_mem += 1;
    let reg_out = p_parse.n_mem;
    if let Some(p_info) = p_sort.p_deferred_row_load.as_deref() {
        inner_loop_load_row(p_parse, p_select, p_info);
    }
    vdbe_add_op3(
        &mut v.borrow_mut(),
        OP_MAKERECORD as i32,
        reg_base + n_ob_sat,
        n_base - n_ob_sat,
        reg_out,
    );
    reg_out
}

/// Gera o código que empurra para o sorter o registro que está nos registros `reg_data` até
/// `reg_data+n_data-1`.
pub fn push_onto_sorter(
    p_parse: &mut Parse,    // Contexto do parser
    p_sort: &mut SortCtx,   // Informação sobre a cláusula ORDER BY
    p_select: &Select,      // O SELECT inteiro
    reg_data: i32,          // Primeiro registro com os dados a ordenar
    reg_orig_data: i32,     // Primeiro registro com os dados antes de empacotar
    n_data: i32,            // Número de elementos do arranjo em reg_data
    n_prefix_reg: i32,      // Nº de registros antes de reg_data disponíveis para uso
) {
    // Comando em construção.
    let v = p_parse
        .p_vdbe
        .clone()
        .expect("o Parse precisa ter um Vdbe em construção");
    let b_seq: i32 = if (p_sort.sort_flags & SORTFLAG_USESORTER) == 0 {
        1
    } else {
        0
    };
    // Número de termos do ORDER BY.
    let n_expr = p_sort
        .p_order_by
        .as_ref()
        .expect("o sorter precisa ter a lista ORDER BY")
        .n_expr;
    // Campos do registro do sorter.
    let n_base = n_expr + b_seq + n_data;
    // Registros do registro do sorter.
    let reg_base: i32;
    // Registro do sorter já montado.
    let mut reg_record: i32 = 0;
    // Termos do ORDER BY a pular.
    let n_ob_sat = p_sort.n_ob_sat;
    // Opcode que adiciona o registro ao sorter.
    let op: u8;
    // Contador do LIMIT.
    let i_limit: i32;
    // Fim do laço de inserção no sorter.
    let mut i_skip: i32 = 0;

    debug_assert!(b_seq == 0 || b_seq == 1);

    // Três casos:
    //   (1) Os dados a ordenar já foram empacotados num registro por um OP_MakeRecord anterior.
    //       Aqui n_data==1 e reg_data não tem relação com reg_orig_data.
    //   (2) Todas as colunas de saída entram no registro do sorter. Aqui reg_data==reg_orig_data.
    //   (3) Algumas colunas de saída ficam fora do registro do sorter, pela otimização
    //       SQLITE_ENABLE_SORTER_REFERENCES, pela SQLITE_ECEL_OMITREF ou pela
    //       SortCtx.pDeferredRowLoad. Nesses casos reg_orig_data é 0 para esta rotina não tentar
    //       copiar valores que talvez ainda não existam.
    debug_assert!(n_data == 1 || reg_data == reg_orig_data || reg_orig_data == 0);

    if n_prefix_reg != 0 {
        debug_assert!(n_prefix_reg == n_expr + b_seq);
        reg_base = reg_data - n_prefix_reg;
    } else {
        reg_base = p_parse.n_mem + 1;
        p_parse.n_mem += n_base;
    }
    debug_assert!(p_select.i_offset == 0 || p_select.i_limit != 0);
    i_limit = if p_select.i_offset != 0 {
        p_select.i_offset + 1
    } else {
        p_select.i_limit
    };
    p_sort.label_done = vdbe_make_label(p_parse);
    expr_code_expr_list(
        p_parse,
        p_sort
            .p_order_by
            .as_deref()
            .expect("o sorter precisa ter a lista ORDER BY"),
        reg_base,
        reg_orig_data,
        SQLITE_ECEL_DUP | if reg_orig_data != 0 { SQLITE_ECEL_REF } else { 0 },
    );
    if b_seq != 0 {
        vdbe_add_op2(
            &mut v.borrow_mut(),
            OP_SEQUENCE as i32,
            p_sort.i_e_cursor,
            reg_base + n_expr,
        );
    }
    if n_prefix_reg == 0 && n_data > 0 {
        expr_code_move(p_parse, reg_data, reg_base + n_expr + b_seq, n_data);
    }
    if n_ob_sat > 0 {
        // As primeiras n_ob_sat colunas da linha anterior.
        let reg_prev_key: i32;
        // Endereço do OP_IfNot.
        let addr_first: i32;
        // Endereço do OP_Jump.
        let addr_jmp: i32;
        // Número de colunas-chave do sorter, incluindo o OP_Sequence.
        let n_key: i32;

        reg_record = make_sorter_record(p_parse, p_sort, p_select, reg_base, n_base);
        reg_prev_key = p_parse.n_mem + 1;
        p_parse.n_mem += p_sort.n_ob_sat;
        n_key = n_expr - p_sort.n_ob_sat + b_seq;
        if b_seq != 0 {
            addr_first = vdbe_add_op1(&mut v.borrow_mut(), OP_IFNOT as i32, reg_base + n_expr);
        } else {
            addr_first = vdbe_add_op1(
                &mut v.borrow_mut(),
                OP_SEQUENCETEST as i32,
                p_sort.i_e_cursor,
            );
        }
        // VdbeCoverage(v) some: o Debian não liga SQLITE_VDBE_COVERAGE.
        vdbe_add_op3(
            &mut v.borrow_mut(),
            OP_COMPARE as i32,
            reg_prev_key,
            reg_base,
            p_sort.n_ob_sat,
        );
        // O opcode que abre o sorter; seu P4 é o KeyInfo original da tabela do sorter.
        let p_ki: KeyInfoRef = {
            let mut vb = v.borrow_mut();
            let p_op = vdbe_get_op(&mut vb, p_sort.addr_sort_index);
            if p_parse
                .db
                .upgrade()
                .expect("a conexão precisa estar viva enquanto o Parse existe")
                .borrow()
                .malloc_failed
                != 0
            {
                return;
            }
            p_op.p2 = n_key + n_data;
            if let P4Value::KeyInfo(p_ki) = &p_op.p4 {
                p_ki.clone()
            } else {
                unreachable!("o opcode que abre o sorter sempre tem P4 do tipo KeyInfo")
            }
        };
        {
            // Faz o OP_Jump ser testável.
            let mut ki = p_ki.borrow_mut();
            let n_key_field = ki.n_key_field as usize;
            ki.a_sort_flags[..n_key_field].fill(0);
        }
        // O KeyInfo original passa a ser o P4 do OP_Compare recém-inserido.
        vdbe_change_p4(
            &mut v.borrow_mut(),
            -1,
            P4Value::KeyInfo(p_ki.clone()),
            P4_KEYINFO as i32,
        );
        let n_extra = {
            let ki = p_ki.borrow();
            ki.n_all_field as i32 - ki.n_key_field as i32 - 1
        };
        let p_new_ki = key_info_from_expr_list(
            p_parse,
            p_sort
                .p_order_by
                .as_deref()
                .expect("o sorter precisa ter a lista ORDER BY"),
            n_ob_sat,
            n_extra,
        );
        {
            let mut vb = v.borrow_mut();
            let p_op = vdbe_get_op(&mut vb, p_sort.addr_sort_index);
            p_op.p4 = match p_new_ki {
                Some(p_new_ki) => P4Value::KeyInfo(p_new_ki),
                None => P4Value::NotUsed,
            };
        }
        addr_jmp = vdbe_current_addr(&v.borrow());
        vdbe_add_op3(
            &mut v.borrow_mut(),
            OP_JUMP as i32,
            addr_jmp + 1,
            0,
            addr_jmp + 1,
        );
        p_sort.label_bk_out = vdbe_make_label(p_parse);
        p_parse.n_mem += 1;
        p_sort.reg_return = p_parse.n_mem;
        vdbe_add_op2(
            &mut v.borrow_mut(),
            OP_GOSUB as i32,
            p_sort.reg_return,
            p_sort.label_bk_out,
        );
        vdbe_add_op1(
            &mut v.borrow_mut(),
            OP_RESETSORTER as i32,
            p_sort.i_e_cursor,
        );
        if i_limit != 0 {
            vdbe_add_op2(
                &mut v.borrow_mut(),
                OP_IFNOT as i32,
                i_limit,
                p_sort.label_done,
            );
        }
        vdbe_jump_here(&mut v.borrow_mut(), addr_first);
        expr_code_move(p_parse, reg_base, reg_prev_key, p_sort.n_ob_sat);
        vdbe_jump_here(&mut v.borrow_mut(), addr_jmp);
    }
    if i_limit != 0 {
        // Neste ponto os valores da nova entrada do sorter estão num arranjo de registros. Eles
        // precisam ser compostos num registro e inseridos no sorter se (a) há menos de
        // LIMIT+OFFSET itens no momento ou (b) o novo registro é menor que o maior registro
        // atual do sorter. Se vale (b) e já há LIMIT+OFFSET itens, apaga a maior entrada antes
        // de inserir a nova. Assim nunca há mais de LIMIT+OFFSET itens no sorter.
        //
        // Se o novo registro não precisa entrar no sorter, salta para a próxima iteração do
        // laço. Se pSort->labelOBLopt não é zero, ele é o rótulo do destino do salto; senão
        // apenas se pula a lógica de inserção. Ver o comentário de cabeçalho de
        // `where_order_by_limit_opt_label()` para mais informação.
        let i_csr = p_sort.i_e_cursor;
        let addr_now = vdbe_current_addr(&v.borrow());
        vdbe_add_op2(
            &mut v.borrow_mut(),
            OP_IFNOTZERO as i32,
            i_limit,
            addr_now + 4,
        );
        vdbe_add_op2(&mut v.borrow_mut(), OP_LAST as i32, i_csr, 0);
        i_skip = vdbe_add_op4_int(
            &mut v.borrow_mut(),
            OP_IDXLE as i32,
            i_csr,
            0,
            reg_base + n_ob_sat,
            n_expr - n_ob_sat,
        );
        vdbe_add_op1(&mut v.borrow_mut(), OP_DELETE as i32, i_csr);
    }
    if reg_record == 0 {
        reg_record = make_sorter_record(p_parse, p_sort, p_select, reg_base, n_base);
    }
    if (p_sort.sort_flags & SORTFLAG_USESORTER) != 0 {
        op = OP_SORTERINSERT;
    } else {
        op = OP_IDXINSERT;
    }
    vdbe_add_op4_int(
        &mut v.borrow_mut(),
        op as i32,
        p_sort.i_e_cursor,
        reg_record,
        reg_base + n_ob_sat,
        n_base - n_ob_sat,
    );
    if i_skip != 0 {
        let addr_target = if p_sort.label_ob_lopt != 0 {
            p_sort.label_ob_lopt
        } else {
            vdbe_current_addr(&v.borrow())
        };
        vdbe_change_p2(&mut v.borrow_mut(), i_skip, addr_target);
    }
}


// ---- part_002.rs ----

/// Lê `u.x.i_order_by_col` de um item de ExprList (a união `u` é um enum no Rust).
fn item_order_by_col(p_item: &ExprListItem) -> i32 {
    match p_item.u {
        ExprListItemU::X { i_order_by_col, .. } => i_order_by_col as i32,
        ExprListItemU::IConstExprReg(_) => 0,
    }
}

/// Grava `u.x.i_order_by_col` de um item de ExprList, preservando `i_alias`.
fn item_set_order_by_col(p_item: &mut ExprListItem, i_col: i32) {
    if let ExprListItemU::X { i_alias, .. } = p_item.u {
        p_item.u = ExprListItemU::X {
            i_order_by_col: i_col as u16,
            i_alias,
        };
    }
}

/// Adiciona código para implementar o OFFSET.
fn code_offset(
    v: &VdbeRef,        // Gera código nesta VM
    i_offset: i32,      // Registro com o contador do offset
    i_continue: i32,    // Salta para cá para pular o registro atual
) {
    if i_offset > 0 {
        vdbe_add_op3(&mut v.borrow_mut(), OP_IFPOS as i32, i_offset, i_continue, 1);
        // VdbeCoverage e VdbeComment não existem neste build.
    }
}

/// Adiciona código que verifica se o arranjo de registros a partir de `reg_elem` forma uma
/// entrada distinta. Usado por "SELECT DISTINCT ..." e por agregados distintos
/// ("SELECT count(DISTINCT <expr>) ..."). Há três estratégias, escolhidas por `e_tnct_type`:
///
///   WHERE_DISTINCT_UNORDERED/WHERE_DISTINCT_NOOP:
///     Usa uma tabela efêmera com todas as entradas já vistas e ignora as repetidas. `i_tab` é o
///     cursor da tabela efêmera, que precisa estar aberta antes de o código gerado rodar. Se
///     existe registro idêntico, salta para `addr_repeat`; senão insere o novo e prossegue.
///     Devolve uma cópia de `i_tab`.
///
///   WHERE_DISTINCT_ORDERED:
///     As linhas chegam ordenadas. A tabela efêmera não é necessária: os valores atuais são
///     comparados com a linha anterior e, se casam, salta para `addr_repeat`. Devolve o primeiro
///     registro do arranjo que guarda a linha anterior; o chamador garante que ele começa NULL
///     (quem cuida disso é `fix_distinct_open_eph()`).
///
///   WHERE_DISTINCT_UNIQUE:
///     Já se sabe que as linhas são distintas. Nada a fazer; devolve zero.
///
/// `p_e_list` dá o número de elementos do arranjo e as colações usadas na comparação ORDERED.
fn code_distinct(
    p_parse: &mut Parse,
    e_tnct_type: i32,
    i_tab: i32,
    addr_repeat: i32,
    p_e_list: &ExprList,
    reg_elem: i32,
) -> i32 {
    let mut i_ret = 0;
    let n_result_col = p_e_list.n_expr;
    let v = p_parse
        .p_vdbe
        .clone()
        .expect("o Parse precisa ter um Vdbe em construção");

    if e_tnct_type == WHERE_DISTINCT_ORDERED as i32 {
        // Aloca espaço para a linha anterior.
        p_parse.n_mem += 1;
        let reg_prev = p_parse.n_mem;
        i_ret = reg_prev;
        p_parse.n_mem += n_result_col - 1;

        let i_jump = vdbe_current_addr(&v.borrow()) + n_result_col;
        for i in 0..n_result_col {
            let p_coll = expr_coll_seq(
                p_parse,
                p_e_list.a[i as usize]
                    .p_expr
                    .as_deref()
                    .expect("termo do DISTINCT sem expressão"),
            );
            let mut vb = v.borrow_mut();
            if i < n_result_col - 1 {
                vdbe_add_op3(&mut vb, OP_NE as i32, reg_elem + i, i_jump, reg_prev + i);
            } else {
                vdbe_add_op3(&mut vb, OP_EQ as i32, reg_elem + i, addr_repeat, reg_prev + i);
            }
            let p4 = match p_coll {
                Some(c) => P4Value::CollSeq(c),
                None => P4Value::NotUsed,
            };
            vdbe_change_p4(&mut vb, -1, p4, P4_COLLSEQ as i32);
            vdbe_change_p5(&mut vb, SQLITE_NULLEQ as u16);
        }
        debug_assert!(
            vdbe_current_addr(&v.borrow()) == i_jump
                || p_parse
                    .db
                    .upgrade()
                    .expect("a conexão precisa estar viva enquanto o Parse existe")
                    .borrow()
                    .malloc_failed
                    != 0
        );
        vdbe_add_op3(
            &mut v.borrow_mut(),
            OP_COPY as i32,
            reg_elem,
            reg_prev,
            n_result_col - 1,
        );
    } else if e_tnct_type == WHERE_DISTINCT_UNIQUE as i32 {
        // nada a fazer
    } else {
        let r1 = get_temp_reg(p_parse);
        let mut vb = v.borrow_mut();
        vdbe_add_op4_int(&mut vb, OP_FOUND as i32, i_tab, addr_repeat, reg_elem, n_result_col);
        vdbe_add_op3(&mut vb, OP_MAKERECORD as i32, reg_elem, n_result_col, r1);
        vdbe_add_op4_int(&mut vb, OP_IDXINSERT as i32, i_tab, r1, reg_elem, n_result_col);
        vdbe_change_p5(&mut vb, OPFLAG_USESEEKRESULT as u16);
        drop(vb);
        release_temp_reg(p_parse, r1);
        i_ret = i_tab;
    }

    i_ret
}

/// Roda depois de `code_distinct()`. Faz os ajustes necessários no OP_OpenEphemeral que
/// `code_distinct()` usou. O processamento é separado porque às vezes `code_distinct()` é
/// chamada antes de o OP_OpenEphemeral ser de fato colocado.
///
/// WHERE_DISTINCT_NOOP e WHERE_DISTINCT_UNORDERED: nenhum ajuste.
///
/// WHERE_DISTINCT_UNIQUE: a tabela efêmera não é necessária; o OP_OpenEphemeral vira OP_Noop.
///
/// WHERE_DISTINCT_ORDERED: a tabela efêmera não é necessária, mas o registro `i_val` precisa
/// começar NULL; o OP_OpenEphemeral vira um OP_Null nesse registro.
fn fix_distinct_open_eph(
    p_parse: &mut Parse,
    e_tnct_type: i32,
    i_val: i32,
    i_open_eph_addr: i32,
) {
    if p_parse.n_err == 0
        && (e_tnct_type == WHERE_DISTINCT_UNIQUE as i32
            || e_tnct_type == WHERE_DISTINCT_ORDERED as i32)
    {
        let v = p_parse
            .p_vdbe
            .clone()
            .expect("o Parse precisa ter um Vdbe em construção");
        let mut vb = v.borrow_mut();
        vdbe_change_to_noop(&mut vb, i_open_eph_addr);
        if vdbe_get_op(&mut vb, i_open_eph_addr + 1).opcode == OP_EXPLAIN {
            vdbe_change_to_noop(&mut vb, i_open_eph_addr + 1);
        }
        if e_tnct_type == WHERE_DISTINCT_ORDERED as i32 {
            // Troca o OP_OpenEphemeral por um OP_Null que liga o bit MEM_Cleared no primeiro
            // registro do valor anterior. Isso faz o OP_Ne de `code_distinct()` sempre falhar
            // na primeira volta do laço, mesmo que a primeira linha seja toda NULL.
            let p_op = vdbe_get_op(&mut vb, i_open_eph_addr);
            p_op.opcode = OP_NULL;
            p_op.p1 = 1;
            p_op.p2 = i_val;
        }
    }
}

// `selectExprDefer()` só existe sob SQLITE_ENABLE_SORTER_REFERENCES, que o Debian não liga:
// a função some, junto com `SortCtx.aDefer`, `nDefer`, `pExtra` e `ExprList_item.fg.bSorterRef`.

/// Gera o código do interior do laço interno de um SELECT.
///
/// Se `src_tab` é negativo, as expressões de `p.p_elist` são avaliadas para obter os dados da
/// linha. Se é zero ou mais, os dados vêm de `src_tab` e `p.p_elist` serve só para obter o
/// número de colunas e a colação de cada coluna.
fn select_inner_loop(
    p_parse: &mut Parse,            // O contexto do parser
    p: &mut Select,                 // O SELECT completo que está sendo codificado
    src_tab: i32,                   // Lê os dados desta tabela se não negativo
    p_sort: Option<&mut SortCtx>,   // Se presente, como processar o ORDER BY
    p_distinct: Option<&DistinctCtx>, // Se presente, como processar o DISTINCT
    p_dest: &mut SelectDest,        // Como dispor dos resultados
    i_continue: i32,                // Salta para cá para seguir à próxima linha
    i_break: i32,                   // Salta para cá para sair do laço interno
) {
    let v = p_parse
        .p_vdbe
        .clone()
        .expect("o Parse precisa ter um Vdbe em construção");
    let has_distinct: i32 = match p_distinct {
        Some(d) => d.e_tnct_type as i32,
        None => WHERE_DISTINCT_NOOP as i32,
    };
    let e_dest = p_dest.e_dest;
    let i_parm = p_dest.i_sdparm;
    let mut n_prefix_reg: i32 = 0;
    let mut s_row_load_info = RowLoadInfo::default();

    debug_assert!(p.p_elist.is_some());
    // if( pSort && pSort->pOrderBy==0 ) pSort = 0;
    let mut p_sort: Option<&mut SortCtx> = match p_sort {
        Some(s) if s.p_order_by.is_some() => Some(s),
        _ => None,
    };
    if p_sort.is_none() && has_distinct == 0 {
        debug_assert!(i_continue != 0);
        code_offset(&v, p.i_offset, i_continue);
    }

    // Puxa as colunas pedidas.
    let mut n_result_col = p.p_elist.as_ref().unwrap().n_expr;

    if p_dest.i_sdst == 0 {
        if let Some(s) = p_sort.as_deref() {
            n_prefix_reg = s.p_order_by.as_ref().unwrap().n_expr;
            if (s.sort_flags & SORTFLAG_USESORTER) == 0 {
                n_prefix_reg += 1;
            }
            p_parse.n_mem += n_prefix_reg;
        }
        p_dest.i_sdst = p_parse.n_mem + 1;
        p_parse.n_mem += n_result_col;
    } else if p_dest.i_sdst + n_result_col > p_parse.n_mem {
        // Condição de erro que pode ocorrer, por exemplo, quando um SELECT do lado direito de um
        // INSERT tem mais colunas que a tabela da esquerda. O erro é pego e relatado depois, mas
        // é preciso alocar memória suficiente para evitar erros espúrios nesse meio tempo.
        p_parse.n_mem += n_result_col;
    }
    p_dest.n_sdst = n_result_col;
    // Normalmente reg_result é a primeira célula do arranjo com a linha de resultado atual e
    // reg_orig recebe o mesmo valor. Mas se os resultados vão para o sorter, os valores das
    // expressões que também fazem parte da chave de ordenação ficam de fora do arranjo e
    // reg_orig vira zero.
    let reg_result = p_dest.i_sdst;
    let mut reg_orig = reg_result;
    if src_tab >= 0 {
        for i in 0..n_result_col {
            vdbe_add_op3(
                &mut v.borrow_mut(),
                OP_COLUMN as i32,
                src_tab,
                i,
                reg_result + i,
            );
        }
    } else if e_dest != SRT_EXISTS {
        // Se o destino é uma expressão EXISTS(...), os valores devolvidos não são necessários.
        // "ecel" é abreviação de "ExprCodeExprList".
        let mut ecel_flags: u8 = if e_dest == SRT_MEM || e_dest == SRT_OUTPUT || e_dest == SRT_COROUTINE
        {
            SQLITE_ECEL_DUP
        } else {
            0
        };
        if p_sort.is_some() && has_distinct == 0 && e_dest != SRT_EPHEMTAB && e_dest != SRT_TABLE {
            // Para cada expressão de `p.p_elist` que é cópia de uma expressão do ORDER BY, grava
            // em `i_order_by_col` o índice mais um da expressão dentro da chave que
            // `push_onto_sorter()` vai gerar. Assim `p.p_elist` pode ficar fora do registro
            // ordenado, poupando espaço e CPU.
            ecel_flags |= SQLITE_ECEL_OMITREF | SQLITE_ECEL_REF;

            let s = p_sort.as_deref().unwrap();
            let n_ob_sat = s.n_ob_sat;
            let n_order = s.p_order_by.as_ref().unwrap().n_expr;
            for i in n_ob_sat..n_order {
                let j = item_order_by_col(&s.p_order_by.as_ref().unwrap().a[i as usize]);
                if j > 0 {
                    item_set_order_by_col(
                        &mut p.p_elist.as_mut().unwrap().a[(j - 1) as usize],
                        i + 1 - n_ob_sat,
                    );
                }
            }

            // Ajusta n_result_col para as colunas omitidas do sorter por estas otimizações.
            let p_e_list = p.p_elist.as_ref().unwrap();
            for i in 0..p_e_list.n_expr {
                if item_order_by_col(&p_e_list.a[i as usize]) > 0 {
                    n_result_col -= 1;
                    reg_orig = 0;
                }
            }

            debug_assert!(
                e_dest == SRT_SET
                    || e_dest == SRT_MEM
                    || e_dest == SRT_COROUTINE
                    || e_dest == SRT_OUTPUT
                    || e_dest == SRT_UPFROM
            );
        }
        s_row_load_info.reg_result = reg_result;
        s_row_load_info.ecel_flags = ecel_flags;
        if p.i_limit != 0 && (ecel_flags & SQLITE_ECEL_OMITREF) != 0 && n_prefix_reg > 0 {
            debug_assert!(p_sort.is_some());
            debug_assert!(has_distinct == 0);
            p_sort.as_deref_mut().unwrap().p_deferred_row_load = Some(Box::new(s_row_load_info));
            reg_orig = 0;
        } else {
            inner_loop_load_row(p_parse, p, &s_row_load_info);
        }
    }

    // Se o SELECT tem DISTINCT e a linha já foi vista, ela não entra no resultado.
    if has_distinct != 0 {
        let d = p_distinct.unwrap();
        let e_type = d.e_tnct_type as i32;
        debug_assert!(n_result_col == p.p_elist.as_ref().unwrap().n_expr);
        let i_tab = code_distinct(
            p_parse,
            e_type,
            d.tab_tnct,
            i_continue,
            p.p_elist.as_ref().unwrap(),
            reg_result,
        );
        fix_distinct_open_eph(p_parse, e_type, i_tab, d.addr_tnct);
        if p_sort.is_none() {
            code_offset(&v, p.i_offset, i_continue);
        }
    }

    if e_dest == SRT_UNION {
        // Grava cada resultado na chave da tabela temporária i_parm.
        let r1 = get_temp_reg(p_parse);
        let mut vb = v.borrow_mut();
        vdbe_add_op3(&mut vb, OP_MAKERECORD as i32, reg_result, n_result_col, r1);
        vdbe_add_op4_int(&mut vb, OP_IDXINSERT as i32, i_parm, r1, reg_result, n_result_col);
        drop(vb);
        release_temp_reg(p_parse, r1);
    } else if e_dest == SRT_EXCEPT {
        // Monta um registro com o resultado, mas em vez de guardá-lo o usa como chave para
        // apagar elementos da tabela temporária i_parm.
        vdbe_add_op3(
            &mut v.borrow_mut(),
            OP_IDXDELETE as i32,
            i_parm,
            reg_result,
            n_result_col,
        );
    } else if e_dest == SRT_FIFO
        || e_dest == SRT_DISTFIFO
        || e_dest == SRT_TABLE
        || e_dest == SRT_EPHEMTAB
    {
        // Guarda o resultado como dado, usando uma chave única.
        let r1 = get_temp_range(p_parse, n_prefix_reg + 1);
        vdbe_add_op3(
            &mut v.borrow_mut(),
            OP_MAKERECORD as i32,
            reg_result,
            n_result_col,
            r1 + n_prefix_reg,
        );
        // O ramo OPFLAG_NOCHNG_MAGIC só existe sob SQLITE_DEBUG e some.
        if e_dest == SRT_DISTFIFO {
            // Se o destino é DistFifo, o cursor (i_parm+1) está aberto sobre um índice efêmero.
            // Se a linha atual já está no índice, não a escreve na saída. Senão a acrescenta
            // ao índice e segue escrevendo na tabela de saída também.
            let mut vb = v.borrow_mut();
            let addr = vdbe_current_addr(&vb) + 4;
            vdbe_add_op4_int(&mut vb, OP_FOUND as i32, i_parm + 1, addr, r1, 0);
            vdbe_add_op4_int(
                &mut vb,
                OP_IDXINSERT as i32,
                i_parm + 1,
                r1,
                reg_result,
                n_result_col,
            );
            debug_assert!(p_sort.is_none());
        }
        if let Some(s) = p_sort.as_deref_mut() {
            debug_assert!(reg_result == reg_orig);
            push_onto_sorter(p_parse, s, p, r1 + n_prefix_reg, reg_orig, 1, n_prefix_reg);
        } else {
            let r2 = get_temp_reg(p_parse);
            let mut vb = v.borrow_mut();
            vdbe_add_op2(&mut vb, OP_NEWROWID as i32, i_parm, r2);
            vdbe_add_op3(&mut vb, OP_INSERT as i32, i_parm, r1, r2);
            vdbe_change_p5(&mut vb, OPFLAG_APPEND as u16);
            drop(vb);
            release_temp_reg(p_parse, r2);
        }
        release_temp_range(p_parse, r1, n_prefix_reg + 1);
    } else if e_dest == SRT_UPFROM {
        if let Some(s) = p_sort.as_deref_mut() {
            push_onto_sorter(p_parse, s, p, reg_result, reg_orig, n_result_col, n_prefix_reg);
        } else {
            let i2 = p_dest.i_sdparm2;
            let r1 = get_temp_reg(p_parse);
            let mut vb = v.borrow_mut();
            // Se o UPDATE FROM é um agregado que não casa linha alguma, ele ainda pode tentar
            // devolver uma linha, porque é o que agregados fazem. Não registra essa linha vazia.
            vdbe_add_op2(&mut vb, OP_ISNULL as i32, reg_result, i_break);
            let neg = (i2 < 0) as i32;
            vdbe_add_op3(
                &mut vb,
                OP_MAKERECORD as i32,
                reg_result + neg,
                n_result_col - neg,
                r1,
            );
            if i2 < 0 {
                vdbe_add_op3(&mut vb, OP_INSERT as i32, i_parm, r1, reg_result);
            } else {
                vdbe_add_op4_int(&mut vb, OP_IDXINSERT as i32, i_parm, r1, reg_result, i2);
            }
        }
    } else if e_dest == SRT_SET {
        // Cria um conjunto para "expr IN (SELECT ...)": deve haver um único item na pilha, que
        // vai para a tabela do conjunto com dado falso.
        if let Some(s) = p_sort.as_deref_mut() {
            // À primeira vista daria para eliminar o ORDER BY, já que a ordem das entradas do
            // conjunto não importa. Mas pode haver LIMIT, e então a ordem importa.
            push_onto_sorter(p_parse, s, p, reg_result, reg_orig, n_result_col, n_prefix_reg);
        } else {
            let r1 = get_temp_reg(p_parse);
            debug_assert!(strlen30_nn(&p_dest.z_aff_sdst) == n_result_col);
            let mut vb = v.borrow_mut();
            vdbe_add_op4(
                &mut vb,
                OP_MAKERECORD as i32,
                reg_result,
                n_result_col,
                r1,
                P4Value::Dynamic(p_dest.z_aff_sdst.clone()),
                n_result_col as i8,
            );
            vdbe_add_op4_int(&mut vb, OP_IDXINSERT as i32, i_parm, r1, reg_result, n_result_col);
            drop(vb);
            release_temp_reg(p_parse, r1);
        }
    } else if e_dest == SRT_EXISTS {
        // Se existe alguma linha no resultado, registra o fato e aborta. O LIMIT termina o laço.
        vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 1, i_parm);
    } else if e_dest == SRT_MEM {
        // SELECT escalar que faz parte de uma expressão: guarda o resultado na célula (ou
        // arranjo de células) apropriada e sai do laço de varredura.
        if let Some(s) = p_sort.as_deref_mut() {
            debug_assert!(n_result_col <= p_dest.n_sdst);
            push_onto_sorter(p_parse, s, p, reg_result, reg_orig, n_result_col, n_prefix_reg);
        } else {
            debug_assert!(n_result_col == p_dest.n_sdst);
            debug_assert!(reg_result == i_parm);
            // O LIMIT salta para fora do laço por nós.
        }
    } else if e_dest == SRT_COROUTINE || e_dest == SRT_OUTPUT {
        if let Some(s) = p_sort.as_deref_mut() {
            push_onto_sorter(p_parse, s, p, reg_result, reg_orig, n_result_col, n_prefix_reg);
        } else if e_dest == SRT_COROUTINE {
            vdbe_add_op1(&mut v.borrow_mut(), OP_YIELD as i32, p_dest.i_sdparm);
        } else {
            vdbe_add_op2(&mut v.borrow_mut(), OP_RESULTROW as i32, reg_result, n_result_col);
        }
    } else if e_dest == SRT_DISTQUEUE || e_dest == SRT_QUEUE {
        // Escreve os resultados numa fila de prioridade ordenada por `p_dest.p_order_by` (pSO).
        // `i_parm` é o cursor de um índice com pSO.n_expr+2 colunas. A chave usa pSO nas
        // primeiras pSO.n_expr colunas e um OP_Sequence final garante chaves únicas. A última
        // coluna é o registro como blob.
        let mut addr_test = 0;
        let n_key = p_dest.p_order_by.as_ref().expect("fila sem ORDER BY").n_expr;
        let r1 = get_temp_reg(p_parse);
        let r2 = get_temp_range(p_parse, n_key + 2);
        let r3 = r2 + n_key + 1;
        {
            let mut vb = v.borrow_mut();
            if e_dest == SRT_DISTQUEUE {
                // O cursor (i_parm+1) está aberto sobre um segundo índice efêmero com todos os
                // valores já adicionados à fila.
                addr_test = vdbe_add_op4_int(
                    &mut vb,
                    OP_FOUND as i32,
                    i_parm + 1,
                    0,
                    reg_result,
                    n_result_col,
                );
            }
            vdbe_add_op3(&mut vb, OP_MAKERECORD as i32, reg_result, n_result_col, r3);
            if e_dest == SRT_DISTQUEUE {
                vdbe_add_op2(&mut vb, OP_IDXINSERT as i32, i_parm + 1, r3);
                vdbe_change_p5(&mut vb, OPFLAG_USESEEKRESULT as u16);
            }
            let p_so = p_dest.p_order_by.as_ref().unwrap();
            for i in 0..n_key {
                vdbe_add_op2(
                    &mut vb,
                    OP_SCOPY as i32,
                    reg_result + item_order_by_col(&p_so.a[i as usize]) - 1,
                    r2 + i,
                );
            }
            vdbe_add_op2(&mut vb, OP_SEQUENCE as i32, i_parm, r2 + n_key);
            vdbe_add_op3(&mut vb, OP_MAKERECORD as i32, r2, n_key + 2, r1);
            vdbe_add_op4_int(&mut vb, OP_IDXINSERT as i32, i_parm, r1, r2, n_key + 2);
            if addr_test != 0 {
                vdbe_jump_here(&mut vb, addr_test);
            }
        }
        release_temp_reg(p_parse, r1);
        release_temp_range(p_parse, r2, n_key + 2);
    } else {
        // Descarta os resultados. Usado em SELECTs dentro do corpo de um TRIGGER, cujo objetivo
        // é chamar funções do usuário com efeitos colaterais.
        debug_assert!(e_dest == SRT_DISCARD);
    }

    // Salta para o fim do laço se o LIMIT foi atingido, exceto se há sorter, caso em que o
    // sorter já limitou a saída.
    if p_sort.is_none() && p.i_limit != 0 {
        vdbe_add_op2(&mut v.borrow_mut(), OP_DECRJUMPZERO as i32, p.i_limit, i_break);
    }
}


// ---- part_003.rs ----

/// Aloca um objeto KeyInfo suficiente para um índice de `n` colunas de chave e `x` colunas
/// extras. A contagem de referências do C é o `Rc`; `n_ref` acompanha o número lógico. A falta de
/// memória do C (`sqlite3OomFault`) não existe aqui: a alocação de `Vec` não falha de forma
/// recuperável.
pub fn key_info_alloc(db: &Sqlite3Ref, n: i32, x: i32) -> Option<KeyInfoRef> {
    let n_all = (n + x) as usize;
    let enc_value = enc(&db.borrow());
    let p = KeyInfo {
        n_ref: 1,
        enc: enc_value,
        n_key_field: n as u16,
        n_all_field: (n + x) as u16,
        db: Rc::downgrade(db),
        a_sort_flags: vec![0; n_all],
        a_coll: vec![None; n_all],
    };
    Some(Rc::new(RefCell::new(p)))
}

/// Libera uma referência a um objeto KeyInfo. O objeto some quando a última referência cai.
pub fn key_info_unref(p: Option<KeyInfoRef>) {
    if let Some(p) = p {
        {
            let mut ki = p.borrow_mut();
            debug_assert!(ki.db.upgrade().is_some());
            debug_assert!(ki.n_ref > 0);
            ki.n_ref -= 1;
        }
        // Ao sair de escopo, o Rc libera o objeto se era a última referência.
        drop(p);
    }
}

/// Cria um novo ponteiro para um objeto KeyInfo.
pub fn key_info_ref(p: Option<&KeyInfoRef>) -> Option<KeyInfoRef> {
    p.map(|p| {
        debug_assert!(p.borrow().n_ref > 0);
        p.borrow_mut().n_ref += 1;
        p.clone()
    })
}

/// Devolve verdadeiro se um KeyInfo pode ser alterado, o que só vale com uma única referência.
/// Usado apenas em `debug_assert!` (no C, sob SQLITE_DEBUG).
pub fn key_info_is_writeable(p: &KeyInfo) -> bool {
    p.n_ref == 1
}

/// Dada uma lista de expressões, gera um KeyInfo que registra a sequência de colação de cada
/// expressão da lista.
///
/// Se a ExprList é uma cláusula ORDER BY ou GROUP BY, o KeyInfo serve para inicializar um índice
/// virtual que implementa a cláusula. Se é o conjunto de resultados de um SELECT, serve para
/// inicializar um índice virtual que implementa um teste DISTINCT.
///
/// O chamador é responsável por garantir que a estrutura seja liberada.
pub fn key_info_from_expr_list(
    p_parse: &mut Parse,    // Contexto de parsing
    p_list: &ExprList,      // Forma o KeyInfo a partir desta ExprList
    i_start: i32,           // Começa nesta coluna de p_list
    n_extra: i32,           // Acrescenta este número de colunas extras no fim
) -> Option<KeyInfoRef> {
    let db = p_parse
        .db
        .upgrade()
        .expect("a conexão precisa estar viva enquanto o Parse existe");
    let n_expr = p_list.n_expr;
    let p_info = key_info_alloc(&db, n_expr - i_start, n_extra + 1)?;
    {
        let mut info = p_info.borrow_mut();
        debug_assert!(key_info_is_writeable(&info));
        for i in i_start..n_expr {
            let p_item = &p_list.a[i as usize];
            info.a_coll[(i - i_start) as usize] = Some(expr_nn_coll_seq(
                p_parse,
                p_item
                    .p_expr
                    .as_deref()
                    .expect("termo da lista sem expressão"),
            ));
            info.a_sort_flags[(i - i_start) as usize] = p_item.fg.sort_flags;
        }
    }
    Some(p_info)
}

/// Nome do operador de conexão, usado em mensagens de erro.
pub fn select_op_name(id: i32) -> &'static [u8] {
    if id == TK_ALL as i32 {
        b"UNION ALL"
    } else if id == TK_INTERSECT as i32 {
        b"INTERSECT"
    } else if id == TK_EXCEPT as i32 {
        b"EXCEPT"
    } else {
        b"UNION"
    }
}

/// A menos que um comando "EXPLAIN QUERY PLAN" esteja sendo processado, esta função não faz
/// nada. Senão, acrescenta uma linha à saída do EQP com a legenda "USE TEMP B-TREE FOR xxx", onde
/// xxx é "DISTINCT", "ORDER BY" ou "GROUP BY", conforme `z_usage`.
fn explain_temp_table(p_parse: &mut Parse, z_usage: &[u8]) {
    let z_msg = [&b"USE TEMP B-TREE FOR "[..], z_usage].concat();
    vdbe_explain(p_parse, 0, z_msg);
}

/// Se o laço interno foi gerado com um `p_order_by` não nulo, os resultados foram para um sorter.
/// Depois do fim do laço é preciso rodar o sorter e emitir os resultados; esta rotina gera o
/// código para isso.
fn generate_sort_tail(
    p_parse: &mut Parse,        // Contexto de parsing
    p: &Select,                 // O SELECT
    p_sort: &SortCtx,           // Informação sobre a cláusula ORDER BY
    n_column: i32,              // Número de colunas de dados
    p_dest: &SelectDest,        // Escreve os resultados ordenados aqui
) {
    let mut n_column = n_column;
    let v = p_parse
        .p_vdbe
        .clone()
        .expect("o Parse precisa ter um Vdbe em construção");
    let addr_break = p_sort.label_done;
    let addr_continue = vdbe_make_label(p_parse);
    let mut addr_once = 0;
    let p_order_by = p_sort
        .p_order_by
        .as_ref()
        .expect("o sorter precisa ter a lista ORDER BY");
    let e_dest = p_dest.e_dest;
    let i_parm = p_dest.i_sdparm;
    let reg_row: i32;
    let reg_rowid: i32;
    let n_ref_key = 0; // sem SQLITE_ENABLE_SORTER_REFERENCES
    let a_out_ex = &p
        .p_elist
        .as_ref()
        .expect("o Select precisa ter a lista de resultado")
        .a;

    // Número de colunas-chave no registro do sorter.
    let n_key = p_order_by.n_expr - p_sort.n_ob_sat;
    if p_sort.n_ob_sat == 0 || n_key == 1 {
        let z_msg = [
            &b"USE TEMP B-TREE FOR "[..],
            if p_sort.n_ob_sat != 0 { &b"LAST TERM OF "[..] } else { &b""[..] },
            &b"ORDER BY"[..],
        ]
        .concat();
        vdbe_explain(p_parse, 0, z_msg);
    } else {
        let z_msg = format!("USE TEMP B-TREE FOR LAST {} TERMS OF ORDER BY", n_key).into_bytes();
        vdbe_explain(p_parse, 0, z_msg);
    }

    debug_assert!(addr_break < 0);
    if p_sort.label_bk_out != 0 {
        let mut vb = v.borrow_mut();
        vdbe_add_op2(&mut vb, OP_GOSUB as i32, p_sort.reg_return, p_sort.label_bk_out);
        vdbe_goto(&mut vb, addr_break);
        vdbe_resolve_label(&mut vb, p_sort.label_bk_out);
    }

    // Cursor do sorter de onde ler.
    let i_tab = p_sort.i_e_cursor;
    if e_dest == SRT_OUTPUT || e_dest == SRT_COROUTINE || e_dest == SRT_MEM {
        if e_dest == SRT_MEM && p.i_offset != 0 {
            vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, p_dest.i_sdst);
        }
        reg_rowid = 0;
        reg_row = p_dest.i_sdst;
    } else {
        reg_rowid = get_temp_reg(p_parse);
        if e_dest == SRT_EPHEMTAB || e_dest == SRT_TABLE {
            reg_row = get_temp_reg(p_parse);
            n_column = 0;
        } else {
            reg_row = get_temp_range(p_parse, n_column);
        }
    }
    let addr: i32;
    let i_sort_tab: i32;
    let b_seq: i32; // Verdadeiro se o registro do sorter inclui o número de sequência.
    if (p_sort.sort_flags & SORTFLAG_USESORTER) != 0 {
        p_parse.n_mem += 1;
        let reg_sort_out = p_parse.n_mem;
        i_sort_tab = p_parse.n_tab;
        p_parse.n_tab += 1;
        let mut vb = v.borrow_mut();
        if p_sort.label_bk_out != 0 {
            addr_once = vdbe_add_op0(&mut vb, OP_ONCE as i32);
        }
        vdbe_add_op3(
            &mut vb,
            OP_OPENPSEUDO as i32,
            i_sort_tab,
            reg_sort_out,
            n_key + 1 + n_column + n_ref_key,
        );
        if addr_once != 0 {
            vdbe_jump_here(&mut vb, addr_once);
        }
        addr = 1 + vdbe_add_op2(&mut vb, OP_SORTERSORT as i32, i_tab, addr_break);
        debug_assert!(p.i_limit == 0 && p.i_offset == 0);
        vdbe_add_op3(&mut vb, OP_SORTERDATA as i32, i_tab, reg_sort_out, i_sort_tab);
        b_seq = 0;
    } else {
        addr = 1 + vdbe_add_op2(&mut v.borrow_mut(), OP_SORT as i32, i_tab, addr_break);
        code_offset(&v, p.i_offset, addr_continue);
        i_sort_tab = i_tab;
        b_seq = 1;
        if p.i_offset > 0 {
            vdbe_add_op2(&mut v.borrow_mut(), OP_ADDIMM as i32, p.i_limit, -1);
        }
    }
    let mut i_col = n_key + b_seq - 1;
    for i in 0..n_column {
        if item_order_by_col(&a_out_ex[i as usize]) == 0 {
            i_col += 1;
        }
    }
    for i in (0..n_column).rev() {
        let i_read = if item_order_by_col(&a_out_ex[i as usize]) != 0 {
            item_order_by_col(&a_out_ex[i as usize]) - 1
        } else {
            let r = i_col;
            i_col -= 1;
            r
        };
        vdbe_add_op3(
            &mut v.borrow_mut(),
            OP_COLUMN as i32,
            i_sort_tab,
            i_read,
            reg_row + i,
        );
    }
    if e_dest == SRT_TABLE || e_dest == SRT_EPHEMTAB {
        let mut vb = v.borrow_mut();
        vdbe_add_op3(&mut vb, OP_COLUMN as i32, i_sort_tab, n_key + b_seq, reg_row);
        vdbe_add_op2(&mut vb, OP_NEWROWID as i32, i_parm, reg_rowid);
        vdbe_add_op3(&mut vb, OP_INSERT as i32, i_parm, reg_row, reg_rowid);
        vdbe_change_p5(&mut vb, OPFLAG_APPEND as u16);
    } else if e_dest == SRT_SET {
        debug_assert!(n_column == strlen30_nn(&p_dest.z_aff_sdst));
        let mut vb = v.borrow_mut();
        vdbe_add_op4(
            &mut vb,
            OP_MAKERECORD as i32,
            reg_row,
            n_column,
            reg_rowid,
            P4Value::Dynamic(p_dest.z_aff_sdst.clone()),
            n_column as i8,
        );
        vdbe_add_op4_int(&mut vb, OP_IDXINSERT as i32, i_parm, reg_rowid, reg_row, n_column);
    } else if e_dest == SRT_MEM {
        // O LIMIT termina o laço por nós.
    } else if e_dest == SRT_UPFROM {
        let i2 = p_dest.i_sdparm2;
        let r1 = get_temp_reg(p_parse);
        let neg = (i2 < 0) as i32;
        let mut vb = v.borrow_mut();
        vdbe_add_op3(&mut vb, OP_MAKERECORD as i32, reg_row + neg, n_column - neg, r1);
        if i2 < 0 {
            vdbe_add_op3(&mut vb, OP_INSERT as i32, i_parm, r1, reg_row);
        } else {
            vdbe_add_op4_int(&mut vb, OP_IDXINSERT as i32, i_parm, r1, reg_row, i2);
        }
    } else {
        debug_assert!(e_dest == SRT_OUTPUT || e_dest == SRT_COROUTINE);
        if e_dest == SRT_OUTPUT {
            vdbe_add_op2(&mut v.borrow_mut(), OP_RESULTROW as i32, p_dest.i_sdst, n_column);
        } else {
            vdbe_add_op1(&mut v.borrow_mut(), OP_YIELD as i32, p_dest.i_sdparm);
        }
    }
    if reg_rowid != 0 {
        if e_dest == SRT_SET {
            release_temp_range(p_parse, reg_row, n_column);
        } else {
            release_temp_reg(p_parse, reg_row);
        }
        release_temp_reg(p_parse, reg_rowid);
    }
    // O fim do laço.
    let mut vb = v.borrow_mut();
    vdbe_resolve_label(&mut vb, addr_continue);
    if (p_sort.sort_flags & SORTFLAG_USESORTER) != 0 {
        vdbe_add_op2(&mut vb, OP_SORTERNEXT as i32, i_tab, addr);
    } else {
        vdbe_add_op2(&mut vb, OP_NEXT as i32, i_tab, addr);
    }
    if p_sort.reg_return != 0 {
        vdbe_add_op1(&mut vb, OP_RETURN as i32, p_sort.reg_return);
    }
    vdbe_resolve_label(&mut vb, addr_break);
}


// ---- part_004.rs ----

/// No SQLite 3.46.1 do Debian `ViewCanHaveRowid` é a constante 0 (sem SQLITE_ALLOW_ROWID_IN_VIEW).
const VIEW_CAN_HAVE_ROWID: bool = false;

/// Devolve o nome de uma coluna (o `zCnName` do C é uma string terminada em zero que também
/// pode carregar o tipo e a collation depois do primeiro zero; aqui para no primeiro zero).
pub fn column_cn_name(p_col: &Column) -> &[u8] {
    let n = p_col.z_cn_name.iter().position(|&c| c == 0).unwrap_or(p_col.z_cn_name.len());
    &p_col.z_cn_name[..n]
}

/// Um nível da cadeia de NameContext: o `pSrcList` do contexto e a conexão (`pParse->db`). O `pNext` do C vira
/// a posição seguinte na fatia (o contexto mais interno vem primeiro). No C o NameContext de
/// `columnTypeImpl` é sempre montado na pilha e aponta para o pai, então uma fatia de níveis
/// emprestados o representa sem precisar de dono para os níveis.
#[derive(Clone)]
pub struct NcLevel<'a> {
    pub p_src_list: Option<&'a SrcList>,
    pub db: Option<Sqlite3Ref>,
}

/// Resultado de `column_type_impl`: o tipo declarado e, com SQLITE_ENABLE_COLUMN_METADATA, o
/// banco, a tabela e a coluna de origem.
#[derive(Default, Clone)]
pub struct ColumnTypeInfo {
    pub z_type: Option<Vec<u8>>,
    pub z_orig_db: Option<Vec<u8>>,
    pub z_orig_tab: Option<Vec<u8>>,
    pub z_orig_col: Option<Vec<u8>>,
}

/// Devolve o 'tipo declarado' da expressão `p_expr`.
///
/// O tipo declarado é a definição exata do tipo extraída do CREATE TABLE original se a expressão
/// for uma coluna. O tipo declarado de um ROWID é INTEGER. Quando uma expressão conta como coluna
/// pode ser complexo na presença de subconsultas. O tipo declarado de qualquer expressão que não
/// seja coluna é NULL.
///
/// A versão do Debian tem COLUMN_METADATA, então devolve também banco, tabela e coluna de origem.
/// `chain[0]` é o NameContext recebido e `chain[1..]` a cadeia de `pNext`.
pub fn column_type_chain(chain: &[NcLevel<'_>], p_expr: &Expr) -> ColumnTypeInfo {
    let mut info = ColumnTypeInfo::default();

    debug_assert!(!chain.is_empty());
    debug_assert!(chain[0].p_src_list.is_some());
    match p_expr.op {
        TK_COLUMN => {
            // A expressão é uma coluna. Localiza a tabela de onde a coluna é extraída em
            // NameContext.pSrcList. Pode ser uma tabela real ou uma subconsulta.
            let mut p_tab: Option<TableRef> = None;
            let mut p_s: Option<&Select> = None;
            let mut i_col = p_expr.i_column as i32;
            // Posição em `chain` do NameContext em que a tabela foi achada (o pNC do C).
            let mut k = 0usize;
            while k < chain.len() && p_tab.is_none() {
                let p_tab_list = chain[k].p_src_list.expect("NameContext sem pSrcList");
                let mut j = 0usize;
                while j < p_tab_list.n_src as usize && p_tab_list.a[j].i_cursor != p_expr.i_table {
                    j += 1;
                }
                if j < p_tab_list.n_src as usize {
                    p_tab = p_tab_list.a[j].p_tab.clone();
                    p_s = p_tab_list.a[j].p_select.as_deref();
                } else {
                    k += 1;
                }
            }

            let p_tab = match p_tab {
                Some(t) => t,
                None => {
                    // Antigamente código como "SELECT new.x" dentro de um trigger chegava aqui.
                    // Depois da reestruturação da geração de código de trigger a condição não
                    // ocorre mais, mas ainda é verdadeira para instruções como:
                    //
                    //   CREATE TABLE t1(col INTEGER);
                    //   SELECT (SELECT t1.col) FROM FROM t1;
                    //
                    // quando columnType() é chamado na expressão "t1.col" da subconsulta. Nesse
                    // caso o tipo da coluna fica NULL, embora devesse ser "INTEGER". Isso não é
                    // problema: o tipo de "t1.col" nunca é usado. Quando columnType() é chamado
                    // em "(SELECT t1.col)", o tipo correto é devolvido (ramo TK_SELECT abaixo).
                    return info;
                }
            };

            if let Some(p_s) = p_s {
                // A "tabela" é na verdade uma subconsulta ou uma view na cláusula FROM do
                // SELECT. Devolve o tipo declarado e os dados de origem da coluna de resultado
                // da subconsulta.
                let p_elist = p_s.p_elist.as_ref().expect("Select sem pEList");
                if i_col < p_elist.n_expr && (!VIEW_CAN_HAVE_ROWID || i_col >= 0) {
                    // Se iCol é menor que zero, a expressão pede o rowid da subconsulta ou
                    // view. Isso é legal (caso de teste misc2.2.2): sempre vale NULL.
                    if let Some(p) = p_elist.a.get(i_col as usize).and_then(|it| it.p_expr.as_deref()) {
                        let mut s_chain: Vec<NcLevel<'_>> = Vec::with_capacity(chain.len() - k + 1);
                        s_chain.push(NcLevel {
                            p_src_list: p_s.p_src.as_deref(),
                            db: chain[k].db.clone(),
                        });
                        s_chain.extend(chain[k..].iter().cloned());
                        info = column_type_chain(&s_chain, p);
                    }
                }
            } else {
                // Uma tabela real ou uma tabela CTE.
                let tab = p_tab.borrow();
                if i_col < 0 {
                    i_col = tab.i_p_key as i32;
                }
                debug_assert!(i_col == XN_ROWID || (i_col >= 0 && i_col < tab.n_col as i32));
                if i_col < 0 {
                    info.z_type = Some(b"INTEGER".to_vec());
                    info.z_orig_col = Some(b"rowid".to_vec());
                } else {
                    let p_col = &tab.a_col[i_col as usize];
                    info.z_orig_col = Some(column_cn_name(p_col).to_vec());
                    info.z_type = column_type(p_col, None).map(|z| z.to_vec());
                }
                info.z_orig_tab = Some(tab.z_name.clone());
                if let (Some(db), Some(w_schema)) = (chain[k].db.as_ref(), tab.p_schema.as_ref()) {
                    if let Some(schema) = w_schema.upgrade() {
                        let db_b = db.borrow();
                        let i_db = schema_to_index(&db_b, Some(&schema.borrow()));
                        info.z_orig_db = db_b.a_db[i_db as usize].z_db_sname.clone();
                    }
                }
            }
        }
        TK_SELECT => {
            // A expressão é uma subconsulta. Devolve o tipo declarado e a informação de origem
            // da única coluna do resultado do SELECT.
            debug_assert!(expr_use_x_select(p_expr));
            let p_s = p_expr.x.p_select.as_ref().expect("TK_SELECT sem pSelect");
            let p = p_s.p_elist.as_ref().expect("Select sem pEList").a[0]
                .p_expr
                .as_deref()
                .expect("item sem expressão");
            let mut s_chain: Vec<NcLevel<'_>> = Vec::with_capacity(chain.len() + 1);
            s_chain.push(NcLevel {
                p_src_list: p_s.p_src.as_deref(),
                db: chain[0].db.clone(),
            });
            s_chain.extend(chain.iter().cloned());
            info = column_type_chain(&s_chain, p);
        }
        _ => {}
    }

    info
}

/// Forma usada pelos chamadores: um NameContext com `pSrcList` e `pParse`, sem `pNext`.
pub fn column_type_impl(
    p_src_list: Option<&SrcList>,
    db: Option<&Sqlite3Ref>,
    p_expr: &Expr,
) -> ColumnTypeInfo {
    let chain = [NcLevel { p_src_list, db: db.cloned() }];
    column_type_chain(&chain, p_expr)
}

/// Gera código que informa ao VDBE os tipos declarados das colunas do resultado.
pub fn generate_column_types(p_parse: &mut Parse, p_tab_list: &SrcList, p_elist: &ExprList) {
    // SQLITE_OMIT_DECLTYPE não está definido.
    let v = p_parse.p_vdbe.clone().expect("Parse sem Vdbe");
    let db = p_parse.db.upgrade();
    for i in 0..p_elist.n_expr {
        let p = p_elist.a[i as usize].p_expr.as_deref().expect("item sem expressão");
        let info = column_type_impl(Some(p_tab_list), db.as_ref(), p);

        // O vdbe precisa fazer a própria cópia do tipo da coluna e das outras strings
        // específicas da coluna, caso o schema seja reiniciado antes de esta máquina virtual
        // ser apagada.
        let mut vm = v.borrow_mut();
        vdbe_set_col_name(&mut vm, i, COLNAME_DATABASE, info.z_orig_db.as_deref(), SQLITE_TRANSIENT);
        vdbe_set_col_name(&mut vm, i, COLNAME_TABLE, info.z_orig_tab.as_deref(), SQLITE_TRANSIENT);
        vdbe_set_col_name(&mut vm, i, COLNAME_COLUMN, info.z_orig_col.as_deref(), SQLITE_TRANSIENT);
        vdbe_set_col_name(&mut vm, i, COLNAME_DECLTYPE, info.z_type.as_deref(), SQLITE_TRANSIENT);
    }
}

/// Calcula os nomes das colunas de um SELECT.
///
/// A única garantia do SQLite sobre nomes de coluna é que, se a coluna tem uma cláusula AS que
/// dá um nome, esse nome é usado. Mesmo assim, incontáveis aplicações assumiram coisas sobre os
/// nomes e quebram se elas mudarem. Use extremo cuidado ao modificar esta rotina.
///
/// Veja também: `columns_from_expr_list()`.
///
/// Os PRAGMAs short_column_names e full_column_names são obsoletos. O padrão é short=ON,
/// full=OFF:
///
///    short=OFF, full=OFF: o nome é o texto da expressão como aparece no SELECT (o zSpan).
///    short=ON, full=OFF: (padrão) se o resultado é uma coluna de tabela, o nome é só COLUMN;
///                        senão usa o zSpan.
///    full=ON, short=QUALQUER: se o resultado é uma coluna de tabela, TABLE.COLUMN; senão zSpan.
pub fn generate_column_names(p_parse: &mut Parse, p_select: &Select) {
    if p_parse.col_names_set != 0 {
        return;
    }
    let v = p_parse.p_vdbe.clone().expect("Parse sem Vdbe");
    let db = p_parse.db.upgrade().expect("Parse sem conexão");
    // Os nomes das colunas vêm do termo mais à esquerda de um SELECT composto.
    let mut p_select = p_select;
    while let Some(prior) = p_select.p_prior.as_deref() {
        p_select = prior;
    }
    let p_tab_list = p_select.p_src.as_deref().expect("Select sem pSrc");
    let p_elist = p_select.p_elist.as_ref().expect("Select sem pEList");
    p_parse.col_names_set = 1;
    let full_name = (db.borrow().flags & SQLITE_FULL_COL_NAMES) != 0;
    let src_name = (db.borrow().flags & SQLITE_SHORT_COL_NAMES) != 0 || full_name;
    vdbe_set_num_cols(&mut v.borrow_mut(), p_elist.n_expr);
    for i in 0..p_elist.n_expr {
        let item = &p_elist.a[i as usize];
        let p = item.p_expr.as_deref().expect("item sem expressão");

        debug_assert!(p.op != TK_AGG_COLUMN); // o processamento de agregados ainda não rodou
        debug_assert!(p.op != TK_COLUMN || (expr_use_y_tab(p) && p.y.p_tab.is_some()));
        if item.z_e_name.is_some() && item.fg.e_e_name == ENAME_NAME {
            // Uma cláusula AS sempre tem a primeira prioridade.
            let z_name = item.z_e_name.as_deref();
            vdbe_set_col_name(&mut v.borrow_mut(), i, COLNAME_NAME, z_name, SQLITE_TRANSIENT);
        } else if src_name && p.op == TK_COLUMN {
            let p_tab = p.y.p_tab.clone().expect("TK_COLUMN sem tabela");
            let tab = p_tab.borrow();
            let mut i_col = p.i_column as i32;
            if i_col < 0 {
                i_col = tab.i_p_key as i32;
            }
            debug_assert!(i_col == -1 || (i_col >= 0 && i_col < tab.n_col as i32));
            let z_col: Vec<u8> = if i_col < 0 {
                b"rowid".to_vec()
            } else {
                column_cn_name(&tab.a_col[i_col as usize]).to_vec()
            };
            if full_name {
                // "%s.%s" com o nome da tabela e o da coluna.
                let mut z_name = tab.z_name.clone();
                z_name.push(b'.');
                z_name.extend_from_slice(&z_col);
                vdbe_set_col_name(&mut v.borrow_mut(), i, COLNAME_NAME, Some(&z_name), SQLITE_TRANSIENT);
            } else {
                vdbe_set_col_name(&mut v.borrow_mut(), i, COLNAME_NAME, Some(&z_col), SQLITE_TRANSIENT);
            }
        } else {
            let z: Vec<u8> = match item.z_e_name.as_deref() {
                None => format!("column{}", i + 1).into_bytes(),
                Some(z) => z.to_vec(),
            };
            vdbe_set_col_name(&mut v.borrow_mut(), i, COLNAME_NAME, Some(&z), SQLITE_TRANSIENT);
        }
    }
    generate_column_types(p_parse, p_tab_list, p_elist);
}

/// Dada uma lista de expressões (na verdade a lista de expressões que forma o resultado de um
/// SELECT), calcula nomes de coluna apropriados para uma tabela que guardaria a lista.
///
/// Todos os nomes de coluna são únicos.
///
/// Só os nomes são calculados. `Column.zType`, `Column.zColl` e os outros campos de Column ficam
/// zerados.
///
/// Devolve SQLITE_OK em sucesso. Em erro, grava lista vazia em `pa_col` e 0 em `pn_col` e
/// devolve o código de erro de `p_parse`.
///
/// Veja também: `generate_column_names()`.
pub fn columns_from_expr_list(
    p_parse: &mut Parse,
    p_elist: Option<&ExprList>,
    pn_col: &mut i16,
    pa_col: &mut Vec<Column>,
) -> i32 {
    // Tabela hash dos nomes de coluna: a chave é o nome sem distinção de maiúsculas (o hash do
    // SQLite compara com sqlite3StrICmp) e o valor é `fg.bUsingTerm` do item que a inseriu, o
    // único campo de `pCollide` que o laço lê.
    let mut ht: std::collections::HashMap<Vec<u8>, bool> = std::collections::HashMap::new();

    let mut n_col: i32;
    let mut a_col: Vec<Column>;
    if let Some(el) = p_elist {
        n_col = el.n_expr;
        a_col = vec![Column::default(); n_col as usize];
        if never(n_col > 32767) {
            n_col = 32767;
        }
    } else {
        n_col = 0;
        a_col = Vec::new();
    }
    debug_assert!(n_col == (n_col as i16) as i32);
    *pn_col = n_col as i16;

    let mut i: i32 = 0;
    while i < n_col && p_parse.n_err == 0 {
        let p_x = &p_elist.expect("lista ausente com n_col > 0").a[i as usize];
        // Obtém um nome apropriado para a coluna.
        let mut z_name: Option<Vec<u8>> = p_x.z_e_name.clone();
        if z_name.is_some() && p_x.fg.e_e_name == ENAME_NAME {
            // Se a coluna contém uma frase "AS <nome>", usa <nome> como nome.
        } else {
            let mut p_col_expr = expr_skip_collate_and_likely(p_x.p_expr.as_deref());
            while let Some(e) = p_col_expr {
                if e.op != TK_DOT {
                    break;
                }
                p_col_expr = e.p_right.as_deref();
                debug_assert!(p_col_expr.is_some());
            }
            let p_col_expr = p_col_expr.expect("expressão ausente");
            if p_col_expr.op == TK_COLUMN && always(expr_use_y_tab(p_col_expr)) && always(p_col_expr.y.p_tab.is_some())
            {
                // Para colunas usa o nome da coluna.
                let mut i_col = p_col_expr.i_column as i32;
                let p_tab = p_col_expr.y.p_tab.clone().expect("TK_COLUMN sem tabela");
                let tab = p_tab.borrow();
                if i_col < 0 {
                    i_col = tab.i_p_key as i32;
                }
                z_name = Some(if i_col >= 0 {
                    column_cn_name(&tab.a_col[i_col as usize]).to_vec()
                } else {
                    b"rowid".to_vec()
                });
            } else if p_col_expr.op == TK_ID {
                debug_assert!(!expr_has_property(p_col_expr, EP_INT_VALUE));
                z_name = p_col_expr.u.z_token.clone();
            } else {
                // Usa o texto original da expressão da coluna como nome (z_name continua sendo
                // o `zEName` do item).
            }
        }
        let mut z_name: Vec<u8> = match z_name {
            Some(z) if !is_true_or_false(&z) => z,
            _ => format!("column{}", i + 1).into_bytes(),
        };

        // Garante que o nome da coluna é único. Se não for, acrescenta um inteiro ao nome para
        // que ele fique único.
        let mut cnt: u32 = 0;
        while let Some(&using_term) = ht.get(&z_name.to_ascii_lowercase()) {
            if using_term {
                a_col[i as usize].col_flags |= COLFLAG_NOEXPAND;
            }
            let mut n_name = z_name.len();
            if n_name > 0 {
                let mut j = n_name - 1;
                while j > 0 && isdigit(z_name[j]) {
                    j -= 1;
                }
                if z_name[j] == b':' {
                    n_name = j;
                }
            }
            // "%.*z:%u" com n_name, z_name e ++cnt.
            cnt = cnt.wrapping_add(1);
            let mut z_new = z_name[..n_name].to_vec();
            z_new.push(b':');
            z_new.extend_from_slice(cnt.to_string().as_bytes());
            z_name = z_new;
            progress_check(p_parse);
            if cnt > 3 {
                let mut buf = [0u8; 4];
                randomness(4, &mut buf);
                cnt = u32::from_ne_bytes(buf);
            }
        }
        let p_col = &mut a_col[i as usize];
        p_col.h_name = str_i_hash(Some(&z_name));
        p_col.z_cn_name = z_name.clone();
        if p_x.fg.b_no_expand != 0 {
            p_col.col_flags |= COLFLAG_NOEXPAND;
        }
        column_properties_from_name(None, p_col);
        // O sqlite3OomFault do C (inserção na hash devolvendo o próprio dado) não existe aqui.
        ht.insert(z_name.to_ascii_lowercase(), p_x.fg.b_using_term != 0);
        i += 1;
    }
    *pa_col = a_col;
    if p_parse.n_err != 0 {
        pa_col.clear();
        *pn_col = 0;
        return p_parse.rc;
    }
    SQLITE_OK
}


// ---- part_005.rs ----

/// pTab é um objeto Table transitório que representa uma subconsulta de algum tipo (talvez uma
/// subconsulta entre parênteses na cláusula FROM de uma consulta maior, ou uma VIEW, ou uma CTE).
/// Esta rotina calcula informações de tipo para esse objeto Table com base no objeto Select que
/// implementa a subconsulta. Para fins desta rotina, "informações de tipo" significa:
///
///    * O nome do tipo de dado, como poderia aparecer em uma instrução CREATE TABLE
///    * Qual sequência de colação usar para a coluna
///    * A afinidade da coluna
///
/// Modelo: no C o laço `pS2 = pS2->pNext` anda da esquerda para a direita pela lista composta.
/// Como `p_next` não pode ser dono (já existe o `p_prior` dono), a cadeia é materializada em
/// `sels`: `sels[0]` é o `p_select` recebido e `sels[len-1]` o termo mais à esquerda; "pNext" de
/// `sels[k]` é `sels[k-1]`.
pub fn subquery_column_types(p_parse: &mut Parse, p_tab: &mut Table, p_select: &Select, aff: u8) {
    let db = p_parse.db.upgrade().expect("Parse sem conexão");

    debug_assert!((p_select.sel_flags & SF_RESOLVED) != 0);
    debug_assert!(
        p_tab.n_col as i32 == p_select.p_elist.as_ref().map_or(0, |el| el.n_expr) || p_parse.n_err > 0
    );
    debug_assert!(aff == SQLITE_AFF_NONE || aff == SQLITE_AFF_BLOB);
    if db.borrow().malloc_failed != 0 || p_parse.e_parse_mode >= PARSE_MODE_RENAME {
        return;
    }
    let mut sels: Vec<&Select> = vec![p_select];
    while let Some(prior) = sels[sels.len() - 1].p_prior.as_deref() {
        sels.push(prior);
    }
    let leftmost = sels.len() - 1;
    let p_sel = sels[leftmost];
    let a = &p_sel.p_elist.as_ref().expect("Select sem pEList").a;
    // sNC.pSrcList = pSelect->pSrc; o resto do NameContext fica zerado.
    let s_src_list = p_sel.p_src.as_deref();

    for i in 0..p_tab.n_col as usize {
        let mut m: u32 = 0;
        let mut i_s2 = leftmost;
        p_tab.tab_flags |= (p_tab.a_col[i].col_flags & COLFLAG_NOINSERT) as u32;
        let p: &Expr = a[i].p_expr.as_deref().expect("item sem expressão");
        // pCol->szEst = ... // O tamanho estimado da coluna nunca é usado em tabelas de SELECT
        let mut affinity = expr_affinity(p);
        while affinity <= SQLITE_AFF_NONE && i_s2 > 0 {
            m |= expr_data_type(sels[i_s2].p_elist.as_ref().expect("Select sem pEList").a[i].p_expr.as_deref());
            i_s2 -= 1;
            affinity = expr_affinity(
                sels[i_s2].p_elist.as_ref().expect("Select sem pEList").a[i].p_expr.as_deref().expect("sem expressão"),
            );
        }
        if affinity <= SQLITE_AFF_NONE {
            affinity = aff;
        }
        if affinity >= SQLITE_AFF_TEXT && (i_s2 > 0 || i_s2 != leftmost) {
            // for(pS2=pS2->pNext; pS2; pS2=pS2->pNext) m |= ...
            let mut k = i_s2;
            while k > 0 {
                k -= 1;
                m |= expr_data_type(sels[k].p_elist.as_ref().expect("Select sem pEList").a[i].p_expr.as_deref());
            }
            if affinity == SQLITE_AFF_TEXT && (m & 0x01) != 0 {
                affinity = SQLITE_AFF_BLOB;
            } else if affinity >= SQLITE_AFF_NUMERIC && (m & 0x02) != 0 {
                affinity = SQLITE_AFF_BLOB;
            }
            if affinity >= SQLITE_AFF_NUMERIC && p.op == TK_CAST {
                affinity = SQLITE_AFF_FLEXNUM;
            }
        }
        p_tab.a_col[i].affinity = affinity;
        let mut z_type: Option<Vec<u8>> = column_type_impl(s_src_list, Some(&db), p).z_type;
        if z_type.is_none() || affinity != affinity_type(z_type.as_deref().unwrap(), None) {
            if affinity == SQLITE_AFF_NUMERIC || affinity == SQLITE_AFF_FLEXNUM {
                z_type = Some(b"NUM".to_vec());
            } else {
                z_type = None;
                for j in 1..SQLITE_N_STDTYPE {
                    if SQLITE_STD_TYPE_AFFINITY[j] == affinity {
                        z_type = Some(SQLITE_STD_TYPE[j].to_vec());
                        break;
                    }
                }
            }
        }
        if let Some(z_type) = z_type {
            // zCnName vira "nome\0tipo\0" numa só alocação.
            let p_col = &mut p_tab.a_col[i];
            let mut z_cn_name = column_cn_name(p_col).to_vec();
            z_cn_name.push(0);
            z_cn_name.extend_from_slice(&z_type);
            z_cn_name.push(0);
            p_col.z_cn_name = z_cn_name;
            p_col.col_flags &= !(COLFLAG_HASTYPE | COLFLAG_HASCOLL);
            p_col.col_flags |= COLFLAG_HASTYPE;
        }
        if let Some(p_coll) = expr_coll_seq(p_parse, p) {
            debug_assert!(p_tab.p_index.is_none());
            let z_name = p_coll.borrow().z_name.clone().unwrap_or_default();
            column_set_coll(&db.borrow(), &mut p_tab.a_col[i], &z_name);
        }
    }
    p_tab.sz_tab_row = 1; // Qualquer valor diferente de zero funciona
}

/// Dado um comando SELECT, gera uma estrutura Table que descreve o conjunto de resultados desse
/// SELECT.
pub fn result_set_of_select(p_parse: &mut Parse, p_select: &mut Select, aff: u8) -> Option<Box<Table>> {
    let db = p_parse.db.upgrade().expect("Parse sem conexão");

    let saved_flags = db.borrow().flags;
    db.borrow_mut().flags &= !SQLITE_FULL_COL_NAMES;
    db.borrow_mut().flags |= SQLITE_SHORT_COL_NAMES;
    select_prep(p_parse, p_select, None);
    db.borrow_mut().flags = saved_flags;
    if p_parse.n_err != 0 {
        return None;
    }
    let mut p_select: &Select = p_select;
    while let Some(prior) = p_select.p_prior.as_deref() {
        p_select = prior;
    }
    let mut p_tab = Box::new(Table::default());
    p_tab.n_tab_ref = 1;
    p_tab.z_name = Vec::new();
    p_tab.n_row_log_est = 200;
    debug_assert!(200 == log_est(1048576));
    columns_from_expr_list(p_parse, p_select.p_elist.as_deref(), &mut p_tab.n_col, &mut p_tab.a_col);
    subquery_column_types(p_parse, &mut p_tab, p_select, aff);
    p_tab.i_p_key = -1;
    if db.borrow().malloc_failed != 0 {
        return None;
    }
    Some(p_tab)
}

/// Obtém um VDBE para o contexto de parsing dado. Cria um novo se necessário. Se ocorrer erro,
/// devolve `None` e deixa uma mensagem em `p_parse`.
pub fn get_vdbe(p_parse: &mut Parse) -> Option<VdbeRef> {
    if let Some(v) = p_parse.p_vdbe.as_ref() {
        return Some(v.clone());
    }
    let db = p_parse.db.upgrade().expect("Parse sem conexão");
    if p_parse.p_toplevel.is_none() && optimization_enabled(&db.borrow(), SQLITE_FACTOR_OUT_CONST) {
        p_parse.ok_const_factor = 1;
    }
    vdbe_create(p_parse)
}

/// Calcula os campos iLimit e iOffset do SELECT com base nas expressões pLimit. `pLimit.pLeft` e
/// `pLimit.pRight` guardam as expressões que aparecem no SQL original depois das palavras LIMIT e
/// OFFSET, ou `None` se omitidas. iLimit e iOffset são os números dos registros inteiros usados
/// como contadores para calcular o limite e o deslocamento. Sem limite e/ou deslocamento, iLimit e
/// iOffset são negativos.
///
/// Esta rotina só muda iLimit e iOffset se um limite ou deslocamento está definido por
/// `pLimit.pLeft` e `pLimit.pRight`. iLimit e iOffset devem ter sido pré-definidos com os valores
/// padrão (zero) antes da chamada.
///
/// O registro iOffset (se existir) é inicializado com o valor do OFFSET. O registro iLimit é
/// inicializado com LIMIT. O registro iOffset+1 é inicializado com LIMIT+OFFSET.
///
/// Só se `pLimit.pLeft != 0` os registros de limite são redefinidos. O operador UNION ALL usa essa
/// propriedade para forçar a reutilização dos mesmos registros de limite e deslocamento em vários
/// SELECTs.
fn compute_limit_registers(p_parse: &mut Parse, p: &mut Select, i_break: i32) {
    if p.i_limit != 0 {
        return;
    }
    // O LIMIT sai de `p` durante o cálculo, porque `expr_code` precisa de `p_parse` e `p` ao mesmo
    // tempo; volta no fim.
    let p_limit = p.p_limit.take();

    // "LIMIT -1" sempre mostra todas as linhas. Há controvérsia sobre qual seria o comportamento
    // correto. A implementação atual interpreta "LIMIT 0" como nenhuma linha.
    if let Some(p_limit_e) = p_limit.as_deref() {
        debug_assert!(p_limit_e.op == TK_LIMIT);
        debug_assert!(p_limit_e.p_left.is_some());
        p_parse.n_mem += 1;
        let i_limit = p_parse.n_mem;
        p.i_limit = i_limit;
        let v = get_vdbe(p_parse).expect("sem Vdbe");
        let p_left = p_limit_e.p_left.as_deref().expect("LIMIT sem pLeft");
        let mut n: i32 = 0;
        if expr_is_integer(p_left, &mut n) != 0 {
            vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, n, i_limit);
            if n == 0 {
                vdbe_goto(&mut v.borrow_mut(), i_break);
            } else if n >= 0 && p.n_select_row > log_est(n as u64) {
                p.n_select_row = log_est(n as u64);
                p.sel_flags |= SF_FIXED_LIMIT;
            }
        } else {
            expr_code(p_parse, Some(p_left), i_limit);
            vdbe_add_op1(&mut v.borrow_mut(), OP_MUST_BE_INT as i32, i_limit);
            vdbe_add_op2(&mut v.borrow_mut(), OP_IF_NOT as i32, i_limit, i_break);
        }
        if let Some(p_right) = p_limit_e.p_right.as_deref() {
            p_parse.n_mem += 1;
            let i_offset = p_parse.n_mem;
            p.i_offset = i_offset;
            p_parse.n_mem += 1; // Aloca um registro extra para limit+offset
            expr_code(p_parse, Some(p_right), i_offset);
            vdbe_add_op1(&mut v.borrow_mut(), OP_MUST_BE_INT as i32, i_offset);
            vdbe_add_op3(&mut v.borrow_mut(), OP_OFFSET_LIMIT as i32, i_limit, i_offset + 1, i_offset);
        }
    }
    p.p_limit = p_limit;
}

/// Devolve a sequência de colação apropriada para a coluna `i_col` do conjunto de resultados do
/// SELECT composto "p". Devolve `None` se a coluna não tem colação padrão.
///
/// A colação do SELECT composto vem do termo mais à esquerda que tem colação.
fn multi_select_coll_seq(p_parse: &mut Parse, p: &Select, i_col: i32) -> Option<CollSeqRef> {
    let mut p_ret = match p.p_prior.as_deref() {
        Some(prior) => multi_select_coll_seq(p_parse, prior, i_col),
        None => None,
    };
    debug_assert!(i_col >= 0);
    // iCol deve ser menor que p->pEList->nExpr. Senão um erro teria sido lançado na resolução de
    // nomes e não teríamos chegado até aqui.
    let p_elist = p.p_elist.as_ref().expect("Select sem pEList");
    if p_ret.is_none() && always(i_col < p_elist.n_expr) {
        p_ret = expr_coll_seq(p_parse, p_elist.a[i_col as usize].p_expr.as_deref().expect("sem expressão"));
    }
    p_ret
}

/// O SELECT passado como segundo parâmetro é um SELECT composto com cláusula ORDER BY. Esta função
/// aloca e devolve uma estrutura KeyInfo adequada para implementar o ORDER BY.
///
/// O espaço da KeyInfo vem do malloc. A função chamadora deve garantir que ela seja liberada.
fn multi_select_order_by_key_info(p_parse: &mut Parse, p: &mut Select, n_extra: i32) -> Option<KeyInfoRef> {
    let db = p_parse.db.upgrade().expect("Parse sem conexão");
    // O ORDER BY sai de `p` durante o laço (multi_select_coll_seq só olha pEList e pPrior) e volta
    // no fim; assim cada termo pode ser reescrito com o COLLATE acrescentado.
    let mut p_order_by = p.p_order_by.take();
    let n_order_by = if always(p_order_by.is_some()) { p_order_by.as_ref().unwrap().n_expr } else { 0 };
    let p_ret = key_info_alloc(&db, n_order_by + n_extra, 1);
    if let Some(p_ret) = p_ret.as_ref() {
        for i in 0..n_order_by as usize {
            let p_item = &mut p_order_by.as_mut().unwrap().a[i];
            let p_term = p_item.p_expr.take().expect("ORDER BY sem termo");
            let p_coll: Option<CollSeqRef>;

            if (p_term.flags & EP_COLLATE) != 0 {
                p_coll = expr_coll_seq(p_parse, &p_term);
                p_item.p_expr = Some(p_term);
            } else {
                let i_order_by_col = match p_item.u {
                    ExprListItemU::X { i_order_by_col, .. } => i_order_by_col as i32,
                    _ => 0,
                };
                let mut c = multi_select_coll_seq(p_parse, p, i_order_by_col - 1);
                if c.is_none() {
                    c = db.borrow().p_dflt_coll.clone();
                }
                let z_name = c.as_ref().expect("sem colação padrão").borrow().z_name.clone().unwrap_or_default();
                p_item.p_expr = Some(expr_add_collate_string(p_parse, p_term, &z_name));
                p_coll = c;
            }
            debug_assert!(key_info_is_writeable(&p_ret.borrow()));
            let mut ki = p_ret.borrow_mut();
            ki.a_coll[i] = p_coll;
            ki.a_sort_flags[i] = p_item.fg.sort_flags;
        }
    }
    p.p_order_by = p_order_by;
    p_ret
}

/// Esta rotina gera código VDBE para calcular o conteúdo de uma consulta WITH RECURSIVE da forma:
///
///   <recursive-table> AS (<setup-query> UNION [ALL] <recursive-query>)
///                         \___________/             \_______________/
///                           p->pPrior                      p
///
/// Há exatamente uma referência à recursive-table na cláusula FROM da recursive-query, marcada
/// com `SrcList.a[].fg.isRecursive`.
///
/// A setup-query roda uma vez para gerar um conjunto inicial de linhas que vai para uma tabela
/// Queue. As linhas são extraídas da Queue uma a uma. Cada linha extraída é enviada a pDest. Então
/// a linha extraída (agora na tabela iCurrent) vira o conteúdo da recursive-table numa execução da
/// recursive-query. A saída da recursive-query volta à Queue. Depois outra linha é extraída e a
/// iteração continua até a Queue esvaziar.
///
/// Se o operador composto é UNION, nenhuma linha duplicada entra na Queue. A tabela iDistinct
/// guarda uma cópia de todas as linhas já inseridas na Queue e descarta duplicatas. Com UNION ALL
/// as duplicatas são permitidas.
///
/// Se a consulta tem ORDER BY, as entradas da Queue ficam em ordem de ORDER BY e a primeira é
/// extraída a cada ciclo. Sem ORDER BY a Queue é só uma FIFO.
///
/// Com LIMIT, a iteração para depois de LIMIT linhas enviadas a pDest. LIMIT zero significa
/// nenhuma linha e LIMIT negativo significa todas. Com OFFSET positivo, as primeiras OFFSET saídas
/// são descartadas em vez de enviadas a pDest. A contagem do LIMIT só começa depois de puladas as
/// linhas do OFFSET.
///
/// Modelo: a setup-query (`pFirstRec->pPrior`) sai da cadeia durante a rotina e volta antes de
/// retornar, e o `ORDER BY` de `p` é guardado em variável local, como no C. O `pSetup->pNext = p`
/// do C não é reproduzido porque `Select.p_next` não é dono (veja a nota do integrador).
fn generate_with_recursive_query(p_parse: &mut Parse, p: &mut Select, p_dest: &mut SelectDest) {
    let n_col = p.p_elist.as_ref().expect("Select sem pEList").n_expr; // Colunas da tabela recursiva
    let db = p_parse.db.upgrade().expect("Parse sem conexão");
    let mut i_current = 0; // A tabela Current
    let mut i_distinct = 0; // Para garantir resultados únicos se UNION
    let e_dest: i32; // Como escrever na Queue
    let mut dest_queue = SelectDest::default(); // SelectDest que aponta para a Queue

    if p.p_win.is_some() {
        error_msg(p_parse, b"cannot use window functions in recursive queries", &[]);
        return;
    }

    // Obtém autorização para fazer uma consulta recursiva
    if auth_check(p_parse, SQLITE_RECURSIVE, None, None, None) != 0 {
        return;
    }

    // Processa as cláusulas LIMIT e OFFSET, se existirem
    let addr_break = vdbe_make_label(p_parse);
    p.n_select_row = 320; // 4 bilhões de linhas
    compute_limit_registers(p_parse, p, addr_break);
    let p_limit = p.p_limit.take();
    let reg_limit = p.i_limit;
    let reg_offset = p.i_offset;
    p.i_limit = 0;
    p.i_offset = 0;
    let has_order_by = p.p_order_by.is_some();
    let n_order_by = p.p_order_by.as_ref().map_or(0, |o| o.n_expr);

    // Localiza o número do cursor da tabela Current
    {
        let p_src = p.p_src.as_ref().expect("Select sem pSrc");
        let mut i = 0;
        while always(i < p_src.n_src) {
            if p_src.a[i as usize].fg.is_recursive != 0 {
                i_current = p_src.a[i as usize].i_cursor;
                break;
            }
            i += 1;
        }
    }

    // Aloca os números de cursor da Queue e da Distinct. O da Distinct deve ser exatamente um a
    // mais que o da Queue para os destinos SRT_DistFifo e SRT_DistQueue funcionarem.
    let i_queue = p_parse.n_tab;
    p_parse.n_tab += 1;
    if p.op == TK_UNION {
        e_dest = (if has_order_by { SRT_DIST_QUEUE } else { SRT_DIST_FIFO }) as i32;
        i_distinct = p_parse.n_tab;
        p_parse.n_tab += 1;
    } else {
        e_dest = (if has_order_by { SRT_QUEUE } else { SRT_FIFO }) as i32;
    }
    select_dest_init(&mut dest_queue, e_dest, i_queue);

    // Aloca cursores para Current, Queue e Distinct.
    p_parse.n_mem += 1;
    let reg_current = p_parse.n_mem;
    let v = p_parse.p_vdbe.clone().expect("Parse sem Vdbe");
    vdbe_add_op3(&mut v.borrow_mut(), OP_OPEN_PSEUDO as i32, i_current, reg_current, n_col);
    if has_order_by {
        let p_key_info = multi_select_order_by_key_info(p_parse, p, 1);
        vdbe_add_op4(
            &mut v.borrow_mut(),
            OP_OPEN_EPHEMERAL as i32,
            i_queue,
            n_order_by + 2,
            0,
            P4Value::KeyInfo(p_key_info.expect("sem KeyInfo")),
            P4_KEYINFO as i8,
        );
        // No C `destQueue.pOrderBy = pOrderBy` aponta para o mesmo ExprList; aqui é uma cópia
        // feita depois de multi_select_order_by_key_info ter acrescentado os COLLATE.
        dest_queue.p_order_by = expr_list_dup(&db, p.p_order_by.as_deref(), 0);
    } else {
        vdbe_add_op2(&mut v.borrow_mut(), OP_OPEN_EPHEMERAL as i32, i_queue, n_col);
    }
    if i_distinct != 0 {
        p.addr_open_ephm[0] = vdbe_add_op2(&mut v.borrow_mut(), OP_OPEN_EPHEMERAL as i32, i_distinct, 0);
        p.sel_flags |= SF_USES_EPHEMERAL;
    }

    // Desanexa a cláusula ORDER BY do SELECT composto
    let p_order_by = p.p_order_by.take();

    // Descobre quantos elementos do SELECT composto fazem parte da consulta recursiva. Garante que
    // nenhum elemento recursivo usa funções de agregação. Marca os elementos recursivos como UNION
    // ALL mesmo que sejam UNION, porque a distinção é garantida pela tabela iDistinct. `depth`
    // fica com a distância de `p` até pFirstRec, o termo recursivo mais à esquerda do CTE.
    let mut depth: usize = 0;
    let mut aborted = false;
    {
        let mut p_first_rec: &mut Select = &mut *p;
        loop {
            if (p_first_rec.sel_flags & SF_AGGREGATE) != 0 {
                error_msg(p_parse, b"recursive aggregate queries not supported", &[]);
                aborted = true;
                break;
            }
            p_first_rec.op = TK_ALL;
            let prior_recursive = (p_first_rec.p_prior.as_ref().expect("sem setup-query").sel_flags & SF_RECURSIVE) != 0;
            if !prior_recursive {
                break;
            }
            depth += 1;
            p_first_rec = p_first_rec.p_prior.as_deref_mut().unwrap();
        }
    }

    if !aborted {
        // Guarda os resultados da setup-query na Queue.
        let mut p_setup: Box<Select> = nth_prior_mut(p, depth).p_prior.take().expect("sem setup-query");
        vdbe_explain(p_parse, 1, b"SETUP", &[]);
        let rc = select(p_parse, &mut p_setup, &mut dest_queue);
        if rc != 0 {
            nth_prior_mut(p, depth).p_prior = Some(p_setup);
        } else {
            // Acha a próxima linha da Queue e a envia
            let addr_top = vdbe_add_op2(&mut v.borrow_mut(), OP_REWIND as i32, i_queue, addr_break);

            // Transfere a próxima linha da Queue para Current
            vdbe_add_op1(&mut v.borrow_mut(), OP_NULL_ROW as i32, i_current); // Para zerar o cache de colunas
            if has_order_by {
                vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, i_queue, n_order_by + 1, reg_current);
            } else {
                vdbe_add_op2(&mut v.borrow_mut(), OP_ROW_DATA as i32, i_queue, reg_current);
            }
            vdbe_add_op1(&mut v.borrow_mut(), OP_DELETE as i32, i_queue);

            // Envia a única linha de Current
            let addr_cont = vdbe_make_label(p_parse);
            code_offset(&mut v.borrow_mut(), reg_offset, addr_cont);
            select_inner_loop(p_parse, p, i_current, None, None, p_dest, addr_cont, addr_break);
            if reg_limit != 0 {
                vdbe_add_op2(&mut v.borrow_mut(), OP_DECR_JUMP_ZERO as i32, reg_limit, addr_break);
            }
            vdbe_resolve_label(&mut v.borrow_mut(), addr_cont);

            // Executa o SELECT recursivo tomando a única linha de Current como valor da
            // recursive-table. Guarda os resultados na Queue. (pFirstRec->pPrior já é zero.)
            vdbe_explain(p_parse, 1, b"RECURSIVE STEP", &[]);
            select(p_parse, p, &mut dest_queue);
            debug_assert!(nth_prior_mut(p, depth).p_prior.is_none());
            nth_prior_mut(p, depth).p_prior = Some(p_setup);

            // Continua rodando o laço até a Queue ficar vazia
            vdbe_goto(&mut v.borrow_mut(), addr_top);
            vdbe_resolve_label(&mut v.borrow_mut(), addr_break);
        }
    }

    // end_of_recursive_query:
    expr_list_delete(&db, p.p_order_by.take());
    p.p_order_by = p_order_by;
    p.p_limit = p_limit;
}

/// Anda `n` posições pela cadeia `p_prior` a partir de `p` (o `pFirstRec` do C é `p` depois de
/// `n` passos).
pub fn nth_prior_mut(p: &mut Select, n: usize) -> &mut Select {
    let mut cur = p;
    for _ in 0..n {
        cur = cur.p_prior.as_deref_mut().expect("cadeia pPrior curta demais");
    }
    cur
}


// ---- part_006.rs ----

/// Copia um SelectDest como o `dest = *pDest` do C. O `p_order_by` (que no C é um ponteiro
/// compartilhado, só usado nos destinos de fila da consulta recursiva) não é copiado.
fn copy_select_dest(d: &SelectDest) -> SelectDest {
    SelectDest {
        e_dest: d.e_dest,
        i_sdparm: d.i_sdparm,
        i_sdparm2: d.i_sdparm2,
        i_sdst: d.i_sdst,
        n_sdst: d.n_sdst,
        z_aff_sdst: d.z_aff_sdst.clone(),
        p_order_by: None,
    }
}

/// Trata o caso especial de um select composto que se origina de uma cláusula VALUES. Tratá-lo
/// como caso especial evita recursão profunda, e assim não é preciso impor o
/// SQLITE_LIMIT_COMPOUND_SELECT numa cláusula VALUES.
///
/// Como o objeto Select vem de uma cláusula VALUES:
///   (1) Não há LIMIT nem OFFSET, ou há um LIMIT de exatamente 1
///   (2) Todos os termos são UNION ALL
///   (3) Não há cláusula ORDER BY
///
/// O caso "LIMIT de exatamente 1" da condição (1) aparece quando um VALUES ocorre dentro de uma
/// expressão escalar (ex: "SELECT (VALUES(1),(2),(3))"). O sqlite3CodeSubselect acrescenta o
/// LIMIT 1 nesse caso. Como o limite é exatamente 1, só o VALUES mais à esquerda é avaliado.
///
/// Modelo: o laço do C vai do termo mais à esquerda para a direita por `pNext`. Aqui a cadeia
/// `p_prior` é desmontada num vetor (o mais à esquerda por último), percorrida de trás para a
/// frente e remontada no fim.
fn multi_select_values(p_parse: &mut Parse, p: &mut Select, p_dest: &mut SelectDest) -> i32 {
    let mut n_row: i32 = 1;
    let rc = 0;
    let b_show_all = p.p_limit.is_none();
    debug_assert!((p.sel_flags & SF_MULTIVALUE) != 0);
    {
        let mut cur: &Select = p;
        loop {
            debug_assert!((cur.sel_flags & SF_VALUES) != 0);
            debug_assert!(cur.op == TK_ALL || (cur.op == TK_SELECT && cur.p_prior.is_none()));
            if cur.p_win.is_some() {
                return -1;
            }
            match cur.p_prior.as_deref() {
                None => break,
                Some(prior) => {
                    cur = prior;
                    n_row += b_show_all as i32;
                }
            }
        }
    }
    let z = format!("SCAN {} CONSTANT ROW{}", n_row, if n_row == 1 { "" } else { "S" });
    vdbe_explain(p_parse, 0, z.as_bytes(), &[]);

    let mut chain: Vec<Box<Select>> = Vec::new();
    let mut next = p.p_prior.take();
    while let Some(mut b) = next {
        next = b.p_prior.take();
        chain.push(b);
    }
    // O mais à esquerda é o último de `chain`; depois dele vêm os demais e por fim o próprio `p`.
    let mut stopped = false;
    for node in chain.iter_mut().rev() {
        select_inner_loop(p_parse, node, -1, None, None, p_dest, 1, 1);
        if !b_show_all {
            stopped = true;
            break;
        }
        node.n_select_row = n_row as LogEst;
    }
    if !stopped {
        select_inner_loop(p_parse, p, -1, None, None, p_dest, 1, 1);
        if b_show_all {
            p.n_select_row = n_row as LogEst;
        }
    }
    // Remonta a cadeia p_prior.
    let mut prior: Option<Box<Select>> = None;
    for mut b in chain.into_iter().rev() {
        b.p_prior = prior;
        prior = Some(b);
    }
    p.p_prior = prior;
    rc
}

/// Devolve verdadeiro se o SELECT, sabidamente a parte recursiva de um CTE recursivo, ainda tem
/// os termos âncora anexados. Se os termos âncora já foram removidos, devolve falso.
fn has_anchor(p: Option<&Select>) -> bool {
    let mut p = p;
    while let Some(s) = p {
        if (s.sel_flags & SF_RECURSIVE) == 0 {
            break;
        }
        p = s.p_prior.as_deref();
    }
    p.is_some()
}

/// Esta rotina é chamada para processar uma consulta composta formada por duas ou mais consultas
/// separadas com UNION, UNION ALL, EXCEPT ou INTERSECT.
///
/// "p" aponta para a mais à direita das duas consultas. A consulta à esquerda é `p.p_prior`. A
/// consulta à esquerda também pode ser composta, e então esta rotina é chamada recursivamente.
///
/// Os resultados da consulta total são gravados num destino do tipo eDest com o parâmetro iParm.
///
/// Exemplo 1: um comando composto de três vias.
///
///     SELECT a FROM t1 UNION SELECT b FROM t2 UNION SELECT c FROM t3
///
/// É analisado assim:
///
///     SELECT c FROM t3
///      |
///      `----->  SELECT b FROM t2
///                |
///                `------>  SELECT a FROM t1
///
/// As setas representam o ponteiro Select.pPrior. Se esta rotina é chamada com p igual à consulta
/// t3, pPrior é a consulta t2 e `p.op` é TK_UNION. Pela forma como o SQLite analisa SELECTs
/// compostos, os selects individuais sempre se agrupam da esquerda para a direita.
///
/// Modelo: o `goto multi_select_end` do C é `break 'end` do bloco rotulado. Enquanto `p.p_prior`
/// está desanexado (como o `p->pPrior = 0` do C) o `pPrior` fica numa variável local.
fn multi_select(p_parse: &mut Parse, p: &mut Select, p_dest: &mut SelectDest) -> i32 {
    let mut rc = SQLITE_OK; // Código de sucesso de uma subrotina
    let mut p_delete: Option<Box<Select>> = None; // Cadeia de selects simples a apagar
    let db = p_parse.db.upgrade().expect("Parse sem conexão");

    // Garante que não há ORDER BY nem LIMIT nos SELECTs anteriores. Só o último (mais à direita)
    // SELECT da série pode ter ORDER BY ou LIMIT.
    debug_assert!(p.p_prior.is_some()); // A função chamadora garante isto
    debug_assert!((p.sel_flags & SF_RECURSIVE) == 0 || p.op == TK_ALL || p.op == TK_UNION);
    debug_assert!((p.sel_flags & SF_COMPOUND) != 0);
    let mut dest = copy_select_dest(p_dest);
    debug_assert!(p.p_prior.as_ref().unwrap().p_order_by.is_none());
    debug_assert!(p.p_prior.as_ref().unwrap().p_limit.is_none());

    let v = get_vdbe(p_parse).expect("O VDBE já foi criado pela função chamadora");

    'end: {
        // Cria a tabela temporária de destino se necessário
        if dest.e_dest == SRT_EPHEM_TAB as u8 {
            debug_assert!(p.p_elist.is_some());
            vdbe_add_op2(
                &mut v.borrow_mut(),
                OP_OPEN_EPHEMERAL as i32,
                dest.i_sdparm,
                p.p_elist.as_ref().unwrap().n_expr,
            );
            dest.e_dest = SRT_TABLE as u8;
        }

        // Tratamento especial para um select composto que se origina de uma cláusula VALUES.
        if (p.sel_flags & SF_MULTIVALUE) != 0 {
            rc = multi_select_values(p_parse, p, &mut dest);
            if rc >= 0 {
                break 'end;
            }
            rc = SQLITE_OK;
        }

        // Garante que todos os SELECTs do comando têm o mesmo número de elementos no resultado.
        debug_assert!(p.p_elist.is_some() && p.p_prior.as_ref().unwrap().p_elist.is_some());
        debug_assert!(
            p.p_elist.as_ref().unwrap().n_expr == p.p_prior.as_ref().unwrap().p_elist.as_ref().unwrap().n_expr
        );

        if (p.sel_flags & SF_RECURSIVE) != 0 && has_anchor(Some(p)) {
            generate_with_recursive_query(p_parse, p, &mut dest);
        } else if p.p_order_by.is_some() {
            // Selects compostos com ORDER BY são tratados à parte.
            return multi_select_order_by(p_parse, p, p_dest);
        } else {
            if p.p_prior.as_ref().unwrap().p_prior.is_none() {
                vdbe_explain(p_parse, 1, b"COMPOUND QUERY", &[]);
                vdbe_explain(p_parse, 1, b"LEFT-MOST SUBQUERY", &[]);
            }

            // Gera código para os SELECTs esquerdo e direito.
            match p.op {
                TK_ALL => {
                    let mut addr = 0;
                    debug_assert!(p.p_prior.as_ref().unwrap().p_limit.is_none());
                    {
                        let (i_limit, i_offset) = (p.i_limit, p.i_offset);
                        let p_limit = p.p_limit.take();
                        let p_prior = p.p_prior.as_deref_mut().unwrap();
                        p_prior.i_limit = i_limit;
                        p_prior.i_offset = i_offset;
                        p_prior.p_limit = p_limit;
                    }
                    rc = select(p_parse, p.p_prior.as_deref_mut().unwrap(), &mut dest);
                    p.p_limit = p.p_prior.as_deref_mut().unwrap().p_limit.take();
                    if rc != 0 {
                        break 'end;
                    }
                    let mut p_prior = p.p_prior.take().unwrap();
                    p.i_limit = p_prior.i_limit;
                    p.i_offset = p_prior.i_offset;
                    if p.i_limit != 0 {
                        addr = vdbe_add_op1(&mut v.borrow_mut(), OP_IF_NOT as i32, p.i_limit);
                        if p.i_offset != 0 {
                            vdbe_add_op3(
                                &mut v.borrow_mut(),
                                OP_OFFSET_LIMIT as i32,
                                p.i_limit,
                                p.i_offset + 1,
                                p.i_offset,
                            );
                        }
                    }
                    vdbe_explain(p_parse, 1, b"UNION ALL", &[]);
                    rc = select(p_parse, p, &mut dest);
                    p_delete = p.p_prior.take();
                    p.n_select_row = log_est_add(p.n_select_row, p_prior.n_select_row);
                    p_prior.p_limit = None;
                    p.p_prior = Some(p_prior);
                    if let Some(p_left) = p.p_limit.as_ref().and_then(|l| l.p_left.as_deref()) {
                        let mut n_limit: i32 = 0;
                        if expr_is_integer(p_left, &mut n_limit) != 0
                            && n_limit > 0
                            && p.n_select_row > log_est(n_limit as u64)
                        {
                            p.n_select_row = log_est(n_limit as u64);
                        }
                    }
                    if addr != 0 {
                        vdbe_jump_here(&mut v.borrow_mut(), addr);
                    }
                }
                TK_EXCEPT | TK_UNION => {
                    let union_tab: i32; // Cursor da tabela temporária com o resultado
                    let prior_op = SRT_UNION as u8; // Operação SRT_ aplicada aos selects anteriores
                    if dest.e_dest == prior_op {
                        // Podemos reaproveitar a tabela temporária gerada por um SELECT à direita.
                        debug_assert!(p.p_limit.is_none()); // Não permitido em elementos à esquerda
                        union_tab = dest.i_sdparm;
                    } else {
                        // Precisamos criar a nossa tabela temporária para os resultados intermediários.
                        union_tab = p_parse.n_tab;
                        p_parse.n_tab += 1;
                        debug_assert!(p.p_order_by.is_none());
                        let addr = vdbe_add_op2(&mut v.borrow_mut(), OP_OPEN_EPHEMERAL as i32, union_tab, 0);
                        debug_assert!(p.addr_open_ephm[0] == -1);
                        p.addr_open_ephm[0] = addr;
                        find_rightmost(p).sel_flags |= SF_USES_EPHEMERAL;
                        debug_assert!(p.p_elist.is_some());
                    }

                    // Codifica os SELECTs à esquerda
                    debug_assert!(p.p_prior.as_ref().unwrap().p_order_by.is_none());
                    let mut union_dest = SelectDest::default();
                    select_dest_init(&mut union_dest, prior_op as i32, union_tab);
                    rc = select(p_parse, p.p_prior.as_deref_mut().unwrap(), &mut union_dest);
                    if rc != 0 {
                        break 'end;
                    }

                    // Codifica o SELECT atual
                    let op: u8 = if p.op == TK_EXCEPT {
                        SRT_EXCEPT as u8
                    } else {
                        debug_assert!(p.op == TK_UNION);
                        SRT_UNION as u8
                    };
                    let mut p_prior = p.p_prior.take().unwrap();
                    let p_limit = p.p_limit.take(); // Valores salvos de p.p_limit
                    union_dest.e_dest = op;
                    let z = format!("{} USING TEMP B-TREE", select_op_name(p.op));
                    vdbe_explain(p_parse, 1, z.as_bytes(), &[]);
                    rc = select(p_parse, p, &mut union_dest);
                    debug_assert!(p.p_order_by.is_none());
                    p_delete = p.p_prior.take();
                    p.p_order_by = None;
                    if p.op == TK_UNION {
                        p.n_select_row = log_est_add(p.n_select_row, p_prior.n_select_row);
                    }
                    p_prior.p_limit = None;
                    p.p_prior = Some(p_prior);
                    expr_delete(&db, p.p_limit.take());
                    p.p_limit = p_limit;
                    p.i_limit = 0;
                    p.i_offset = 0;

                    // Converte os dados da tabela temporária na forma de que precisamos agora.
                    debug_assert!(union_tab == dest.i_sdparm || dest.e_dest != prior_op);
                    debug_assert!(p.p_elist.is_some() || db.borrow().malloc_failed != 0);
                    if dest.e_dest != prior_op && db.borrow().malloc_failed == 0 {
                        let i_break = vdbe_make_label(p_parse);
                        let i_cont = vdbe_make_label(p_parse);
                        compute_limit_registers(p_parse, p, i_break);
                        vdbe_add_op2(&mut v.borrow_mut(), OP_REWIND as i32, union_tab, i_break);
                        let i_start = vdbe_current_addr(&v.borrow());
                        select_inner_loop(p_parse, p, union_tab, None, None, &mut dest, i_cont, i_break);
                        vdbe_resolve_label(&mut v.borrow_mut(), i_cont);
                        vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, union_tab, i_start);
                        vdbe_resolve_label(&mut v.borrow_mut(), i_break);
                        vdbe_add_op2(&mut v.borrow_mut(), OP_CLOSE as i32, union_tab, 0);
                    }
                }
                _ => {
                    debug_assert!(p.op == TK_INTERSECT);

                    // INTERSECT é diferente dos outros porque exige duas tabelas temporárias. Por
                    // isso tem o seu próprio caso. Começa alocando as tabelas necessárias.
                    let tab1 = p_parse.n_tab;
                    p_parse.n_tab += 1;
                    let tab2 = p_parse.n_tab;
                    p_parse.n_tab += 1;
                    debug_assert!(p.p_order_by.is_none());

                    let addr = vdbe_add_op2(&mut v.borrow_mut(), OP_OPEN_EPHEMERAL as i32, tab1, 0);
                    debug_assert!(p.addr_open_ephm[0] == -1);
                    p.addr_open_ephm[0] = addr;
                    find_rightmost(p).sel_flags |= SF_USES_EPHEMERAL;
                    debug_assert!(p.p_elist.is_some());

                    // Codifica os SELECTs à esquerda na tabela temporária "tab1".
                    let mut intersect_dest = SelectDest::default();
                    select_dest_init(&mut intersect_dest, SRT_UNION as i32, tab1);
                    rc = select(p_parse, p.p_prior.as_deref_mut().unwrap(), &mut intersect_dest);
                    if rc != 0 {
                        break 'end;
                    }

                    // Codifica o SELECT atual na tabela temporária "tab2"
                    let addr = vdbe_add_op2(&mut v.borrow_mut(), OP_OPEN_EPHEMERAL as i32, tab2, 0);
                    debug_assert!(p.addr_open_ephm[1] == -1);
                    p.addr_open_ephm[1] = addr;
                    let mut p_prior = p.p_prior.take().unwrap();
                    let p_limit = p.p_limit.take();
                    intersect_dest.i_sdparm = tab2;
                    let z = format!("{} USING TEMP B-TREE", select_op_name(p.op));
                    vdbe_explain(p_parse, 1, z.as_bytes(), &[]);
                    rc = select(p_parse, p, &mut intersect_dest);
                    p_delete = p.p_prior.take();
                    if p.n_select_row > p_prior.n_select_row {
                        p.n_select_row = p_prior.n_select_row;
                    }
                    p_prior.p_limit = None;
                    p.p_prior = Some(p_prior);
                    expr_delete(&db, p.p_limit.take());
                    p.p_limit = p_limit;

                    // Gera código para fazer a interseção das duas tabelas temporárias.
                    if rc == 0 {
                        debug_assert!(p.p_elist.is_some());
                        let i_break = vdbe_make_label(p_parse);
                        let i_cont = vdbe_make_label(p_parse);
                        compute_limit_registers(p_parse, p, i_break);
                        vdbe_add_op2(&mut v.borrow_mut(), OP_REWIND as i32, tab1, i_break);
                        let r1 = get_temp_reg(p_parse);
                        let i_start = vdbe_add_op2(&mut v.borrow_mut(), OP_ROW_DATA as i32, tab1, r1);
                        vdbe_add_op4_int(&mut v.borrow_mut(), OP_NOT_FOUND as i32, tab2, i_cont, r1, 0);
                        release_temp_reg(p_parse, r1);
                        select_inner_loop(p_parse, p, tab1, None, None, &mut dest, i_cont, i_break);
                        vdbe_resolve_label(&mut v.borrow_mut(), i_cont);
                        vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, tab1, i_start);
                        vdbe_resolve_label(&mut v.borrow_mut(), i_break);
                        vdbe_add_op2(&mut v.borrow_mut(), OP_CLOSE as i32, tab2, 0);
                        vdbe_add_op2(&mut v.borrow_mut(), OP_CLOSE as i32, tab1, 0);
                    }
                }
            }

            // O `if( p->pNext==0 )` do C: o `Select.p_next` não é dono no modelo Rust, então a
            // condição vale para o select mais à direita, que é o único que chega aqui sem
            // ser filho de outro composto (veja a nota do integrador sobre `p_next`).
            if p.p_next.is_none() {
                vdbe_explain_pop(p_parse);
            }
        }
        if p_parse.n_err != 0 {
            break 'end;
        }

        // Calcula as sequências de colação usadas pelas tabelas temporárias necessárias para
        // implementar o select composto. Anexa a estrutura KeyInfo a todas as tabelas temporárias.
        //
        // Esta seção só roda no SELECT mais à direita. Os SELECTs à esquerda sempre a pulam. O
        // mais à direita também pode pulá-la se não tem ORDER BY e nenhuma tabela temporária é
        // necessária.
        if (p.sel_flags & SF_USES_EPHEMERAL) != 0 {
            debug_assert!(p.p_next.is_none());
            debug_assert!(p.p_elist.is_some());
            let n_col = p.p_elist.as_ref().unwrap().n_expr;
            let p_key_info = match key_info_alloc(&db, n_col, 1) {
                Some(k) => k,
                None => {
                    rc = SQLITE_NOMEM_BKPT;
                    break 'end;
                }
            };
            for i in 0..n_col {
                let mut p_coll = multi_select_coll_seq(p_parse, p, i);
                if p_coll.is_none() {
                    p_coll = db.borrow().p_dflt_coll.clone();
                }
                p_key_info.borrow_mut().a_coll[i as usize] = p_coll;
            }

            let mut p_loop: Option<&mut Select> = Some(&mut *p);
            while let Some(l) = p_loop {
                for i in 0..2 {
                    let addr = l.addr_open_ephm[i];
                    if addr < 0 {
                        // Se [0] não é usado, [1] também não. Dá para abortar com segurança ao
                        // achar o primeiro espaço não usado.
                        debug_assert!(l.addr_open_ephm[1] < 0);
                        break;
                    }
                    vdbe_change_p2(&mut v.borrow_mut(), addr, n_col);
                    vdbe_change_p4(
                        &mut v.borrow_mut(),
                        addr,
                        P4Value::KeyInfo(key_info_ref(&p_key_info)),
                        P4_KEYINFO as i32,
                    );
                    l.addr_open_ephm[i] = -1;
                }
                p_loop = l.p_prior.as_deref_mut();
            }
            key_info_unref(Some(p_key_info));
        }
    }

    // multi_select_end:
    p_dest.i_sdst = dest.i_sdst;
    p_dest.n_sdst = dest.n_sdst;
    if let Some(d) = p_delete {
        parser_add_cleanup(p_parse, select_delete_generic, d);
    }
    rc
}


// ---- part_007.rs ----

/// Mensagem de erro para quando dois ou mais termos de um select composto têm conjuntos de
/// resultado de tamanhos diferentes.
pub fn select_wrong_num_terms_error(p_parse: &mut Parse, p: &Select) {
    if (p.sel_flags & SF_VALUES) != 0 {
        error_msg(p_parse, b"all VALUES must have the same number of terms", &[]);
    } else {
        let z = format!(
            "SELECTs to the left and right of {} do not have the same number of result columns",
            select_op_name(p.op)
        );
        error_msg(p_parse, z.as_bytes(), &[]);
    }
}

/// Codifica uma sub-rotina de saída para a implementação de um SELECT por co-rotina.
///
/// Os dados a emitir estão em `p_in.i_sdst`. Há `p_in.n_sdst` colunas a emitir. `p_dest` é para
/// onde a saída deve ir.
///
/// `reg_return` é o número do registro com o endereço de retorno da sub-rotina.
///
/// Se `reg_prev > 0`, é o primeiro registro de um vetor que guarda a saída anterior. `mem[regPrev]`
/// é uma flag que é falsa se não houve saída anterior. Com `reg_prev > 0` é gerado código para
/// suprimir duplicatas, e `p_key_info` é usado para comparar as chaves.
///
/// Se o LIMIT de `p.i_limit` for atingido, salta de imediato para `i_break`.
fn generate_output_subroutine(
    p_parse: &mut Parse,
    p: &mut Select,
    p_in: &SelectDest,
    p_dest: &mut SelectDest,
    reg_return: i32,
    reg_prev: i32,
    p_key_info: Option<&KeyInfoRef>,
    i_break: i32,
) -> i32 {
    let v = p_parse.p_vdbe.clone().expect("Parse sem Vdbe");

    let addr = vdbe_current_addr(&v.borrow());
    let i_continue = vdbe_make_label(p_parse);

    // Suprime duplicatas para UNION, EXCEPT e INTERSECT
    if reg_prev != 0 {
        let addr1 = vdbe_add_op1(&mut v.borrow_mut(), OP_IF_NOT as i32, reg_prev);
        let addr2 = vdbe_add_op4(
            &mut v.borrow_mut(),
            OP_COMPARE as i32,
            p_in.i_sdst,
            reg_prev + 1,
            p_in.n_sdst,
            P4Value::KeyInfo(key_info_ref(p_key_info.expect("sem KeyInfo"))),
            P4_KEYINFO as i8,
        );
        vdbe_add_op3(&mut v.borrow_mut(), OP_JUMP as i32, addr2 + 2, i_continue, addr2 + 2);
        vdbe_jump_here(&mut v.borrow_mut(), addr1);
        vdbe_add_op3(&mut v.borrow_mut(), OP_COPY as i32, p_in.i_sdst, reg_prev + 1, p_in.n_sdst - 1);
        vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 1, reg_prev);
    }
    if p_parse.db.upgrade().expect("Parse sem conexão").borrow().malloc_failed != 0 {
        return 0;
    }

    // Suprime as primeiras OFFSET entradas se há cláusula OFFSET
    code_offset(&mut v.borrow_mut(), p.i_offset, i_continue);

    debug_assert!(p_dest.e_dest != SRT_EXISTS as u8);
    debug_assert!(p_dest.e_dest != SRT_TABLE as u8);
    let e_dest = p_dest.e_dest;
    if e_dest == SRT_EPHEM_TAB as u8 {
        // Guarda o resultado como dados usando uma chave única.
        let r1 = get_temp_reg(p_parse);
        let r2 = get_temp_reg(p_parse);
        vdbe_add_op3(&mut v.borrow_mut(), OP_MAKE_RECORD as i32, p_in.i_sdst, p_in.n_sdst, r1);
        vdbe_add_op2(&mut v.borrow_mut(), OP_NEW_ROWID as i32, p_dest.i_sdparm, r2);
        vdbe_add_op3(&mut v.borrow_mut(), OP_INSERT as i32, p_dest.i_sdparm, r1, r2);
        vdbe_change_p5(&mut v.borrow_mut(), OPFLAG_APPEND);
        release_temp_reg(p_parse, r2);
        release_temp_reg(p_parse, r1);
    } else if e_dest == SRT_SET as u8 {
        // Se estamos criando um conjunto para um "expr IN (SELECT ...)".
        let r1 = get_temp_reg(p_parse);
        vdbe_add_op4(
            &mut v.borrow_mut(),
            OP_MAKE_RECORD as i32,
            p_in.i_sdst,
            p_in.n_sdst,
            r1,
            P4Value::Text(Some(p_dest.z_aff_sdst.clone())),
            p_in.n_sdst as i8,
        );
        vdbe_add_op4_int(&mut v.borrow_mut(), OP_IDX_INSERT as i32, p_dest.i_sdparm, r1, p_in.i_sdst, p_in.n_sdst);
        release_temp_reg(p_parse, r1);
    } else if e_dest == SRT_MEM as u8 {
        // Se é um select escalar que faz parte de uma expressão, guarda os resultados na célula
        // de memória apropriada e sai do laço de varredura. O select pode devolver várias colunas
        // se for o lado direito de um operador IN com valores de linha.
        expr_code_move(p_parse, p_in.i_sdst, p_dest.i_sdparm, p_in.n_sdst);
        // A cláusula LIMIT salta para fora do laço por nós.
    } else if e_dest == SRT_COROUTINE as u8 {
        // Os resultados ficam numa sequência de registros a partir de pDest->iSdst. Depois a
        // co-rotina cede o controle.
        if p_dest.i_sdst == 0 {
            p_dest.i_sdst = get_temp_range(p_parse, p_in.n_sdst);
            p_dest.n_sdst = p_in.n_sdst;
        }
        expr_code_move(p_parse, p_in.i_sdst, p_dest.i_sdst, p_in.n_sdst);
        vdbe_add_op1(&mut v.borrow_mut(), OP_YIELD as i32, p_dest.i_sdparm);
    } else {
        // Se nenhuma das anteriores, o destino do resultado deve ser SRT_Output. Esta rotina
        // nunca é chamada com outro destino além dos tratados acima ou SRT_Output.
        //
        // Em SRT_Output os resultados ficam numa sequência de registros. Depois o opcode
        // OP_ResultRow faz sqlite3_step() devolver a próxima linha de resultado.
        debug_assert!(e_dest == SRT_OUTPUT as u8);
        vdbe_add_op2(&mut v.borrow_mut(), OP_RESULT_ROW as i32, p_in.i_sdst, p_in.n_sdst);
    }

    // Salta para o fim do laço se o LIMIT foi atingido.
    if p.i_limit != 0 {
        vdbe_add_op2(&mut v.borrow_mut(), OP_DECR_JUMP_ZERO as i32, p.i_limit, i_break);
    }

    // Gera o retorno da sub-rotina
    vdbe_resolve_label(&mut v.borrow_mut(), i_continue);
    vdbe_add_op1(&mut v.borrow_mut(), OP_RETURN as i32, reg_return);

    addr
}

/// Gerador alternativo de código para compostos quando há cláusula ORDER BY.
///
/// Supomos uma consulta da forma:
///
///      <selectA>  <operator>  <selectB>  ORDER BY <orderbylist>
///
/// <operator> é UNION ALL, UNION, EXCEPT ou INTERSECT. A ideia é codificar <selectA> e <selectB>
/// com a cláusula ORDER BY como co-rotinas. Depois as co-rotinas rodam em paralelo e os
/// resultados são mesclados na saída. Além das duas co-rotinas (selectA e selectB) há 7
/// sub-rotinas:
///
///    outA:    Move a saída da co-rotina selectA para a saída da consulta composta.
///
///    outB:    Move a saída da co-rotina selectB para a saída da consulta composta. (Só gerada
///             para UNION e UNION ALL. EXCEPT e INTERSECT nunca emitem uma linha que só aparece
///             em B.)
///
///    AltB:    Chamada quando há dados das duas co-rotinas e A<B.
///
///    AeqB:    Chamada quando há dados das duas co-rotinas e A==B.
///
///    AgtB:    Chamada quando há dados das duas co-rotinas e A>B.
///
///    EofA:    Chamada quando os dados de selectA se esgotaram.
///
///    EofB:    Chamada quando os dados de selectB se esgotaram.
///
/// A implementação das cinco últimas sub-rotinas depende do <operator>:
///
///             UNION ALL         UNION            EXCEPT          INTERSECT
///          -------------  -----------------  --------------  -----------------
///   AltB:   outA, nextA      outA, nextA       outA, nextA         nextA
///
///   AeqB:   outA, nextA         nextA             nextA         outA, nextA
///
///   AgtB:   outB, nextB      outB, nextB          nextB            nextB
///
///   EofA:   outB, nextB      outB, nextB          halt             halt
///
///   EofB:   outA, nextA      outA, nextA       outA, nextA         halt
///
/// Nas sub-rotinas AltB, AeqB e AgtB, um EOF em A depois de nextA causa um salto imediato para
/// EofA e um EOF em B depois de nextB causa um salto imediato para EofB. Dentro de EofA e EofB,
/// um EOF na entrada ou depois de nextX causa um salto para o fim do processamento do select.
///
/// A remoção de duplicatas em UNION, EXCEPT e INTERSECT é tratada na sub-rotina de saída. O
/// conjunto de registros regPrev guarda o valor emitido antes. Compara-se com esse valor e a
/// saída é pulada se os próximos resultados forem iguais ao anterior.
///
/// O plano é implementar primeiro as duas co-rotinas e as sete sub-rotinas e depois pôr a lógica
/// de controle no fim, assim:
///
///          goto Init
///     coA: co-rotina da consulta da esquerda (A)
///     coB: co-rotina da consulta da direita (B)
///    outA: emite uma linha de A
///    outB: emite uma linha de B (só UNION e UNION ALL)
///    EofA: ...
///    EofB: ...
///    AltB: ...
///    AeqB: ...
///    AgtB: ...
///    Init: inicializa os registros das co-rotinas
///          yield coA
///          if eof(A) goto EofA
///          yield coB
///          if eof(B) goto EofB
///    Cmpr: Compara A, B
///          Jump AltB, AeqB, AgtB
///     End: ...
///
/// Chamamos AltB, AeqB, AgtB, EofA e EofB de "sub-rotinas", mas na verdade não são chamadas com
/// Gosub e não fazem Return. EofA e EofB giram até esgotar todos os dados e saltam para o rótulo
/// "end". AltB, AeqB e AgtB saltam para L2 ou para EofA ou EofB.
///
/// Modelo: `pSplit` é `p` avançado `depth` posições por `p_prior` (`nth_prior_mut`). A cadeia da
/// direita fica em `p` e a da esquerda, `pPrior`, é um `Box` local até ser reanexada no fim.
fn multi_select_order_by(p_parse: &mut Parse, p: &mut Select, p_dest: &mut SelectDest) -> i32 {
    let db = p_parse.db.upgrade().expect("Parse sem conexão");
    let mut p_key_dup: Option<KeyInfoRef> = None; // Informação de comparação para remover duplicatas

    debug_assert!(p.p_order_by.is_some());
    let v = p_parse.p_vdbe.clone().expect("O erro já foi lançado se o VDBE falhou");
    let label_end = vdbe_make_label(p_parse); // Rótulo do fim do SELECT inteiro
    let label_cmpr = vdbe_make_label(p_parse); // Rótulo do início do algoritmo de mesclagem

    // Ajusta a cláusula ORDER BY
    let op = p.op; // Um de TK_ALL, TK_UNION, TK_EXCEPT, TK_INTERSECT
    debug_assert!(p.p_prior.as_ref().unwrap().p_order_by.is_none());
    let mut n_order_by = p.p_order_by.as_ref().unwrap().n_expr;

    // Para operadores diferentes de UNION ALL é preciso garantir que o ORDER BY cobre todos os
    // termos do resultado. Acrescenta termos ao ORDER BY conforme necessário.
    if op != TK_ALL {
        let n_expr = p.p_elist.as_ref().unwrap().n_expr;
        let mut i = 1;
        while db.borrow().malloc_failed == 0 && i <= n_expr {
            let mut j = 0;
            {
                let p_order_by = p.p_order_by.as_ref().unwrap();
                while j < n_order_by {
                    let i_order_by_col = match p_order_by.a[j as usize].u {
                        ExprListItemU::X { i_order_by_col, .. } => i_order_by_col as i32,
                        _ => 0,
                    };
                    debug_assert!(i_order_by_col > 0);
                    if i_order_by_col == i {
                        break;
                    }
                    j += 1;
                }
            }
            if j == n_order_by {
                let mut p_new = match expr(&db, TK_INTEGER, None) {
                    Some(e) => e,
                    None => return SQLITE_NOMEM_BKPT,
                };
                p_new.flags |= EP_INT_VALUE;
                p_new.u.i_value = i;
                p.p_order_by = expr_list_append(p_parse, p.p_order_by.take(), Some(p_new));
                if let Some(ob) = p.p_order_by.as_mut() {
                    ob.a[n_order_by as usize].u = ExprListItemU::X { i_order_by_col: i as u16, i_alias: 0 };
                    n_order_by += 1;
                }
            }
            i += 1;
        }
    }

    // Calcula a permutação de comparação e o keyinfo usados com a permutação para decidir se a
    // próxima linha de resultados vem de selectA ou de selectB. Também acrescenta colações
    // explícitas aos termos do ORDER BY para que as subconsultas da direita e da esquerda usem a
    // colação correta ao serem avaliadas.
    let mut a_permute: Vec<u32> = vec![0u32; n_order_by as usize + 1];
    a_permute[0] = n_order_by as u32;
    for i in 1..=n_order_by {
        let p_item = &p.p_order_by.as_ref().unwrap().a[(i - 1) as usize];
        let i_order_by_col = match p_item.u {
            ExprListItemU::X { i_order_by_col, .. } => i_order_by_col as i32,
            _ => 0,
        };
        debug_assert!(i_order_by_col > 0);
        debug_assert!(i_order_by_col <= p.p_elist.as_ref().unwrap().n_expr);
        a_permute[i as usize] = (i_order_by_col - 1) as u32;
    }
    let p_key_merge = multi_select_order_by_key_info(p_parse, p, 1); // Informação de comparação para mesclar linhas

    // Aloca um intervalo de registros temporários e o KeyInfo necessário para a lógica que
    // remove linhas de resultado duplicadas quando o operador é UNION, EXCEPT ou INTERSECT (mas
    // não UNION ALL).
    let reg_prev: i32; // Intervalo de registros com a saída anterior
    if op == TK_ALL {
        reg_prev = 0;
    } else {
        let n_expr = p.p_elist.as_ref().unwrap().n_expr;
        debug_assert!(n_order_by >= n_expr || db.borrow().malloc_failed != 0);
        reg_prev = p_parse.n_mem + 1;
        p_parse.n_mem += n_expr + 1;
        vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, reg_prev);
        p_key_dup = key_info_alloc(&db, n_expr, 1);
        if let Some(kd) = p_key_dup.as_ref() {
            debug_assert!(key_info_is_writeable(&kd.borrow()));
            for i in 0..n_expr {
                let c = multi_select_coll_seq(p_parse, p, i);
                let mut k = kd.borrow_mut();
                k.a_coll[i as usize] = c;
                k.a_sort_flags[i as usize] = 0;
            }
        }
    }

    // Separa a consulta da esquerda da da direita
    let mut n_select = 1;
    if (op == TK_ALL || op == TK_UNION) && optimization_enabled(&db.borrow(), SQLITE_BALANCED_MERGE) {
        let mut p_split: &Select = p;
        while p_split.p_prior.is_some() && p_split.op == op {
            n_select += 1;
            p_split = p_split.p_prior.as_deref().unwrap();
        }
    }
    // `depth` é a distância de `p` até pSplit.
    let mut depth: usize = 0;
    if n_select > 3 {
        let mut i = 2;
        while i < n_select {
            depth += 1;
            i += 2;
        }
    }
    debug_assert!(p.p_order_by.is_some() || db.borrow().malloc_failed != 0);
    let p_order_by_dup = expr_list_dup(&db, p.p_order_by.as_deref(), 0);
    let mut p_prior: Box<Select> = nth_prior_mut(p, depth).p_prior.take().expect("sem consulta à esquerda");
    p_prior.p_order_by = p_order_by_dup;
    // sqlite3ResolveOrderGroupBy(pParse, p, p->pOrderBy, "ORDER"): o ORDER BY sai de `p` durante a
    // chamada para não haver duas referências mutáveis.
    {
        let mut ob = p.p_order_by.take();
        resolve_order_group_by(p_parse, p, ob.as_deref_mut(), b"ORDER");
        p.p_order_by = ob;
        let mut ob = p_prior.p_order_by.take();
        resolve_order_group_by(p_parse, &mut p_prior, ob.as_deref_mut(), b"ORDER");
        p_prior.p_order_by = ob;
    }

    // Calcula os registros de limite
    compute_limit_registers(p_parse, p, label_end);
    let reg_limit_a: i32;
    let reg_limit_b: i32;
    if p.i_limit != 0 && op == TK_ALL {
        p_parse.n_mem += 1;
        reg_limit_a = p_parse.n_mem;
        p_parse.n_mem += 1;
        reg_limit_b = p_parse.n_mem;
        vdbe_add_op2(
            &mut v.borrow_mut(),
            OP_COPY as i32,
            if p.i_offset != 0 { p.i_offset + 1 } else { p.i_limit },
            reg_limit_a,
        );
        vdbe_add_op2(&mut v.borrow_mut(), OP_COPY as i32, reg_limit_a, reg_limit_b);
    } else {
        reg_limit_a = 0;
        reg_limit_b = 0;
    }
    expr_delete(&db, p.p_limit.take());

    p_parse.n_mem += 1;
    let reg_addr_a = p_parse.n_mem;
    p_parse.n_mem += 1;
    let reg_addr_b = p_parse.n_mem;
    p_parse.n_mem += 1;
    let reg_out_a = p_parse.n_mem;
    p_parse.n_mem += 1;
    let reg_out_b = p_parse.n_mem;
    let mut dest_a = SelectDest::default(); // Destino da co-rotina A
    let mut dest_b = SelectDest::default(); // Destino da co-rotina B
    select_dest_init(&mut dest_a, SRT_COROUTINE as i32, reg_addr_a);
    select_dest_init(&mut dest_b, SRT_COROUTINE as i32, reg_addr_b);

    let z = format!("MERGE ({})", select_op_name(p.op));
    vdbe_explain(p_parse, 1, z.as_bytes(), &[]);

    // Gera uma co-rotina para avaliar o SELECT à esquerda do operador composto, o select "A".
    let addr_select_a = vdbe_current_addr(&v.borrow()) + 1; // Endereço da co-rotina select-A
    let _ = addr_select_a;
    let addr1 = vdbe_add_op3(&mut v.borrow_mut(), OP_INIT_COROUTINE as i32, reg_addr_a, 0, addr_select_a);
    p_prior.i_limit = reg_limit_a;
    vdbe_explain(p_parse, 1, b"LEFT", &[]);
    select(p_parse, &mut p_prior, &mut dest_a);
    vdbe_end_coroutine(&mut v.borrow_mut(), reg_addr_a);
    vdbe_jump_here(&mut v.borrow_mut(), addr1);

    // Gera uma co-rotina para avaliar o SELECT à direita, o select "B"
    let addr_select_b = vdbe_current_addr(&v.borrow()) + 1; // Endereço da co-rotina select-B
    // O `addr1` de B substitui o de A (como no C, a variável é reutilizada); ele é o alvo do
    // salto de inicialização mais abaixo.
    let addr1 = vdbe_add_op3(&mut v.borrow_mut(), OP_INIT_COROUTINE as i32, reg_addr_b, 0, addr_select_b);
    let saved_limit = p.i_limit;
    let saved_offset = p.i_offset;
    p.i_limit = reg_limit_b;
    p.i_offset = 0;
    vdbe_explain(p_parse, 1, b"RIGHT", &[]);
    select(p_parse, p, &mut dest_b);
    p.i_limit = saved_limit;
    p.i_offset = saved_offset;
    vdbe_end_coroutine(&mut v.borrow_mut(), reg_addr_b);

    // Gera uma sub-rotina que emite a linha atual do select A como próxima linha de saída do
    // select composto.
    let addr_out_a = generate_output_subroutine(
        p_parse,
        p,
        &dest_a,
        p_dest,
        reg_out_a,
        reg_prev,
        p_key_dup.as_ref(),
        label_end,
    ); // Endereço da sub-rotina output-A

    // Gera uma sub-rotina que emite a linha atual do select B como próxima linha de saída do
    // select composto.
    let mut addr_out_b = 0; // Endereço da sub-rotina output-B
    if op == TK_ALL || op == TK_UNION {
        addr_out_b = generate_output_subroutine(
            p_parse,
            p,
            &dest_b,
            p_dest,
            reg_out_b,
            reg_prev,
            p_key_dup.as_ref(),
            label_end,
        );
    }
    key_info_unref(p_key_dup.take());

    // Gera uma sub-rotina para rodar quando os resultados do select A se esgotaram e só restam
    // dados no select B.
    let addr_eof_a: i32; // Endereço da sub-rotina select-A-esgotado
    let addr_eof_a_no_b: i32; // Alternativa a addrEofA se B não foi inicializado
    if op == TK_EXCEPT || op == TK_INTERSECT {
        addr_eof_a = label_end;
        addr_eof_a_no_b = label_end;
    } else {
        addr_eof_a = vdbe_add_op2(&mut v.borrow_mut(), OP_GOSUB as i32, reg_out_b, addr_out_b);
        addr_eof_a_no_b = vdbe_add_op2(&mut v.borrow_mut(), OP_YIELD as i32, reg_addr_b, label_end);
        vdbe_goto(&mut v.borrow_mut(), addr_eof_a);
        p.n_select_row = log_est_add(p.n_select_row, p_prior.n_select_row);
    }

    // Gera uma sub-rotina para rodar quando os resultados do select B se esgotaram e só restam
    // dados no select A.
    let addr_eof_b: i32; // Endereço da sub-rotina select-B-esgotado
    if op == TK_INTERSECT {
        addr_eof_b = addr_eof_a;
        if p.n_select_row > p_prior.n_select_row {
            p.n_select_row = p_prior.n_select_row;
        }
    } else {
        addr_eof_b = vdbe_add_op2(&mut v.borrow_mut(), OP_GOSUB as i32, reg_out_a, addr_out_a);
        vdbe_add_op2(&mut v.borrow_mut(), OP_YIELD as i32, reg_addr_a, label_end);
        vdbe_goto(&mut v.borrow_mut(), addr_eof_b);
    }

    // Gera código para tratar o caso A<B
    let mut addr_alt_b = vdbe_add_op2(&mut v.borrow_mut(), OP_GOSUB as i32, reg_out_a, addr_out_a);
    vdbe_add_op2(&mut v.borrow_mut(), OP_YIELD as i32, reg_addr_a, addr_eof_a);
    vdbe_goto(&mut v.borrow_mut(), label_cmpr);

    // Gera código para tratar o caso A==B
    let addr_aeq_b: i32;
    if op == TK_ALL {
        addr_aeq_b = addr_alt_b;
    } else if op == TK_INTERSECT {
        addr_aeq_b = addr_alt_b;
        addr_alt_b += 1;
    } else {
        addr_aeq_b = vdbe_add_op2(&mut v.borrow_mut(), OP_YIELD as i32, reg_addr_a, addr_eof_a);
        vdbe_goto(&mut v.borrow_mut(), label_cmpr);
    }

    // Gera código para tratar o caso A>B
    let addr_agt_b = vdbe_current_addr(&v.borrow());
    if op == TK_ALL || op == TK_UNION {
        vdbe_add_op2(&mut v.borrow_mut(), OP_GOSUB as i32, reg_out_b, addr_out_b);
    }
    vdbe_add_op2(&mut v.borrow_mut(), OP_YIELD as i32, reg_addr_b, addr_eof_b);
    vdbe_goto(&mut v.borrow_mut(), label_cmpr);

    // Este código roda uma vez para inicializar tudo.
    vdbe_jump_here(&mut v.borrow_mut(), addr1);
    vdbe_add_op2(&mut v.borrow_mut(), OP_YIELD as i32, reg_addr_a, addr_eof_a_no_b);
    vdbe_add_op2(&mut v.borrow_mut(), OP_YIELD as i32, reg_addr_b, addr_eof_b);

    // Implementa o laço principal da mesclagem
    vdbe_resolve_label(&mut v.borrow_mut(), label_cmpr);
    vdbe_add_op4(
        &mut v.borrow_mut(),
        OP_PERMUTATION as i32,
        0,
        0,
        0,
        P4Value::IntArray(a_permute),
        P4_INTARRAY as i8,
    );
    vdbe_add_op4(
        &mut v.borrow_mut(),
        OP_COMPARE as i32,
        dest_a.i_sdst,
        dest_b.i_sdst,
        n_order_by,
        P4Value::KeyInfo(p_key_merge.expect("sem KeyInfo de mesclagem")),
        P4_KEYINFO as i8,
    );
    vdbe_change_p5(&mut v.borrow_mut(), OPFLAG_PERMUTE);
    vdbe_add_op3(&mut v.borrow_mut(), OP_JUMP as i32, addr_alt_b, addr_aeq_b, addr_agt_b);

    // Salta para este ponto para terminar a consulta.
    vdbe_resolve_label(&mut v.borrow_mut(), label_end);

    // Providencia a liberação do 2º braço do composto e dos seguintes depois que o parse terminar.
    {
        let p_split = nth_prior_mut(p, depth);
        if let Some(rest) = p_split.p_prior.take() {
            parser_add_cleanup(p_parse, select_delete_generic, rest);
        }
        expr_list_delete(&db, p_prior.p_order_by.take());
        p_split.p_prior = Some(p_prior);
    }

    // TBD: inserir chamadas de sub-rotina para fechar cursores de subconsultas incompletas
    vdbe_explain_pop(p_parse);
    (p_parse.n_err != 0) as i32
}


// ---- part_008.rs ----

/// Uma instância de SubstContext descreve uma edição de substituição a ser
/// feita numa árvore de análise.
///
/// Todas as referências a colunas da tabela `i_table` são trocadas pelas
/// expressões correspondentes em `p_e_list`.
///
/// ## Sobre "is_outer_join":
///
/// `is_outer_join` indica que a substituição ocorre numa posição do pai que
/// pode ser NULL por causa de um OUTER JOIN: o slot de destino é o operando
/// direito de um LEFT JOIN, ou um dos operandos esquerdos de um RIGHT JOIN.
/// Nos dois casos pode ser preciso contornar a expressão substituída com
/// OP_IfNullRow.
///
/// Se a expressão original é uma constante inteira, mesmo com a flag nullRow
/// ligada na tabela ela não vira NULL. Por isso se insere um OP_IfNullRow que
/// consulta a flag nullRow da tabela: ligada, o registro recebe NULL e a
/// expressão original é contornada; desligada, a expressão original roda e
/// preenche o registro.
///
/// Exemplo em que isso é necessário:
///
///      CREATE TABLE t1(a INTEGER PRIMARY KEY, b INT);
///      CREATE TABLE t2(x INT UNIQUE);
///
///      SELECT a,b,m,x FROM t1 LEFT JOIN (SELECT 59 AS m,x FROM t2) ON b=x;
///
/// Quando a subconsulta do lado direito do LEFT JOIN é achatada, é preciso pôr
/// OP_IfNullRow na frente do OP_Integer que implementa o valor "m", para que
/// NULL seja carregado no lugar de 59 numa linha da esquerda sem par.
///
/// `p_e_list` e `p_c_list` são emprestadas: no C apontam para a subconsulta
/// achatada, que nunca é a mesma árvore que está sendo editada.
pub struct SubstContext<'a> {
    /// O contexto de análise.
    pub p_parse: ParseRef,
    /// Substituir referências a esta tabela.
    pub i_table: i32,
    /// Número da nova tabela.
    pub i_new_table: i32,
    /// Acrescentar opcodes TK_IF_NULL_ROW em cada substituição.
    pub is_outer_join: i32,
    /// Expressões de substituição.
    pub p_e_list: &'a ExprList,
    /// Sequências de colação das expressões de substituição.
    pub p_c_list: &'a ExprList,
}

/// Percorre a expressão `p_expr`. Troca toda referência a uma coluna da tabela
/// número `i_table` por uma cópia da entrada `i_column` de `p_e_list`. (As
/// referências à coluna ROWID ficam como estão.)
///
/// Faz parte do procedimento de achatamento. Uma subconsulta cujo conjunto de
/// resultados é definido por `p_e_list` aparece como entrada na cláusula FROM
/// de um SELECT, e o cursor VDBE dessa entrada é `i_table`. Esta rotina muda
/// `p_expr` para que ela se refira direto à tabela de origem da subconsulta, e
/// não ao conjunto de resultados dela.
pub fn subst_expr(p_subst: &mut SubstContext, p_expr: Option<Box<Expr>>) -> Option<Box<Expr>> {
    let mut p_expr = match p_expr {
        Some(e) => e,
        None => return None,
    };
    if expr_has_property(&p_expr, EP_OUTER_ON | EP_INNER_ON) && p_expr.w.i_join == p_subst.i_table {
        p_expr.w.i_join = p_subst.i_new_table;
    }
    if p_expr.op == TK_COLUMN
        && p_expr.i_table == p_subst.i_table
        && !expr_has_property(&p_expr, EP_FIXED_COL)
    {
        // SQLITE_ALLOW_ROWID_IN_VIEW não está ligado no Debian: o ramo some.
        let i_column = p_expr.i_column as usize;
        debug_assert!(i_column < p_subst.p_e_list.a.len());
        debug_assert!(p_expr.p_right.is_none());
        let p_copy: &Expr = p_subst.p_e_list.a[i_column].p_expr.as_deref().unwrap();
        if expr_is_vector(p_copy) {
            vector_error_msg(&p_subst.p_parse, p_copy);
        } else {
            let db = p_subst.p_parse.borrow().db.upgrade().unwrap();
            // No C, quando precisa de OP_IfNullRow o nó temporário aponta para
            // p_copy e a duplicação copia os dois. Aqui o filho é duplicado
            // antes de entrar no nó novo, o que dá a mesma árvore.
            let p_new_opt = if p_subst.is_outer_join != 0
                && (p_copy.op != TK_COLUMN || p_copy.i_table != p_subst.i_new_table)
            {
                let mut if_null_row = Box::new(Expr::default());
                if_null_row.op = TK_IF_NULL_ROW;
                if_null_row.p_left = expr_dup(&db.borrow(), p_copy, 0);
                if_null_row.i_table = p_subst.i_new_table;
                if_null_row.i_column = -99;
                if_null_row.flags = EP_IF_NULL_ROW;
                Some(if_null_row)
            } else {
                expr_dup(&db.borrow(), p_copy, 0)
            };
            let malloc_failed = db.borrow().malloc_failed != 0;
            if malloc_failed {
                expr_delete(&db.borrow(), p_new_opt);
                return Some(p_expr);
            }
            let mut p_new = p_new_opt.unwrap();
            if p_subst.is_outer_join != 0 {
                expr_set_property(&mut p_new, EP_CAN_BE_NULL);
            }
            if expr_has_property(&p_expr, EP_OUTER_ON | EP_INNER_ON) {
                set_join_expr(
                    Some(&mut p_new),
                    p_expr.w.i_join,
                    p_expr.flags & (EP_OUTER_ON | EP_INNER_ON),
                );
            }
            expr_delete(&db.borrow(), Some(p_expr));
            if p_new.op == TK_TRUEFALSE {
                p_new.u.i_value = expr_truth_value(&p_new);
                p_new.op = TK_INTEGER;
                expr_set_property(&mut p_new, EP_INT_VALUE);
            }

            // Garante que a expressão agora tenha uma sequência de colação
            // implícita, como tinha quando era coluna de uma visão ou subconsulta.
            let (p_nat, p_coll) = {
                let mut parse = p_subst.p_parse.borrow_mut();
                let nat = expr_coll_seq(&mut parse, &p_new);
                let coll = expr_coll_seq(
                    &mut parse,
                    p_subst.p_c_list.a[i_column].p_expr.as_deref().unwrap(),
                );
                (nat, coll)
            };
            let differ = match (&p_nat, &p_coll) {
                (None, None) => false,
                (Some(a), Some(b)) => !Rc::ptr_eq(a, b),
                _ => true,
            };
            if differ || (p_new.op != TK_COLUMN && p_new.op != TK_COLLATE) {
                let z_coll = match &p_coll {
                    Some(c) => c.borrow().z_name.clone(),
                    None => b"BINARY".to_vec(),
                };
                p_new = expr_add_collate_string(&p_subst.p_parse.borrow(), p_new, &z_coll);
            }
            expr_clear_property(&mut p_new, EP_COLLATE);
            return Some(p_new);
        }
    } else {
        if p_expr.op == TK_IF_NULL_ROW && p_expr.i_table == p_subst.i_table {
            p_expr.i_table = p_subst.i_new_table;
        }
        p_expr.p_left = subst_expr(p_subst, p_expr.p_left.take());
        p_expr.p_right = subst_expr(p_subst, p_expr.p_right.take());
        if expr_use_x_select(&p_expr) {
            subst_select(p_subst, p_expr.x.p_select.as_deref_mut(), 1);
        } else {
            subst_expr_list(p_subst, p_expr.x.p_list.as_deref_mut());
        }
        // SQLITE_OMIT_WINDOWFUNC não está ligado no Debian.
        if expr_has_property(&p_expr, EP_WIN_FUNC) {
            let p_win = p_expr.y.p_win.clone().unwrap();
            let mut win = p_win.borrow_mut();
            win.p_filter = subst_expr(p_subst, win.p_filter.take());
            subst_expr_list(p_subst, win.p_partition.as_deref_mut());
            subst_expr_list(p_subst, win.p_order_by.as_deref_mut());
        }
    }
    Some(p_expr)
}

/// Percorre a lista `p_list` e faz as substituições em cada expressão.
pub fn subst_expr_list(p_subst: &mut SubstContext, p_list: Option<&mut ExprList>) {
    let p_list = match p_list {
        Some(l) => l,
        None => return,
    };
    for item in p_list.a.iter_mut() {
        item.p_expr = subst_expr(p_subst, item.p_expr.take());
    }
}

/// Percorre o SELECT `p` e faz as substituições. Com `do_prior` diferente de
/// zero percorre também a cadeia `p_prior`.
pub fn subst_select(p_subst: &mut SubstContext, p: Option<&mut Select>, do_prior: i32) {
    let mut cur = p;
    while let Some(s) = cur {
        subst_expr_list(p_subst, s.p_elist.as_deref_mut());
        subst_expr_list(p_subst, s.p_group_by.as_deref_mut());
        subst_expr_list(p_subst, s.p_order_by.as_deref_mut());
        s.p_having = subst_expr(p_subst, s.p_having.take());
        s.p_where = subst_expr(p_subst, s.p_where.take());
        let p_src = s.p_src.as_deref_mut().unwrap();
        for p_item in p_src.a.iter_mut() {
            subst_select(p_subst, p_item.p_select.as_deref_mut(), 1);
            if p_item.fg.is_tab_func != 0 {
                if let SrcItemU1::FuncArg(p_func_arg) = &mut p_item.u1 {
                    subst_expr_list(p_subst, Some(p_func_arg));
                }
            }
        }
        cur = if do_prior != 0 { s.p_prior.as_deref_mut() } else { None };
    }
}

/// Callback de expressão do Walker de `recompute_columns_used`. O Walker leva
/// em `u` (`WalkerU::AiCol`) o trio `[cursor, baixo, alto]`: o número do
/// cursor do item da FROM e a máscara `col_used` acumulada, dividida em duas
/// metades de 32 bits. É o modelo sem ponteiros do `pWalker->u.pSrcItem` do C,
/// que apontava para um item dentro da própria árvore percorrida.
fn recompute_columns_used_expr(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    if p_expr.op != TK_COLUMN {
        return WRC_CONTINUE;
    }
    if let WalkerU::AiCol(acc) = &mut p_walker.u {
        if acc[0] != p_expr.i_table {
            return WRC_CONTINUE;
        }
        if p_expr.i_column < 0 {
            return WRC_CONTINUE;
        }
        let mask: u64 = expr_col_used(p_expr);
        acc[1] |= (mask & 0xffff_ffff) as u32 as i32;
        acc[2] |= (mask >> 32) as u32 as i32;
    }
    WRC_CONTINUE
}

/// `p_select` é um SELECT e `i_src_item` indexa um item da cláusula FROM dele
/// (`p_select.p_src.a[i_src_item]`). Percorre o SELECT inteiro e recalcula a
/// máscara `col_used` do item.
pub fn recompute_columns_used(p_select: &mut Select, i_src_item: usize) {
    let i_cursor = {
        let p_src_item = &mut p_select.p_src.as_deref_mut().unwrap().a[i_src_item];
        if p_src_item.p_tab.is_none() {
            return;
        }
        p_src_item.col_used = 0;
        p_src_item.i_cursor
    };
    let mut w = Walker {
        p_parse: None,
        x_expr_callback: Some(recompute_columns_used_expr),
        x_select_callback: Some(select_walk_noop),
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WalkerU::AiCol(vec![i_cursor, 0, 0]),
    };
    walk_select(&mut w, Some(&mut *p_select));
    if let WalkerU::AiCol(acc) = &w.u {
        let mask = (acc[1] as u32 as u64) | ((acc[2] as u32 as u64) << 32);
        p_select.p_src.as_deref_mut().unwrap().a[i_src_item].col_used = mask;
    }
}

/// Atribui novos números de cursor a cada item de `p_src`. Para cada número
/// novo, grava uma entrada em `a_csr_map[]` que leva o cursor antigo ao novo:
///
///     a_csr_map[i_old+1] = i_new;
///
/// O chamador garante que o vetor comporta todos os cursores existentes em
/// `p_src`. `a_csr_map[0]` é o tamanho do vetor.
///
/// Se `p_src` tem sub-selects, a rotina se chama recursivamente na cláusula
/// FROM de cada um, com `i_except` igual a -1.
fn srclist_renumber_cursors(
    p_parse: &mut Parse,
    a_csr_map: &mut [i32],
    p_src: &mut SrcList,
    i_except: i32,
) {
    for (i, p_item) in p_src.a.iter_mut().enumerate() {
        if i as i32 != i_except {
            debug_assert!(p_item.i_cursor < a_csr_map[0]);
            if p_item.fg.is_recursive == 0 || a_csr_map[(p_item.i_cursor + 1) as usize] == 0 {
                a_csr_map[(p_item.i_cursor + 1) as usize] = p_parse.n_tab;
                p_parse.n_tab += 1;
            }
            p_item.i_cursor = a_csr_map[(p_item.i_cursor + 1) as usize];
            let mut p = p_item.p_select.as_deref_mut();
            while let Some(s) = p {
                srclist_renumber_cursors(p_parse, a_csr_map, s.p_src.as_deref_mut().unwrap(), -1);
                p = s.p_prior.as_deref_mut();
            }
        }
    }
}

/// `pi_cursor` é um número de cursor. Muda-o se precisar ser mapeado.
fn renumber_cursor_do_mapping(p_walker: &Walker, pi_cursor: &mut i32) {
    if let WalkerU::AiCol(a_csr_map) = &p_walker.u {
        let i_csr = *pi_cursor;
        if i_csr < a_csr_map[0] && a_csr_map[(i_csr + 1) as usize] > 0 {
            *pi_cursor = a_csr_map[(i_csr + 1) as usize];
        }
    }
}

/// Callback de expressão usado por `renumber_cursors` para atualizar os
/// objetos Expr aos números de cursor recém-atribuídos.
fn renumber_cursors_cb(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    let op = p_expr.op;
    if op == TK_COLUMN || op == TK_IF_NULL_ROW {
        renumber_cursor_do_mapping(p_walker, &mut p_expr.i_table);
    }
    if expr_has_property(p_expr, EP_OUTER_ON) {
        renumber_cursor_do_mapping(p_walker, &mut p_expr.w.i_join);
    }
    WRC_CONTINUE
}

/// Atribui um novo número de cursor a cada cursor da cláusula FROM
/// (`Select.p_src`) do SELECT `p`, e a cada cursor da FROM de qualquer
/// sub-select dela, recursivamente. Exceção: o item `i_except` da FROM de `p`
/// não ganha número novo. Atualiza todas as expressões e demais referências
/// para os números novos.
///
/// `a_csr_map` serve de espaço de trabalho. O chamador garante duas coisas:
///
///   * o vetor é maior que o maior número de cursor usado no SELECT, e
///
///   * as entradas dos números de cursor que *não* aparecem nas cláusulas FROM
///     do SELECT, como descrito acima, estão zeradas.
pub fn renumber_cursors(p_parse: &mut Parse, p: &mut Select, i_except: i32, a_csr_map: &mut [i32]) {
    srclist_renumber_cursors(p_parse, a_csr_map, p.p_src.as_deref_mut().unwrap(), i_except);
    let mut w = Walker {
        p_parse: None,
        x_expr_callback: Some(renumber_cursors_cb),
        x_select_callback: Some(select_walk_noop),
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WalkerU::AiCol(a_csr_map.to_vec()),
    };
    walk_select(&mut w, Some(p));
}

/// Se `p_sel` não faz parte de um SELECT composto, devolve a lista de
/// expressões dele. Senão, devolve a lista do SELECT mais à esquerda do
/// composto.
pub fn find_leftmost_expr_list(p_sel: &Select) -> Option<&ExprList> {
    let mut p_sel = p_sel;
    while let Some(prior) = p_sel.p_prior.as_deref() {
        p_sel = prior;
    }
    p_sel.p_elist.as_deref()
}

/// Verdadeiro se alguma coluna do conjunto de resultados da consulta composta
/// tem afinidades incompatíveis em um ou mais braços do composto.
pub fn compound_has_different_affinities(p: &Select) -> bool {
    debug_assert!(p.p_prior.is_some());
    let p_list = p.p_elist.as_deref().unwrap();
    for ii in 0..p_list.a.len() {
        let aff = expr_affinity(p_list.a[ii].p_expr.as_deref().unwrap());
        let mut p_sub1 = p.p_prior.as_deref();
        while let Some(sub) = p_sub1 {
            let sub_list = sub.p_elist.as_deref().unwrap();
            debug_assert!(sub_list.a.len() > ii);
            if expr_affinity(sub_list.a[ii].p_expr.as_deref().unwrap()) != aff {
                return true;
            }
            p_sub1 = sub.p_prior.as_deref();
        }
    }
    false
}


// ---- part_009.rs ----

/// Tenta achatar subconsultas como otimização de desempenho. Devolve 1 se fez
/// mudanças e 0 se não houve achatamento.
///
/// Para entender o conceito, considere a consulta:
///
///     SELECT a FROM (SELECT x+y AS a FROM t1 WHERE z<100) WHERE a>5
///
/// O jeito padrão executa a subconsulta primeiro, guarda o resultado numa
/// tabela temporária e roda a consulta externa sobre ela. Isso exige duas
/// passadas pelos dados e, como a tabela temporária não tem índices, o WHERE
/// externo não pode ser otimizado.
///
/// Esta rotina reescreve consultas assim num único select plano:
///
///     SELECT x+y AS a FROM t1 WHERE z<100 AND a>5
///
/// O código gerado dá o mesmo resultado, mas varre os dados uma vez só, e como
/// podem existir índices em t1, uma varredura completa pode ser evitada.
///
/// O achatamento obedece às restrições abaixo (as marcadas "(**)" foram
/// removidas ou absorvidas por outras):
///
///   (3)  Se a subconsulta é o operando direito de um LEFT JOIN, então
///        (3a) ela não pode ser um join, (3b) a FROM dela não pode ter tabela
///        virtual e (3d) a consulta externa não pode ser DISTINCT. Para RIGHT
///        JOIN veja (26).
///   (4)  A subconsulta não pode ser DISTINCT.
///   (7)  A subconsulta precisa ter cláusula FROM.
///   (8)  Com LIMIT na subconsulta, a externa não pode ser um join.
///   (9)  Com LIMIT na subconsulta, a externa não pode ser agregada.
///  (11)  Subconsulta e externa não podem ter as duas ORDER BY.
///  (13)  Subconsulta e externa não podem usar as duas LIMIT.
///  (14)  A subconsulta não pode usar OFFSET.
///  (15)  Se a externa é parte de um select composto, a subconsulta não pode
///        usar LIMIT.
///  (16)  Se a externa é agregada, a subconsulta não pode usar ORDER BY.
///  (17)  Se a subconsulta é um select composto: (17a) todos os operadores são
///        UNION ALL, (17b) nenhum termo é agregado ou DISTINCT, (17c) todo
///        termo tem FROM, (17d) a externa não é agregada nem DISTINCT, (17e) a
///        subconsulta não tem funções de janela, (17f) ela não é o lado direito
///        de um LEFT JOIN, (17g) ela é o primeiro elemento da externa ou não há
///        RIGHT/FULL JOIN em nenhum braço, (17h) as expressões correspondentes
///        de todos os braços têm a mesma afinidade.
///  (18)  Se a subconsulta é composta, todos os termos do ORDER BY do pai
///        precisam ser cópias de um termo devolvido pelo pai.
///  (19)  Com LIMIT na subconsulta, a externa não pode ter WHERE.
///  (20)  Se a subconsulta é composta, ela não pode usar ORDER BY.
///  (21)  Com LIMIT na subconsulta, a externa não pode ser DISTINCT.
///  (22)  A subconsulta não pode ser um CTE recursivo.
///  (23)  Se a externa é um CTE recursivo, a subconsulta não pode ser composta.
///  (25)  Se a subconsulta ou o pai tem função de janela na lista de seleção ou
///        no ORDER BY, não se tenta achatar.
///  (26)  A subconsulta não pode ser o operando direito de um RIGHT JOIN.
///  (27)  A subconsulta não pode conter FULL ou RIGHT JOIN, a menos que seja o
///        primeiro elemento do pai.
///  (28)  A subconsulta não é um CTE MATERIALIZED (tratado pelo chamador).
///
/// `p` é a consulta externa e a subconsulta é `p.p_src.a[i_from]`. `is_agg` é
/// verdadeiro se a externa usa agregados. Toda a análise de expressões precisa
/// ter ocorrido na externa e na subconsulta antes desta rotina.
///
/// Modelo sem ponteiros: `p_parse` é o `ParseRef` compartilhado, `i_from` é
/// índice, e o vínculo de volta `pNext` entre termos de um composto não é
/// refeito (veja o comentário no laço de duplicação).
pub fn flatten_subquery(p_parse: &ParseRef, p: &mut Select, i_from: usize, is_agg: i32) -> i32 {
    let z_saved_auth_context = p_parse.borrow().z_auth_context.clone();
    let mut is_outer_join: i32 = 0; // Verdadeiro se pSub é o lado direito de um LEFT JOIN
    let db: Sqlite3Ref = p_parse.borrow().db.upgrade().unwrap();
    let mut a_csr_map: Option<Vec<i32>> = None;
    let i_parent: i32; // Cursor VDBE da tabela temporária do resultado de pSub

    // Verifica se o achatamento é permitido. Devolve 0 se não.
    debug_assert!(p.p_prior.is_none());
    if optimization_disabled(&db.borrow(), SQLITE_QUERY_FLATTENER) {
        return 0;
    }
    {
        let p_src = p.p_src.as_deref().unwrap();
        debug_assert!(i_from < p_src.a.len());
        let p_subitem = &p_src.a[i_from];
        i_parent = p_subitem.i_cursor;
        let p_sub = p_subitem.p_select.as_deref().unwrap();

        if p.p_win.is_some() || p_sub.p_win.is_some() {
            return 0; // Restrição (25)
        }

        let p_sub_src = p_sub.p_src.as_deref().unwrap();
        // Antes da versão 3.1.2, quando LIMIT e OFFSET tinham de ser constantes
        // simples, permitia-se alguma combinação deles, pois podiam ser
        // calculados na compilação. Quando viraram expressões arbitrárias,
        // foi preciso acrescentar as restrições (13) e (14).
        if p_sub.p_limit.is_some() && p.p_limit.is_some() {
            return 0; // Restrição (13)
        }
        if let Some(p_limit) = p_sub.p_limit.as_deref() {
            if p_limit.p_right.is_some() {
                return 0; // Restrição (14)
            }
        }
        if (p.sel_flags & SF_COMPOUND) != 0 && p_sub.p_limit.is_some() {
            return 0; // Restrição (15)
        }
        if p_sub_src.a.is_empty() {
            return 0; // Restrição (7)
        }
        if (p_sub.sel_flags & SF_DISTINCT) != 0 {
            return 0; // Restrição (4)
        }
        if p_sub.p_limit.is_some() && (p_src.a.len() > 1 || is_agg != 0) {
            return 0; // Restrições (8)(9)
        }
        if p.p_order_by.is_some() && p_sub.p_order_by.is_some() {
            return 0; // Restrição (11)
        }
        if is_agg != 0 && p_sub.p_order_by.is_some() {
            return 0; // Restrição (16)
        }
        if p_sub.p_limit.is_some() && p.p_where.is_some() {
            return 0; // Restrição (19)
        }
        if p_sub.p_limit.is_some() && (p.sel_flags & SF_DISTINCT) != 0 {
            return 0; // Restrição (21)
        }
        if (p_sub.sel_flags & SF_RECURSIVE) != 0 {
            return 0; // Restrição (22)
        }

        // Se a subconsulta é o operando direito de um LEFT JOIN, ela não pode
        // ser um join (3a). Exemplo do porquê:
        //
        //         t1 LEFT OUTER JOIN (t2 JOIN t3)
        //
        // Achatando, sairia
        //
        //         (t1 LEFT OUTER JOIN t2) JOIN t3
        //
        // que não é a mesma coisa. Veja também os tickets #306, #350 e #3300.
        if (p_subitem.fg.jointype & (JT_OUTER | JT_LTORJ)) != 0 {
            if p_sub_src.a.len() > 1 // (3a)
                || p_sub_src.a[0].p_tab.as_ref().map_or(false, |t| is_virtual(&t.borrow())) // (3b)
                || (p.sel_flags & SF_DISTINCT) != 0 // (3d)
                || (p_subitem.fg.jointype & JT_RIGHT) != 0
            // (26)
            {
                return 0;
            }
            is_outer_join = 1;
        }

        debug_assert!(!p_sub_src.a.is_empty()); // Verdade pela restrição (7)
        if i_from > 0 && (p_sub_src.a[0].fg.jointype & JT_LTORJ) != 0 {
            return 0; // Restrição (27a)
        }

        // A condição (28) é bloqueada pelo chamador.

        // Restrição (17): se a subconsulta é um SELECT composto, só pode usar
        // o operador UNION ALL, e nenhum dos selects simples que formam o
        // composto pode ser agregado ou DISTINCT.
        if p_sub.p_prior.is_some() {
            if p_sub.p_order_by.is_some() {
                return 0; // Restrição (20)
            }
            if is_agg != 0 || (p.sel_flags & SF_DISTINCT) != 0 || is_outer_join > 0 {
                return 0; // (17d1), (17d2) ou (17f)
            }
            let mut p_sub1 = Some(p_sub);
            while let Some(s1) = p_sub1 {
                debug_assert!((p_sub.sel_flags & SF_RECURSIVE) == 0);
                debug_assert!(
                    p_sub.p_elist.as_deref().unwrap().a.len()
                        == s1.p_elist.as_deref().unwrap().a.len()
                );
                if (s1.sel_flags & (SF_DISTINCT | SF_AGGREGATE)) != 0 // (17b)
                    || (s1.p_prior.is_some() && s1.op != TK_ALL) // (17a)
                    || s1.p_src.as_deref().unwrap().a.is_empty() // (17c)
                    || s1.p_win.is_some()
                // (17e)
                {
                    return 0;
                }
                if i_from > 0
                    && (s1.p_src.as_deref().unwrap().a[0].fg.jointype & JT_LTORJ) != 0
                {
                    // Sem esta restrição, o flag JT_LTORJ acabaria omitido nas
                    // tabelas do lado esquerdo do right join achatado.
                    return 0; // Restrições (17g), (27b)
                }
                p_sub1 = s1.p_prior.as_deref();
            }

            // Restrição (18).
            if let Some(p_order_by) = p.p_order_by.as_deref() {
                for item in p_order_by.a.iter() {
                    let i_order_by_col = match item.u {
                        ExprListItemU::X { i_order_by_col, .. } => i_order_by_col,
                        _ => 0,
                    };
                    if i_order_by_col == 0 {
                        return 0;
                    }
                }
            }

            // Restrição (23)
            if (p.sel_flags & SF_RECURSIVE) != 0 {
                return 0;
            }

            // Restrição (17h)
            if compound_has_different_affinities(p_sub) {
                return 0;
            }

            if p_src.a.len() > 1 {
                let n_tab = {
                    let parse = p_parse.borrow();
                    if parse.n_select > 500 {
                        return 0;
                    }
                    parse.n_tab
                };
                if optimization_disabled(&db.borrow(), SQLITE_FLTTN_UNION_ALL) {
                    return 0;
                }
                let mut map = vec![0i32; (n_tab as usize) + 1];
                map[0] = n_tab;
                a_csr_map = Some(map);
            }
        }
    }

    // ***** Se chegamos aqui, o achatamento é permitido. *****

    // Autoriza a subconsulta
    {
        let z_name = p.p_src.as_deref().unwrap().a[i_from].z_name.clone();
        let mut parse = p_parse.borrow_mut();
        parse.z_auth_context = Some(z_name);
        let _ = auth_check(&mut parse, SQLITE_SELECT, None, None, None);
        parse.z_auth_context = z_saved_auth_context;
    }

    // Apaga as estruturas transitórias associadas à subconsulta
    let mut p_sub1: Box<Select> = {
        let p_subitem = &mut p.p_src.as_deref_mut().unwrap().a[i_from];
        let p_sub1 = p_subitem.p_select.take().unwrap();
        p_subitem.z_database = Vec::new();
        p_subitem.z_name = Vec::new();
        p_subitem.z_alias = Vec::new();
        p_sub1
    };

    // Se a subconsulta é um SELECT composto, então (pelas restrições 17 e 18)
    // ela é um UNION ALL e a consulta pai tem a forma:
    //
    //     SELECT <expr-list> FROM (<sub-query>) <where-clause>
    //
    // seguida de ORDER BY, LIMIT e/ou OFFSET. Este bloco cria N-1 cópias da
    // consulta pai, sem ORDER BY, LIMIT nem OFFSET, e as junta à esquerda do
    // original com operadores UNION ALL. N é o número de selects simples do
    // composto da subconsulta.
    //
    // Exemplo:
    //
    //     SELECT a+1 FROM (
    //        SELECT x FROM tab
    //        UNION ALL
    //        SELECT y FROM tab
    //        UNION ALL
    //        SELECT abs(z*2) FROM tab2
    //     ) WHERE a!=5 ORDER BY 1
    //
    // vira:
    //
    //     SELECT x+1 FROM tab WHERE x+1!=5
    //     UNION ALL
    //     SELECT y+1 FROM tab WHERE y+1!=5
    //     UNION ALL
    //     SELECT abs(z*2)+1 FROM tab2 WHERE abs(z*2)+1!=5
    //     ORDER BY 1
    //
    // Chamamos isso de "achatamento de subconsulta composta".
    let mut n_dup = 0usize;
    {
        let mut q = p_sub1.p_prior.as_deref();
        while let Some(s) = q {
            n_dup += 1;
            q = s.p_prior.as_deref();
        }
    }
    for _ in 0..n_dup {
        let p_order_by = p.p_order_by.take();
        let p_limit = p.p_limit.take();
        let p_prior = p.p_prior.take();
        let p_item_tab = p.p_src.as_deref_mut().unwrap().a[i_from].p_tab.take();
        let p_new = select_dup(&db.borrow(), p, 0);
        p.p_limit = p_limit;
        p.p_order_by = p_order_by;
        p.op = TK_ALL;
        p.p_src.as_deref_mut().unwrap().a[i_from].p_tab = p_item_tab;
        match p_new {
            None => {
                p.p_prior = p_prior;
            }
            Some(mut p_new) => {
                {
                    let mut parse = p_parse.borrow_mut();
                    parse.n_select += 1;
                    p_new.sel_id = parse.n_select as u32;
                    if db.borrow().malloc_failed == 0 {
                        if let Some(map) = a_csr_map.as_mut() {
                            renumber_cursors(&mut parse, &mut p_new, i_from as i32, map);
                        }
                    }
                }
                p_new.p_prior = p_prior;
                // No C, aqui vêm `pPrior->pNext = pNew` e `pNew->pNext = p`: o
                // vínculo de volta pNext não tem como ser dono sem ponteiros, e
                // quem precisar dele percorre a cadeia pPrior a partir de p.
                p.p_prior = Some(p_new);
            }
        }
    }
    drop(a_csr_map);
    if db.borrow().malloc_failed != 0 {
        p.p_src.as_deref_mut().unwrap().a[i_from].p_select = Some(p_sub1);
        return 1;
    }

    // Adia a remoção do objeto Table associado à subconsulta até o fim da
    // geração de código, pois ainda podem existir entradas Expr.pTab que se
    // referem à subconsulta mesmo depois do achatamento. Ticket #3346.
    //
    // pSubitem->pTab nunca é nulo pelas restrições e testes acima.
    if let Some(p_tab_to_del) = p.p_src.as_deref_mut().unwrap().a[i_from].p_tab.take() {
        let n_tab_ref = p_tab_to_del.borrow().n_tab_ref;
        if n_tab_ref == 1 {
            let p_toplevel = parse_toplevel(p_parse);
            let _ = parser_add_cleanup(
                &mut p_toplevel.borrow_mut(),
                Box::new(move |db: &Sqlite3Ref| {
                    delete_table_generic(&db.borrow(), Some(&p_tab_to_del));
                }),
                None,
            );
        } else {
            p_tab_to_del.borrow_mut().n_tab_ref -= 1;
        }
    }

    // O laço a seguir roda uma vez para cada termo de um achatamento de
    // subconsulta composta. Em outro tipo de achatamento ele roda uma vez só.
    //
    // O laço move todos os elementos FROM da subconsulta para a FROM da
    // consulta externa. Antes, guarda em i_parent o número do cursor do
    // elemento FROM original da externa. O cursor i_parent nunca será usado.
    // O código seguinte varre as expressões atrás de referências a i_parent e
    // as troca por expressões que resolvem para os elementos FROM da
    // subconsulta que acabamos de copiar.
    let mut i_new_parent: i32 = -1; // Tabela que substitui i_parent
    let mut first = true;
    // Valor LTORJ do item i_from da FROM usada na volta anterior (no C a
    // variável pSrc ainda aponta para a FROM da volta anterior neste ponto).
    let mut ltorj_prev: u8 = p.p_src.as_deref().unwrap().a[i_from].fg.jointype & JT_LTORJ;
    let mut p_sub_cur: Option<&mut Select> = Some(&mut *p_sub1);
    let mut p_parent_cur: Option<&mut Select> = Some(&mut *p);
    while let Some(p_parent) = p_parent_cur {
        let p_sub = match p_sub_cur {
            Some(s) => s,
            None => {
                debug_assert!(false);
                break;
            }
        };
        let mut jointype: u8 = 0;
        let ltorj: u8 = ltorj_prev;
        let n_sub_src = p_sub.p_src.as_deref().unwrap().a.len();

        if first {
            // Primeira passada pelo laço (pParent==p)
            jointype = p_parent.p_src.as_deref().unwrap().a[i_from].fg.jointype;
            first = false;
        }

        // A subconsulta usa um único slot da FROM da consulta externa. Se a
        // FROM da subconsulta tem mais de um elemento, a externa é expandida
        // para abrir espaço para todos.
        //
        // Exemplo:
        //
        //    SELECT * FROM tabA, (SELECT * FROM sub1, sub2), tabB;
        //
        // A externa tem 3 slots na FROM. Um deles (o do meio) é usado pela
        // subconsulta. O próximo bloco expande a FROM da externa para 4 slots,
        // e o do meio vira dois, para os dois elementos da FROM da subconsulta.
        if n_sub_src > 1 {
            let p_src = p_parent.p_src.as_deref_mut().unwrap();
            if !src_list_enlarge(
                &mut p_parse.borrow_mut(),
                p_src,
                (n_sub_src - 1) as i32,
                (i_from + 1) as i32,
            ) {
                break;
            }
        }

        // Transfere os termos FROM da subconsulta para a consulta externa.
        {
            let p_sub_src = p_sub.p_src.as_deref_mut().unwrap();
            let p_src = p_parent.p_src.as_deref_mut().unwrap();
            for i in 0..n_sub_src {
                debug_assert!(p_src.a[i + i_from].fg.is_tab_func == 0);
                // No C: apaga o u3.pUsing do item (se isUsing), copia o item da
                // subconsulta por cima e zera o original. Aqui os dois itens
                // trocam de lugar: o item antigo da externa fica na lista da
                // subconsulta e é liberado junto com ela (o Drop do IdList
                // equivale ao IdListDelete), e o slot da subconsulta deixa de
                // ter dono de qualquer recurso transferido.
                std::mem::swap(&mut p_src.a[i + i_from], &mut p_sub_src.a[i]);
                p_src.a[i + i_from].fg.jointype |= ltorj;
                i_new_parent = p_src.a[i + i_from].i_cursor;
            }
            p_src.a[i_from].fg.jointype &= JT_LTORJ;
            p_src.a[i_from].fg.jointype |= jointype | ltorj;
        }

        // Agora começa a substituição das expressões do conjunto de resultados
        // da subconsulta pelas referências a i_parent na consulta externa.
        //
        // Exemplo:
        //
        //   SELECT a+5, b*10 FROM (SELECT x*3 AS a, y+10 AS b FROM t1) WHERE a>b;
        //   \                     \_____________ subquery __________/          /
        //    \_____________________ outer query ______________________________/
        //
        // Olhamos toda expressão da externa e, onde aparece "a", pomos "x*3",
        // e onde aparece "b", pomos "y+10".
        if p_sub.p_order_by.is_some() && (p_parent.sel_flags & SF_NOOPORDERBY) == 0 {
            // Neste ponto, qualquer iOrderByCol diferente de zero indica que a
            // expressão da coluna do ORDER BY é idêntica à iOrderByCol-ésima
            // expressão devolvida pelo SELECT pSub. Como esses valores não
            // correspondem necessariamente a colunas do SELECT pParent, zeram-se
            // antes de transferir o ORDER BY.
            //
            // Não fazer isso pode causar erro se uma chamada posterior desta
            // função tentar achatar uma subconsulta composta em pParent (só
            // acontece se a composta está em pSub->pSrc). Ticket [d11a6e908f].
            let mut p_order_by = p_sub.p_order_by.take().unwrap();
            for item in p_order_by.a.iter_mut() {
                if let ExprListItemU::X { i_alias, .. } = item.u {
                    item.u = ExprListItemU::X { i_order_by_col: 0, i_alias };
                }
            }
            debug_assert!(p_parent.p_order_by.is_none());
            p_parent.p_order_by = Some(p_order_by);
        }
        let mut p_where = p_sub.p_where.take();
        if is_outer_join > 0 {
            set_join_expr(p_where.as_deref_mut(), i_new_parent, EP_OUTER_ON);
        }
        if let Some(p_where) = p_where {
            if p_parent.p_where.is_some() {
                let p_parent_where = p_parent.p_where.take();
                p_parent.p_where = p_expr(
                    &mut p_parse.borrow_mut(),
                    TK_AND as i32,
                    Some(p_where),
                    p_parent_where,
                );
            } else {
                p_parent.p_where = Some(p_where);
            }
        }
        if db.borrow().malloc_failed == 0 {
            let mut x = SubstContext {
                p_parse: p_parse.clone(),
                i_table: i_parent,
                i_new_table: i_new_parent,
                is_outer_join,
                p_e_list: p_sub.p_elist.as_deref().unwrap(),
                p_c_list: find_leftmost_expr_list(p_sub).unwrap(),
            };
            subst_select(&mut x, Some(&mut *p_parent), 0);
        }

        // O select achatado é composto se a consulta interna ou a externa é.
        p_parent.sel_flags |= p_sub.sel_flags & SF_COMPOUND;
        debug_assert!((p_sub.sel_flags & SF_DISTINCT) == 0); // restrição (17b)

        // SELECT ... FROM (SELECT ... LIMIT a OFFSET b) LIMIT x OFFSET y;
        //
        // É tentador somar a e b para combinar os limites, mas isso não
        // funciona se um dos limites é negativo.
        if p_sub.p_limit.is_some() {
            p_parent.p_limit = p_sub.p_limit.take();
        }

        // Recalcula as máscaras SrcItem.colUsed das tabelas achatadas.
        for i in 0..n_sub_src {
            recompute_columns_used(p_parent, i + i_from);
        }

        ltorj_prev = p_parent.p_src.as_deref().unwrap().a[i_from].fg.jointype & JT_LTORJ;
        p_parent_cur = p_parent.p_prior.as_deref_mut();
        p_sub_cur = p_sub.p_prior.as_deref_mut();
    }

    // Por fim, apaga o que sobrou da subconsulta e devolve sucesso.
    let mut w = Walker {
        p_parse: None,
        x_expr_callback: None,
        x_select_callback: None,
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WalkerU::None,
    };
    agg_info_persist_walker_init(&mut w, p_parse);
    walk_select(&mut w, Some(&mut *p_sub1));
    select_delete(&db, Some(p_sub1));

    1
}


// ---- part_010.rs ----

/// Uma entrada de `WhereConst`: a coluna fixada a um valor conhecido por um
/// termo COLUNA=VALOR do WHERE.
///
/// No C `apExpr[i*2]` e `apExpr[i*2+1]` apontam para nós da própria árvore que
/// o Walker reescreve. Sem ponteiros: `col_addr` guarda só o endereço do nó da
/// coluna (usado apenas para identificar "este mesmo nó" no teste
/// `pColumn==pExpr`, nunca para acessar memória), `i_table`, `i_column` e `aff`
/// são a identidade e a afinidade da coluna, e `p_value` é uma cópia do valor.
pub struct WhereConstEntry {
    /// Endereço do nó COLUMN na árvore, só para comparar identidade.
    pub col_addr: usize,
    /// Cursor da tabela da coluna.
    pub i_table: i32,
    /// Índice da coluna.
    pub i_column: i32,
    /// Afinidade da coluna (`expr_affinity` no momento da inserção).
    pub aff: u8,
    /// O VALUE do termo (cópia).
    pub p_value: Box<Expr>,
}

/// Uma estrutura para acompanhar todos os valores de coluna que são fixos a um
/// valor conhecido por causa de restrições do WHERE do formato COLUNA=VALOR.
///
/// `nConst` do C é `a_const.len()`. `pOomFault` do C (ponteiro para
/// `pParse->db->mallocFailed`) vira a leitura de `malloc_failed` do banco.
pub struct WhereConst {
    /// Contexto de análise.
    pub p_parse: ParseRef,
    /// Número de vezes que uma constante foi propagada.
    pub n_chng: i32,
    /// Pelo menos uma coluna de a_const tem afinidade BLOB.
    pub b_has_aff_blob: i32,
    /// Quais expressões ON excluir da consideração: EP_OuterON, ou
    /// EP_InnerON|EP_OuterON.
    pub m_exclude_on: u32,
    /// Os pares COLUNA=VALOR.
    pub a_const: Vec<WhereConstEntry>,
}

/// Lê `db->mallocFailed` pelo Parse da estrutura.
fn where_const_oom(p_const: &WhereConst) -> bool {
    let db = p_const.p_parse.borrow().db.upgrade().unwrap();
    let failed = db.borrow().malloc_failed != 0;
    failed
}

/// Acrescenta uma entrada nova a `p_const`. Exceto: não acrescenta entradas
/// duplicadas de `p_column`, nem se acrescentar não for apropriado.
///
/// O chamador garante que `p_column` é uma coluna e `p_value` é constante.
/// Esta rotina faz algumas verificações a mais antes de completar a inserção.
fn const_insert(p_const: &mut WhereConst, p_column: &Expr, p_value: &Expr, p_expr: &Expr) {
    debug_assert!(p_column.op == TK_COLUMN);

    if expr_has_property(p_column, EP_FIXED_COL) {
        return;
    }
    if expr_affinity(p_value) != 0 {
        return;
    }
    let p_coll = expr_compare_coll_seq(&mut p_const.p_parse.borrow_mut(), p_expr);
    let binary = match &p_coll {
        None => true,
        Some(c) => is_binary(Some(&c.borrow())) != 0,
    };
    if !binary {
        return;
    }

    // Ticket [cf5ed20f] de 2018-10-25: garante que o mesmo p_column não é
    // inserido mais de uma vez.
    for e2 in p_const.a_const.iter() {
        if e2.i_table == p_column.i_table && e2.i_column == p_column.i_column {
            return; // Já está presente. Volta sem fazer nada.
        }
    }
    let aff = expr_affinity(p_column);
    if aff == SQLITE_AFF_BLOB {
        p_const.b_has_aff_blob = 1;
    }

    let db = p_const.p_parse.borrow().db.upgrade().unwrap();
    let p_dup = expr_dup(&db.borrow(), p_value, 0);
    match p_dup {
        None => {
            // No C, a falha do realloc zera nConst e libera o vetor.
            p_const.a_const.clear();
        }
        Some(p_dup) => {
            p_const.a_const.push(WhereConstEntry {
                col_addr: p_column as *const Expr as usize,
                i_table: p_column.i_table,
                i_column: p_column.i_column,
                aff,
                p_value: p_dup,
            });
        }
    }
}

/// Acha todos os termos COLUNA=VALOR ou VALOR=COLUNA em `p_expr` em que VALOR
/// é uma expressão constante e o termo precisa ser verdadeiro porque faz parte
/// dos termos ligados por AND da expressão. Cada termo achado entra em
/// `p_const`.
fn find_const_in_where(p_const: &mut WhereConst, p_expr: Option<&Expr>) {
    let p_expr = match p_expr {
        Some(e) => e,
        None => return,
    };
    if expr_has_property(p_expr, p_const.m_exclude_on) {
        return;
    }
    if p_expr.op == TK_AND {
        find_const_in_where(p_const, p_expr.p_right.as_deref());
        find_const_in_where(p_const, p_expr.p_left.as_deref());
        return;
    }
    if p_expr.op != TK_EQ {
        return;
    }
    let p_right = p_expr.p_right.as_deref().unwrap();
    let p_left = p_expr.p_left.as_deref().unwrap();
    if p_right.op == TK_COLUMN && expr_is_constant(&p_const.p_parse, p_left) != 0 {
        const_insert(p_const, p_right, p_left, p_expr);
    }
    if p_left.op == TK_COLUMN && expr_is_constant(&p_const.p_parse, p_right) != 0 {
        const_insert(p_const, p_left, p_right, p_expr);
    }
}

/// Função auxiliar do callback `propagate_constant_expr_rewrite`.
///
/// `p_expr` é uma expressão candidata a ser trocada por um valor. Se ela é
/// equivalente a uma das colunas de `p_const`, é sobrescrita com o valor
/// correspondente. Exceto: se `b_ignore_aff_blob` é diferente de zero e a
/// afinidade da coluna é SQLITE_AFF_BLOB, não se faz nada.
fn propagate_constant_expr_rewrite_one(
    p_const: &mut WhereConst,
    p_expr: &mut Expr,
    b_ignore_aff_blob: i32,
) -> i32 {
    if where_const_oom(p_const) {
        return WRC_PRUNE;
    }
    if p_expr.op != TK_COLUMN {
        return WRC_CONTINUE;
    }
    if expr_has_property(p_expr, EP_FIXED_COL | p_const.m_exclude_on) {
        return WRC_CONTINUE;
    }
    let addr = &*p_expr as *const Expr as usize;
    for i in 0..p_const.a_const.len() {
        let p_column = &p_const.a_const[i];
        if p_column.col_addr == addr {
            continue;
        }
        if p_column.i_table != p_expr.i_table {
            continue;
        }
        if p_column.i_column != p_expr.i_column {
            continue;
        }
        if b_ignore_aff_blob != 0 && p_column.aff == SQLITE_AFF_BLOB {
            break;
        }
        // Achou um par. Acrescenta a propriedade EP_FixedCol.
        let db = p_const.p_parse.borrow().db.upgrade().unwrap();
        let p_dup = expr_dup(&db.borrow(), &p_column.p_value, 0);
        p_const.n_chng += 1;
        expr_clear_property(p_expr, EP_LEAF);
        expr_set_property(p_expr, EP_FIXED_COL);
        debug_assert!(p_expr.p_left.is_none());
        p_expr.p_left = p_dup;
        if db.borrow().malloc_failed != 0 {
            return WRC_PRUNE;
        }
        break;
    }
    WRC_PRUNE
}

/// Callback de expressão do Walker. `p_expr` é um nó da cláusula WHERE de um
/// SELECT. Examina `p_expr` para ver se alguma substituição baseada no
/// conteúdo de `pWalker->u.pConst` deve ser feita nele ou nos filhos diretos.
///
/// Faz-se a substituição se:
///
///   + `p_expr` é uma coluna com afinidade diferente de BLOB que casa com uma
///     das colunas de `pWalker->u.pConst`, ou
///
///   + `p_expr` é um operador de comparação binário (=, <=, >=, <, >) que usa
///     afinidade diferente de TEXT e um dos filhos diretos é uma coluna que
///     casa com uma das colunas de `pWalker->u.pConst`.
fn propagate_constant_expr_rewrite(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    let p_const = match &mut p_walker.u {
        WalkerU::Const(c) => c,
        _ => return WRC_CONTINUE,
    };
    debug_assert!(TK_GT == TK_EQ + 1);
    debug_assert!(TK_LE == TK_EQ + 2);
    debug_assert!(TK_LT == TK_EQ + 3);
    debug_assert!(TK_GE == TK_EQ + 4);
    if p_const.b_has_aff_blob != 0 {
        if (p_expr.op >= TK_EQ && p_expr.op <= TK_GE) || p_expr.op == TK_IS {
            propagate_constant_expr_rewrite_one(p_const, p_expr.p_left.as_deref_mut().unwrap(), 0);
            if where_const_oom(p_const) {
                return WRC_PRUNE;
            }
            if expr_affinity(p_expr.p_left.as_deref().unwrap()) != SQLITE_AFF_TEXT {
                propagate_constant_expr_rewrite_one(
                    p_const,
                    p_expr.p_right.as_deref_mut().unwrap(),
                    0,
                );
            }
        }
    }
    let b_has_aff_blob = p_const.b_has_aff_blob;
    propagate_constant_expr_rewrite_one(p_const, p_expr, b_has_aff_blob)
}

/// A otimização de propagação de constantes do WHERE.
///
/// Se o WHERE contém termos COLUNA=CONSTANTE ou CONSTANTE=COLUNA que são termos
/// de nível superior ligados por AND e que não fazem parte de um ON de LEFT
/// JOIN, então em toda a consulta as outras ocorrências de COLUNA são trocadas
/// por CONSTANTE.
///
/// Por exemplo, a consulta:
///
///      SELECT * FROM t1, t2, t3 WHERE t1.a=39 AND t2.b=t1.a AND t3.c=t2.b
///
/// vira
///
///      SELECT * FROM t1, t2, t3 WHERE t1.a=39 AND t2.b=39 AND t3.c=39
///
/// Devolve verdadeiro se alguma transformação foi feita.
///
/// Nota de implementação: a propagação de constantes é delicada por causa da
/// interação entre afinidade e sequência de colação. Considere:
///
///    CREATE TABLE t1(a INT,b TEXT);
///    INSERT INTO t1 VALUES(123,'0123');
///    SELECT * FROM t1 WHERE a=123 AND b=a;
///    SELECT * FROM t1 WHERE a=123 AND b=123;
///
/// Os dois SELECT devem dar respostas diferentes. b=a é sempre verdadeira
/// porque a comparação usa afinidade numérica, mas b=123 é falsa porque usa
/// afinidade de texto e '0123' não é igual a '123'. Por isso a árvore não muda
/// de "b=a" para "b=123": o "a" de "b=a" ganha EP_FixedCol e o valor "123" é
/// pendurado em pLeft. O gerador de código sabe gerar a constante "123" no
/// lugar de buscar o valor da coluna. Para evitar problemas de colação, a
/// otimização só é tentada se o termo "a=123" usa a colação BINARY padrão.
///
/// Forum post 6a06202608 de 2021-05-25: outro caso traiçoeiro:
///
///    CREATE TABLE t1(x);
///    INSERT INTO t1 VALUES(10.0);
///    SELECT 1 FROM t1 WHERE x=10 AND x LIKE 10;
///
/// A consulta não deve devolver linhas, porque o valor de t1.x é '10.0' e não
/// '10', e '10.0' não é LIKE '10'. Sem cuidado, o termo "x=10" faria o segundo
/// virar "10 LIKE 10", um falso positivo. Para evitar, a propagação para
/// colunas de afinidade BLOB só é permitida se a constante é usada com os
/// operadores ==, <=, <, >=, > ou IS, de modo que as conversões de tipo
/// corretas aconteçam. Veja a lógica ligada ao flag `b_has_aff_blob`.
pub fn propagate_constants(p_parse: &ParseRef, p: &mut Select) -> i32 {
    let mut n_chng = 0;
    loop {
        let mut x = WhereConst {
            p_parse: p_parse.clone(),
            n_chng: 0,
            b_has_aff_blob: 0,
            m_exclude_on: 0,
            a_const: Vec::new(),
        };
        let right_join = match p.p_src.as_deref() {
            Some(p_src) => !p_src.a.is_empty() && (p_src.a[0].fg.jointype & JT_LTORJ) != 0,
            None => false,
        };
        if right_join {
            // Não propaga constantes em nenhuma cláusula ON se há um RIGHT JOIN
            // em qualquer lugar da consulta.
            x.m_exclude_on = EP_INNER_ON | EP_OUTER_ON;
        } else {
            // Não propaga constantes pela cláusula ON de um LEFT JOIN.
            x.m_exclude_on = EP_OUTER_ON;
        }
        find_const_in_where(&mut x, p.p_where.as_deref());
        let mut n_chng_round = 0;
        if !x.a_const.is_empty() {
            let mut w = Walker {
                p_parse: Some(p_parse.clone()),
                x_expr_callback: Some(propagate_constant_expr_rewrite),
                x_select_callback: Some(select_walk_noop),
                x_select_callback2: None,
                walker_depth: 0,
                e_code: 0,
                m_w_flags: 0,
                u: WalkerU::Const(Box::new(x)),
            };
            walk_expr(&mut w, p.p_where.as_deref_mut());
            if let WalkerU::Const(x) = w.u {
                n_chng_round = x.n_chng;
            }
            n_chng += n_chng_round;
        }
        if n_chng_round == 0 {
            break;
        }
    }
    n_chng
}

/// Determina se é seguro empurrar a expressão `p_expr` do WHERE para a
/// subconsulta `p_subq` da FROM, que contém pelo menos uma função de janela.
/// Devolve 1 se é seguro e a expressão deve ser empurrada, 0 senão.
///
/// Só é seguro empurrar a expressão se ela consiste apenas de constantes e de
/// cópias de expressões que aparecem na cláusula PARTITION BY de todas as
/// funções de janela usadas pela subconsulta. É seguro filtrar partições
/// inteiras, mas não linhas dentro de partições, pois isso pode mudar o
/// resultado das funções de janela.
///
/// No momento da chamada é garantido que
///
///   * a subconsulta usa só um quadro de janela distinto, e
///   * esse quadro de janela tem cláusula PARTITION BY.
fn push_down_window_check(p_parse: &ParseRef, p_subq: &mut Select, p_expr: &Expr) -> i32 {
    debug_assert!(p_subq.p_win.as_ref().unwrap().p_partition.is_some());
    debug_assert!((p_subq.sel_flags & SF_MULTIPART) == 0);
    debug_assert!(p_subq.p_prior.is_none());
    expr_is_constant_or_group_by(
        p_parse,
        p_expr,
        &mut p_subq.p_win.as_mut().unwrap().p_partition,
    )
}

/// Faz cópias dos termos relevantes do WHERE da consulta externa no WHERE da
/// subconsulta. Exemplo:
///
///    SELECT * FROM (SELECT a AS x, c-d AS y FROM t1) WHERE x=5 AND y=10;
///
/// Transformado em:
///
///    SELECT * FROM (SELECT a AS x, c-d AS y FROM t1 WHERE a=5 AND c-d=10)
///     WHERE x=5 AND y=10;
///
/// A esperança é que os termos acrescentados à consulta interna a tornem mais
/// eficiente.
///
/// AMBIGUIDADE DE NOME: esta é a "otimização de push-down do WHERE". Não a
/// confunda com a "otimização de push-down do MySQL", sem relação, em que os
/// termos do WHERE avaliáveis só com o índice rodam primeiro para evitar buscas
/// desnecessárias na tabela.
///
/// REGRAS: não se tenta a otimização se:
///
///   (2) A consulta interna é a parte recursiva de um CTE.
///   (3) A consulta interna tem LIMIT (mudaria o sentido do LIMIT).
///   (4) A consulta interna é o operando direito de um LEFT JOIN e a expressão
///       não vem do ON desse LEFT JOIN.
///   (5) A expressão do WHERE vem do ON ou USING de um LEFT JOIN em que
///       iCursor não é a tabela direita desse join.
///   (6) Funções de janela: (6a) a consulta interna usa várias partições de
///       janela incompatíveis; (6b) a interna é composta e usa funções de
///       janela; (6c) o WHERE não consiste inteiramente de constantes e cópias
///       de expressões do PARTITION BY de todas as funções de janela.
///   (7) A interna é um CTE que deve ser materializado (tratado no chamador).
///   (8) Se a subconsulta é composta com UNION, INTERSECT ou EXCEPT, todas as
///       colunas do resultado de todos os braços devem usar colação BINARY.
///   (9) A expressão vem do ON/USING de um join, a subconsulta está à direita
///       dele e há um RIGHT/FULL JOIN entre os dois.
///  (10) A consulta interna não é a tabela direita de um RIGHT JOIN.
///  (11) A subconsulta não é uma cláusula VALUES.
///  (12) Só com SQLITE_ALLOW_ROWID_IN_VIEW (desligado no Debian).
///
/// Devolve 0 se nada mudou e diferente de zero se um ou mais termos do WHERE
/// foram duplicados na subconsulta.
///
/// `p_subq` normalmente é `p_src_list.a[i_src].p_select`. Como o Rust não admite
/// as duas referências vivas, o chamador tira o select do item (`take`), chama
/// esta rotina e o devolve ao item.
pub fn push_down_where_terms(
    p_parse: &ParseRef,
    p_subq: &mut Select,
    p_where: &Expr,
    p_src_list: &SrcList,
    i_src: usize,
) -> i32 {
    let mut n_chng = 0;
    let p_src = &p_src_list.a[i_src];
    if (p_subq.sel_flags & (SF_RECURSIVE | SF_MULTIPART)) != 0 {
        return 0; // restrições (2) e (11)
    }
    if (p_src.fg.jointype & (JT_LTORJ | JT_RIGHT)) != 0 {
        return 0; // restrição (10)
    }

    if p_subq.p_prior.is_some() {
        let mut not_union_all = false;
        let mut p_sel = Some(&*p_subq);
        while let Some(s) = p_sel {
            let op = s.op;
            debug_assert!(
                op == TK_ALL || op == TK_SELECT || op == TK_UNION || op == TK_INTERSECT || op == TK_EXCEPT
            );
            if op != TK_ALL && op != TK_SELECT {
                not_union_all = true;
            }
            if s.p_win.is_some() {
                return 0; // restrição (6b)
            }
            p_sel = s.p_prior.as_deref();
        }
        if not_union_all {
            // Se algum braço do composto é ligado por UNION, INTERSECT ou
            // EXCEPT, nenhuma coluna pode usar colação diferente de BINARY.
            let mut p_sel = Some(&*p_subq);
            while let Some(s) = p_sel {
                let p_list = s.p_elist.as_deref().unwrap();
                for item in p_list.a.iter() {
                    let p_coll = expr_coll_seq(
                        &mut p_parse.borrow_mut(),
                        item.p_expr.as_deref().unwrap(),
                    );
                    let binary = match &p_coll {
                        None => true,
                        Some(c) => is_binary(Some(&c.borrow())) != 0,
                    };
                    if !binary {
                        return 0; // Restrição (8)
                    }
                }
                p_sel = s.p_prior.as_deref();
            }
        }
    } else if let Some(p_win) = p_subq.p_win.as_deref() {
        if p_win.p_partition.is_none() {
            return 0;
        }
    }

    // SQLITE_DEBUG: só o primeiro termo de um composto pode ter WITH; a
    // verificação de SF_Recursive nos demais é só asserção e fica abaixo.
    #[cfg(debug_assertions)]
    {
        let mut p_x = Some(&*p_subq);
        while let Some(s) = p_x {
            debug_assert!((s.sel_flags & SF_RECURSIVE) == 0);
            p_x = s.p_prior.as_deref();
        }
    }

    if p_subq.p_limit.is_some() {
        return 0; // restrição (3)
    }
    let mut p_where = p_where;
    while p_where.op == TK_AND {
        n_chng += push_down_where_terms(
            p_parse,
            p_subq,
            p_where.p_right.as_deref().unwrap(),
            p_src_list,
            i_src,
        );
        p_where = p_where.p_left.as_deref().unwrap();
    }

    // SQLITE_ALLOW_ROWID_IN_VIEW não está ligado no Debian: a restrição (12)
    // some. As verificações (4), (5) e (9) agora ficam em
    // expr_is_single_table_constraint().

    if expr_is_single_table_constraint(p_where, p_src_list, i_src as i32, 1) != 0 {
        n_chng += 1;
        p_subq.sel_flags |= SF_PUSHDOWN;
        let db = p_parse.borrow().db.upgrade().unwrap();
        let mut cur = Some(&mut *p_subq);
        while let Some(s) = cur {
            let mut p_new = expr_dup(&db.borrow(), p_where, 0);
            unset_join_expr(p_new.as_deref_mut(), -1, 1);
            let p_new = {
                let mut x = SubstContext {
                    p_parse: p_parse.clone(),
                    i_table: p_src.i_cursor,
                    i_new_table: p_src.i_cursor,
                    is_outer_join: 0,
                    p_e_list: s.p_elist.as_deref().unwrap(),
                    p_c_list: find_leftmost_expr_list(s).unwrap(),
                };
                subst_expr(&mut x, p_new)
            };
            let blocked = s.p_win.is_some()
                && p_new
                    .as_deref()
                    .map_or(false, |n| push_down_window_check(p_parse, s, n) == 0);
            if blocked {
                // A restrição 6c impediu o push-down neste caso.
                expr_delete(&db.borrow(), p_new);
                n_chng -= 1;
                break;
            }
            {
                let mut parse = p_parse.borrow_mut();
                if (s.sel_flags & SF_AGGREGATE) != 0 {
                    s.p_having = expr_and(&mut parse, s.p_having.take(), p_new);
                } else {
                    s.p_where = expr_and(&mut parse, s.p_where.take(), p_new);
                }
            }
            cur = s.p_prior.as_deref_mut();
        }
    }
    n_chng
}


// ---- part_011.rs ----

/// Verifica se uma subconsulta contém colunas do conjunto de resultados que
/// nunca são usadas. Se contém, troca o valor dessas colunas por NULL, para que
/// não causem trabalho desnecessário no cálculo.
///
/// Devolve o número de colunas que viraram NULL.
pub fn disable_unused_subquery_result_columns(p_item: &mut SrcItem) -> i32 {
    let mut n_chng: i32 = 0; // Número de colunas convertidas para NULL

    if p_item.fg.is_correlated != 0 || p_item.fg.is_cte != 0 {
        return 0;
    }
    let p_tab = p_item.p_tab.clone().unwrap();
    let mut col_used: Bitmask = p_item.col_used; // Colunas que não podem virar NULL
    let p_sub: &mut Select = p_item.p_select.as_deref_mut().unwrap();
    debug_assert!(
        p_sub.p_elist.as_deref().unwrap().a.len() == p_tab.borrow().n_col as usize
    );
    {
        let mut p_x = Some(&*p_sub);
        while let Some(x) = p_x {
            if (x.sel_flags & (SF_DISTINCT | SF_AGGREGATE)) != 0 {
                return 0;
            }
            if x.p_prior.is_some() && x.op != TK_ALL {
                // Esta otimização não funciona com subconsultas compostas que
                // usam UNION, INTERSECT ou EXCEPT. Só UNION ALL é permitido.
                return 0;
            }
            if x.p_win.is_some() {
                // Esta otimização não funciona com subconsultas que usam
                // funções de janela.
                return 0;
            }
            p_x = x.p_prior.as_deref();
        }
    }
    if let Some(p_list) = p_sub.p_order_by.as_deref() {
        for item in p_list.a.iter() {
            let mut i_col: u16 = match item.u {
                ExprListItemU::X { i_order_by_col, .. } => i_order_by_col,
                _ => 0,
            };
            if i_col > 0 {
                i_col -= 1;
                let shift = if (i_col as i32) >= BMS { BMS - 1 } else { i_col as i32 };
                col_used |= 1u64 << shift;
            }
        }
    }
    let n_col = p_tab.borrow().n_col as i32;
    for j in 0..n_col {
        let m: Bitmask = if j < BMS - 1 { 1u64 << j } else { TOPBIT };
        if (m & col_used) != 0 {
            continue;
        }
        let mut p_x = Some(&mut *p_sub);
        while let Some(x) = p_x {
            let changed = {
                let p_y = x.p_elist.as_deref_mut().unwrap().a[j as usize]
                    .p_expr
                    .as_deref_mut()
                    .unwrap();
                if p_y.op == TK_NULL {
                    false
                } else {
                    p_y.op = TK_NULL;
                    expr_clear_property(p_y, EP_SKIP | EP_UNLIKELY);
                    true
                }
            };
            if changed {
                x.sel_flags |= SF_PUSHDOWN;
                n_chng += 1;
            }
            p_x = x.p_prior.as_deref_mut();
        }
    }
    n_chng
}

/// `p_func` é a única função de agregação da consulta. Verifica se a consulta
/// é candidata à otimização min/max.
///
/// Se é, grava em `*pp_min_max` a cláusula ORDER BY a usar na otimização e
/// devolve WHERE_ORDERBY_MIN ou WHERE_ORDERBY_MAX conforme `p_func` seja min()
/// ou max().
///
/// Se não é, devolve WHERE_ORDERBY_NORMAL (que precisa ser zero).
///
/// Esta rotina precisa ser chamada depois de as funções de agregação serem
/// localizadas, mas antes de os argumentos delas passarem pela análise de
/// agregação.
pub fn min_max_query(db: &Sqlite3, p_func: &Expr, pp_min_max: &mut Option<Box<ExprList>>) -> u32 {
    let e_ret: u32;
    let mut sort_flags: u8 = 0;

    debug_assert!(pp_min_max.is_none());
    debug_assert!(p_func.op == TK_AGG_FUNCTION);
    debug_assert!(!is_window_func(p_func));
    debug_assert!(expr_use_x_list(p_func));
    let p_e_list = match p_func.x.p_list.as_deref() {
        Some(l) => l,
        None => return WHERE_ORDERBY_NORMAL,
    };
    if p_e_list.a.len() != 1
        || expr_has_property(p_func, EP_WIN_FUNC)
        || optimization_disabled(db, SQLITE_MIN_MAX_OPT)
    {
        return WHERE_ORDERBY_NORMAL;
    }
    debug_assert!(!expr_has_property(p_func, EP_INT_VALUE));
    let z_func: &[u8] = match p_func.u.z_token.as_deref() {
        Some(z) => z,
        None => return WHERE_ORDERBY_NORMAL,
    };
    if str_i_cmp(z_func, b"min") == 0 {
        e_ret = WHERE_ORDERBY_MIN;
        if expr_can_be_null(p_e_list.a[0].p_expr.as_deref().unwrap()) != 0 {
            sort_flags = KEYINFO_ORDER_BIGNULL;
        }
    } else if str_i_cmp(z_func, b"max") == 0 {
        e_ret = WHERE_ORDERBY_MAX;
        sort_flags = KEYINFO_ORDER_DESC;
    } else {
        return WHERE_ORDERBY_NORMAL;
    }
    *pp_min_max = expr_list_dup(db, p_e_list, 0);
    debug_assert!(pp_min_max.is_some() || db.malloc_failed != 0);
    if let Some(p_order_by) = pp_min_max.as_deref_mut() {
        p_order_by.a[0].fg.sort_flags = sort_flags;
    }
    e_ret
}

/// O SELECT passado como primeiro argumento é uma consulta agregada. O segundo
/// argumento é o objeto de informação de agregação associado. Esta função
/// testa se o SELECT tem a forma:
///
///   SELECT count(*) FROM <tbl>
///
/// onde a tabela é uma tabela de banco de dados, não um sub-select nem uma
/// visão. Se a consulta casa com o padrão, devolve a Table que representa
/// <tbl>. Senão, devolve None.
///
/// A rotina verifica se é seguro usar a otimização de contagem. Ainda se obtém
/// a resposta correta (talvez mais devagar) se ela devolve None quando podia
/// devolver a tabela. Mas devolver a tabela quando devia devolver None pode dar
/// respostas erradas ou quebrar. Na dúvida, devolve None.
pub fn is_simple_count(p: &Select, p_agg_info: &AggInfoRef) -> Option<TableRef> {
    debug_assert!(p.p_group_by.is_none());

    let p_e_list = p.p_elist.as_deref().unwrap();
    let p_src = p.p_src.as_deref().unwrap();
    if p.p_where.is_some()
        || p_e_list.a.len() != 1
        || p_src.a.len() != 1
        || p_src.a[0].p_select.is_some()
        || p_agg_info.borrow().n_func != 1
        || p.p_having.is_some()
    {
        return None;
    }
    let p_tab = p_src.a[0].p_tab.clone().unwrap();
    {
        let tab = p_tab.borrow();
        debug_assert!(!is_view(&tab));
        if !is_ordinary_table(&tab) {
            return None;
        }
    }
    let p_expr = p_e_list.a[0].p_expr.as_deref().unwrap();
    if p_expr.op != TK_AGG_FUNCTION {
        return None;
    }
    match &p_expr.p_agg_info {
        Some(a) if Rc::ptr_eq(a, p_agg_info) => {}
        _ => return None,
    }
    let func_flags = p_agg_info.borrow().a_func[0]
        .p_func
        .as_ref()
        .unwrap()
        .borrow()
        .func_flags;
    if (func_flags & SQLITE_FUNC_COUNT) == 0 {
        return None;
    }
    if expr_has_property(p_expr, EP_DISTINCT | EP_WIN_FUNC) {
        return None;
    }
    Some(p_tab)
}

/// Se o item da lista de origem passado como argumento foi aumentado com uma
/// cláusula INDEXED BY, tenta localizar o índice especificado. Se havia essa
/// cláusula e o índice nomeado não é achado, devolve SQLITE_ERROR e deixa um
/// erro em `p_parse`. Senão, preenche `p_from.u2` (`pIBIndex`) e devolve
/// SQLITE_OK.
pub fn indexed_by_lookup(p_parse: &mut Parse, p_from: &mut SrcItem) -> i32 {
    let p_tab = p_from.p_tab.clone().unwrap();
    let z_indexed_by: Vec<u8> = match &p_from.u1 {
        SrcItemU1::IndexedBy(z) => z.clone(),
        _ => Vec::new(),
    };
    debug_assert!(p_from.fg.is_indexed_by != 0);

    let mut p_idx = p_tab.borrow().p_index.clone();
    while let Some(idx) = p_idx.clone() {
        if str_i_cmp(&idx.borrow().z_name, &z_indexed_by) == 0 {
            break;
        }
        p_idx = idx.borrow().p_next.clone();
    }
    let p_idx = match p_idx {
        Some(i) => i,
        None => {
            error_msg(p_parse, b"no such index: %s", &[PrintfArg::Text(z_indexed_by)]);
            p_parse.check_schema = 1;
            return SQLITE_ERROR;
        }
    };
    debug_assert!(p_from.fg.is_cte == 0);
    p_from.u2 = SrcItemU2::IBIndex(p_idx);
    SQLITE_OK
}

/// Detecta SELECT compostos que usam ORDER BY com uma sequência de colação
/// alternativa.
///
///    SELECT ... FROM t1 EXCEPT SELECT ... FROM t2 ORDER BY .. COLLATE ...
///
/// Eles são reescritos como subconsulta:
///
///    SELECT * FROM (SELECT ... FROM t1 EXCEPT SELECT ... FROM t2)
///     ORDER BY ... COLLATE ...
///
/// A transformação é necessária porque a rotina multi_select_order_by(), que
/// gera o código de um SELECT composto com ORDER BY, usa um algoritmo de
/// intercalação que exige a mesma colação nas colunas do resultado e no ORDER
/// BY. Veja o ticket http://www.sqlite.org/src/info/6709574d2a
///
/// A transformação só é necessária para EXCEPT, INTERSECT e UNION. O operador
/// UNION ALL funciona bem com multi_select_order_by() mesmo com termos COLLATE
/// no ORDER BY.
///
/// Modelo sem ponteiros: no C o `*pNew = *p` copia o struct inteiro e depois
/// zera em `pNew` os campos que `p` conserva (GROUP BY, HAVING, ORDER BY,
/// LIMIT). Aqui os campos que ficam com o subselect novo são movidos de `p` e
/// os que `p` conserva simplesmente não saem dele. `pNew->pPrior->pNext = pNew`
/// (vínculo de volta) não é refeito. `p.p_win` é movido junto: neste ponto da
/// preparação ele ainda é `None`. Se a lista FROM nova não puder ser criada
/// (falta de memória), `p` já perdeu os campos movidos, e o chamador só aborta.
pub fn convert_compound_select_to_subquery(p_walker: &mut Walker, p: &mut Select) -> i32 {
    if p.p_prior.is_none() {
        return WRC_CONTINUE;
    }
    if p.p_order_by.is_none() {
        return WRC_CONTINUE;
    }
    {
        let mut p_x = Some(&*p);
        while let Some(x) = p_x {
            if !(x.op == TK_ALL || x.op == TK_SELECT) {
                break;
            }
            p_x = x.p_prior.as_deref();
        }
        if p_x.is_none() {
            return WRC_CONTINUE;
        }
    }
    let a = &p.p_order_by.as_deref().unwrap().a;
    // Se iOrderByCol já é diferente de zero, ele já foi casado com uma coluna
    // de resultado do SELECT. Isso ocorre quando o SELECT é reescrito para o
    // processamento de funções de janela e passa uma segunda vez por
    // select_prep() e afins. A reescrita desta função não é necessária então.
    if let ExprListItemU::X { i_order_by_col, .. } = a[0].u {
        if i_order_by_col != 0 {
            return WRC_CONTINUE;
        }
    }
    let mut found = false;
    for item in a.iter().rev() {
        if (item.p_expr.as_deref().unwrap().flags & EP_COLLATE) != 0 {
            found = true;
            break;
        }
    }
    if !found {
        return WRC_CONTINUE;
    }

    // Se chegamos aqui, a transformação é necessária.
    let p_parse = p_walker.p_parse.clone().unwrap();
    let mut parse = p_parse.borrow_mut();
    let db = parse.db.upgrade().unwrap();
    let p_new = Box::new(Select {
        op: p.op,
        n_select_row: p.n_select_row,
        sel_flags: p.sel_flags,
        i_limit: p.i_limit,
        i_offset: p.i_offset,
        sel_id: p.sel_id,
        addr_open_ephm: p.addr_open_ephm,
        p_elist: p.p_elist.take(),
        p_src: p.p_src.take(),
        p_where: p.p_where.take(),
        p_group_by: None,
        p_having: None,
        p_order_by: None,
        p_prior: p.p_prior.take(),
        p_next: p.p_next.take(),
        p_limit: None,
        p_with: p.p_with.take(),
        p_win: p.p_win.take(),
        p_win_defn: p.p_win_defn.take(),
    });
    let dummy = Token::default();
    let p_new_src = src_list_append_from_term(&mut parse, None, None, None, &dummy, Some(p_new), None);
    if p_new_src.is_none() {
        return WRC_ABORT;
    }
    p.p_src = p_new_src;
    let p_star = expr(&db.borrow(), TK_ASTERISK as i32, None);
    p.p_elist = expr_list_append(&mut parse, None, p_star);
    p.op = TK_SELECT;
    // p.p_where, p.p_prior, p.p_next, p.p_with e p.p_win_defn já saíram de `p`.
    p.sel_flags &= !SF_COMPOUND;
    debug_assert!((p.sel_flags & SF_CONVERTED) == 0);
    p.sel_flags |= SF_CONVERTED;
    WRC_CONTINUE
}

/// Verifica se o termo `p_from` da cláusula FROM tem argumentos de função de
/// tabela. Se tem, deixa uma mensagem de erro em `p_parse` e devolve diferente
/// de zero, pois `p_from` não pode ser uma função de tabela.
pub fn cannot_be_function(p_parse: &mut Parse, p_from: &SrcItem) -> i32 {
    if p_from.fg.is_tab_func != 0 {
        error_msg(p_parse, b"'%s' is not a function", &[PrintfArg::Text(p_from.z_name.clone())]);
        return 1;
    }
    0
}

/// O argumento `p_with` (que pode ser None) é uma lista encadeada de contextos
/// WITH aninhados, do mais interno ao mais externo. Se a tabela identificada
/// pelo elemento `p_item` da FROM é na verdade uma expressão de tabela comum
/// (CTE), devolve o contexto `With` a que a CTE pertence e o índice dela em
/// `With.a` (no C: o ponteiro para a CTE e o `*ppContext`). Senão, devolve None.
pub fn search_with(p_with: Option<&WithRef>, p_item: &SrcItem) -> Option<(WithRef, usize)> {
    let z_name: &[u8] = &p_item.z_name;
    debug_assert!(p_item.z_database.is_empty());
    debug_assert!(!z_name.is_empty());
    let mut p = p_with.cloned();
    while let Some(w) = p {
        {
            let wb = w.borrow();
            for i in 0..wb.n_cte as usize {
                if str_i_cmp(z_name, &wb.a[i].z_name) == 0 {
                    return Some((w.clone(), i));
                }
            }
            if wb.b_view != 0 {
                break;
            }
        }
        p = w.borrow().p_outer.clone();
    }
    None
}

/// O gerador de código mantém uma pilha de cláusulas WITH ativas, com a mais
/// interna no topo.
///
/// Esta rotina empilha a cláusula WITH passada como segundo argumento. Se
/// `b_free` é verdadeiro, essa cláusula nunca sai da pilha e é liberada junto
/// com o objeto Parse. Nos outros casos, com `b_free==0`, o objeto With é
/// liberado junto com o SELECT a que está associado (no modelo Rust, o `Rc`
/// guardado na limpeza do Parse segura o With vivo até o fim da análise).
///
/// A rotina devolve uma cópia de `p_with`. Ou, se `b_free` é verdadeiro e o
/// objeto é destruído na hora por falta de memória, devolve None.
pub fn with_push(p_parse: &mut Parse, p_with: Option<WithRef>, b_free: u8) -> Option<WithRef> {
    if let Some(w) = &p_with {
        if b_free != 0 {
            let keep = w.clone();
            let p_kept = parser_add_cleanup(
                p_parse,
                Box::new(move |_db: &Sqlite3Ref| {
                    drop(keep);
                }),
                Some(Vec::new()),
            );
            if p_kept.is_none() {
                return None;
            }
        }
        if p_parse.n_err == 0 {
            debug_assert!(match &p_parse.p_with {
                Some(o) => !Rc::ptr_eq(o, w),
                None => true,
            });
            w.borrow_mut().p_outer = p_parse.p_with.take();
            p_parse.p_with = Some(w.clone());
        }
    }
    p_with
}


// ---- part_012.rs ----

// Modelo adotado nesta parte (ver os tipos em sqliteInt_h):
//  - `ParseRef = Rc<RefCell<Parse>>` vive no `Walker.p_parse`; os empréstimos de `RefCell` são sempre
//    curtos e soltos antes de qualquer recursão do `Walker`, para não estourar o `BorrowMutError`.
//  - `Select.p_src`, `SrcItem.p_select`, `Select.p_prior` são `Option<Box<..>>`; o "ponteiro" `pRecTerm`
//    do C (percorre `p_prior`) vira um índice de profundidade resolvido por `prior_at`.
//  - `With` e `CteUse` são compartilhados: `WithRef = Rc<RefCell<With>>`, `CteUseRef = Rc<RefCell<CteUse>>`.
//    `search_with` devolve `(WithRef, índice da Cte)`.
//  - Texto NULL do C (`zName`, `zDatabase`, `zAlias`) é `Vec<u8>` vazio nos campos de `SrcItem`.

/// Desce `n` elos de `p_prior` a partir de `p` (o `pRecTerm = pRecTerm->pPrior` repetido do C).
fn prior_at(p: &mut Select, n: usize) -> &mut Select {
    let mut cur = p;
    for _ in 0..n {
        cur = cur.p_prior.as_mut().expect("p_prior").as_mut();
    }
    cur
}

/// Verifica se o argumento pFrom refere a uma CTE declarada pela cláusula WITH na pilha
/// mantida pelo analisador (na lista ligada pParse.p_with). Se estiver processando uma
/// expressão de CTE, verifica se a referência é uma referência recursiva à CTE.
///
/// Se pFrom corresponde a uma CTE conforme um desses dois casos acima, pFrom.p_tab
/// e outros campos são preenchidos adequadamente. O termo FROM é `p_tab_list.a[i_from]`.
///
/// Retorna 0 se nenhuma correspondência é encontrada.
/// Retorna 1 se uma correspondência é encontrada.
/// Retorna 2 se uma condição de erro é detectada.
fn resolve_from_term_to_cte(
    p_parse: &ParseRef,
    p_walker: &mut Walker,
    p_tab_list: &mut SrcList,
    i_from: usize,
) -> i32 {
    debug_assert!(p_tab_list.a[i_from].p_tab.is_none());
    let p_parse_with = p_parse.borrow().p_with.clone();
    let p_parse_with = match p_parse_with {
        // Não há cláusulas WITH na pilha. Nenhuma correspondência é possível.
        None => return 0,
        Some(w) => w,
    };
    if p_parse.borrow().n_err != 0 {
        // Erros anteriores podem ter deixado pParse.p_with num estado ruim, então
        // não vá adiante.
        return 0;
    }
    if !p_tab_list.a[i_from].z_database.is_empty() {
        // O termo FROM contém um qualificador de schema (ex: main.t1) e portanto
        // não pode ser uma referência a CTE.
        return 0;
    }
    if p_tab_list.a[i_from].fg.not_cte != 0 {
        // O termo FROM está especificamente excluído de corresponder a uma CTE.
        //   (1)  Faz parte de um trigger que costumava ter zDatabase mas teve
        //        zDatabase removido por sqlite3FixTriggerStep().
        //   (2)  Este é o primeiro termo na cláusula FROM de um UPDATE.
        return 0;
    }
    let (p_with, i_cte) = match search_with(&p_parse_with, &p_tab_list.a[i_from]) {
        None => return 0, // Nenhuma correspondência
        Some(found) => found,
    };
    let db = p_parse.borrow().db.upgrade().expect("db");
    let z_cte_name: Vec<u8> = p_with.borrow().a[i_cte].z_name.clone();

    // Se pCte.z_cte_err não é NULL neste ponto, então esta é uma referência
    // recursiva ilegal à CTE pCte. Deixe um erro em pParse e retorne cedo.
    // Se pCte.z_cte_err é NULL, então esta não é uma referência recursiva.
    // Neste caso, prossiga.
    let z_cte_err = p_with.borrow().a[i_cte].z_cte_err.clone();
    if let Some(z_fmt) = z_cte_err {
        error_msg(
            &mut p_parse.borrow_mut(),
            &z_fmt,
            &[PrintfArg::Text(&z_cte_name)],
        );
        return 2;
    }
    if cannot_be_function(&mut p_parse.borrow_mut(), &p_tab_list.a[i_from]) != 0 {
        return 2;
    }

    debug_assert!(p_tab_list.a[i_from].p_tab.is_none());
    let p_tab: TableRef = Rc::new(RefCell::new(Table::default()));
    let existing_use = p_with.borrow().a[i_cte].p_use.clone();
    let p_cte_use: CteUseRef = match existing_use {
        Some(u) => u,
        None => {
            // O sqlite3ParserAddCleanup(sqlite3DbFree) do C vira o Drop do Rc.
            let u: CteUseRef = Rc::new(RefCell::new(CteUse::default()));
            p_with.borrow_mut().a[i_cte].p_use = Some(u.clone());
            let e_m10d = p_with.borrow().a[i_cte].e_m10d;
            u.borrow_mut().e_m10d = e_m10d;
            u
        }
    };
    {
        let p_from = &mut p_tab_list.a[i_from];
        p_from.p_tab = Some(p_tab.clone());
        {
            let mut t = p_tab.borrow_mut();
            t.n_tab_ref = 1;
            t.z_name = z_cte_name.clone();
            t.i_p_key = -1;
            t.n_row_log_est = 200;
            debug_assert!(200 == log_est(1048576));
            t.tab_flags |= TF_EPHEMERAL | TF_NO_VISIBLE_ROWID;
        }
        let dup = {
            let w = p_with.borrow();
            select_dup(&db, w.a[i_cte].p_select.as_deref(), 0)
        };
        p_from.p_select = dup;
        if db.borrow().malloc_failed != 0 {
            return 2;
        }
        p_from.p_select.as_mut().expect("p_select").sel_flags |= SF_COPYCTE;
        debug_assert!(p_from.p_select.is_some());
        if p_from.fg.is_indexed_by != 0 {
            let z_indexed_by: Vec<u8> = match &p_from.u1 {
                SrcItemU1::IndexedBy(z) => z.clone(),
                _ => Vec::new(),
            };
            error_msg(
                &mut p_parse.borrow_mut(),
                b"no such index: \"%s\"",
                &[PrintfArg::Text(&z_indexed_by)],
            );
            return 2;
        }
        p_from.fg.is_cte = 1;
        p_from.u2 = SrcItemU2::CteUse(p_cte_use.clone());
        p_cte_use.borrow_mut().n_use += 1;
    }

    // Verifica se é uma CTE recursiva.
    let p_sel: &mut Select = p_tab_list.a[i_from].p_select.as_mut().expect("p_select").as_mut();
    let sel_op = p_sel.op;
    let b_may_recursive = sel_op == TK_ALL || sel_op == TK_UNION;
    let mut i_rec_tab: i32 = -1; // Cursor para tabela recursiva
    let mut depth: usize = 0; // pRecTerm = prior_at(p_sel, depth)
    while b_may_recursive && prior_at(p_sel, depth).op == sel_op {
        let p_rec_term = prior_at(p_sel, depth);
        debug_assert!(p_rec_term.p_prior.is_some());
        let n_src = p_rec_term.p_src.as_ref().map_or(0, |s| s.n_src);
        for i in 0..n_src as usize {
            let p_item = &mut p_rec_term.p_src.as_mut().expect("p_src").a[i];
            if p_item.z_database.is_empty()
                && !p_item.z_name.is_empty()
                && 0 == str_i_cmp(&p_item.z_name, &z_cte_name)
            {
                p_item.p_tab = Some(p_tab.clone());
                p_tab.borrow_mut().n_tab_ref += 1;
                p_item.fg.is_recursive = 1;
                if (p_rec_term.sel_flags & SF_RECURSIVE) != 0 {
                    error_msg(
                        &mut p_parse.borrow_mut(),
                        b"multiple references to recursive table: %s",
                        &[PrintfArg::Text(&z_cte_name)],
                    );
                    return 2;
                }
                p_rec_term.sel_flags |= SF_RECURSIVE;
                if i_rec_tab < 0 {
                    let mut pp = p_parse.borrow_mut();
                    i_rec_tab = pp.n_tab;
                    pp.n_tab += 1;
                }
                p_rec_term.p_src.as_mut().expect("p_src").a[i].i_cursor = i_rec_tab;
            }
        }
        if (p_rec_term.sel_flags & SF_RECURSIVE) == 0 {
            break;
        }
        depth += 1;
    }

    p_with.borrow_mut().a[i_cte].z_cte_err = Some(b"circular reference: %s".to_vec());
    let p_saved_with = p_parse.borrow().p_with.clone(); // Valor inicial de pParse.p_with
    p_parse.borrow_mut().p_with = Some(p_with.clone());
    if (p_sel.sel_flags & SF_RECURSIVE) != 0 {
        let p_sel_with = p_sel.p_with.clone();
        let p_rec_term = prior_at(p_sel, depth);
        debug_assert!((p_rec_term.sel_flags & SF_RECURSIVE) == 0);
        debug_assert!(p_rec_term.p_with.is_none());
        p_rec_term.p_with = p_sel_with;
        let rc = walk_select(p_walker, p_rec_term);
        p_rec_term.p_with = None;
        if rc != 0 {
            p_parse.borrow_mut().p_with = p_saved_with;
            return 2;
        }
    } else if walk_select(p_walker, p_sel) != 0 {
        p_parse.borrow_mut().p_with = p_saved_with;
        return 2;
    }
    p_parse.borrow_mut().p_with = Some(p_with.clone());

    // pLeft: SELECT mais à esquerda (segue p_prior até o fim)
    let mut p_left: &Select = p_sel;
    while let Some(prior) = p_left.p_prior.as_deref() {
        p_left = prior;
    }
    let p_cols_guard = p_with.borrow();
    let p_cols: Option<&ExprList> = p_cols_guard.a[i_cte].p_cols.as_deref();
    let mut p_elist: Option<&ExprList> = p_left.p_elist.as_deref();
    if let Some(cols) = p_cols {
        if let Some(el) = p_elist {
            if el.n_expr != cols.n_expr {
                error_msg(
                    &mut p_parse.borrow_mut(),
                    b"table %s has %d values for %d columns",
                    &[
                        PrintfArg::Text(&z_cte_name),
                        PrintfArg::Int(el.n_expr as i64),
                        PrintfArg::Int(cols.n_expr as i64),
                    ],
                );
                p_parse.borrow_mut().p_with = p_saved_with;
                return 2;
            }
        }
        p_elist = Some(cols);
    }
    {
        let mut t = p_tab.borrow_mut();
        let t = &mut *t;
        columns_from_expr_list(&mut p_parse.borrow_mut(), p_elist, &mut t.n_col, &mut t.a_col);
    }
    drop(p_cols_guard);
    if b_may_recursive {
        let z_err: &[u8] = if (p_sel.sel_flags & SF_RECURSIVE) != 0 {
            b"multiple recursive references: %s"
        } else {
            b"recursive reference in a subquery: %s"
        };
        p_with.borrow_mut().a[i_cte].z_cte_err = Some(z_err.to_vec());
        walk_select(p_walker, p_sel);
    }
    p_with.borrow_mut().a[i_cte].z_cte_err = None;
    p_parse.borrow_mut().p_with = p_saved_with;
    1 // Sucesso
}

/// Se o SELECT passado como segundo argumento tem uma cláusula WITH associada,
/// faça pop dela da pilha armazenada como parte do objeto Parse.
///
/// Esta função é usada como o callback xSelectCallback2() por
/// sqlite3SelectExpand() ao caminhar uma árvore SELECT para resolver nomes de
/// tabelas e outros elementos da cláusula FROM.
pub fn select_pop_with(p_walker: &mut Walker, p: &mut Select) {
    let p_parse = p_walker.p_parse.clone().expect("p_parse");
    let tem_with = p_parse.borrow().p_with.is_some();
    if tem_with && p.p_prior.is_none() {
        if let Some(p_with) = find_rightmost(p).p_with.clone() {
            debug_assert!(
                p_parse
                    .borrow()
                    .p_with
                    .as_ref()
                    .map_or(false, |w| Rc::ptr_eq(w, &p_with))
                    || p_parse.borrow().n_err != 0
            );
            let outer = p_with.borrow().p_outer.clone();
            p_parse.borrow_mut().p_with = outer;
        }
    }
}

/// O objeto SrcItem passado como segundo argumento representa uma subconsulta
/// na cláusula FROM de uma instrução SELECT. Esta função aloca e popula o
/// objeto SrcItem.p_tab. Se bem sucedido, SQLITE_OK é retornado. Caso contrário,
/// se um erro de OOM é encontrado, SQLITE_NOMEM.
pub fn expand_subquery(p_parse: &mut Parse, p_from: &mut SrcItem) -> i32 {
    let db = p_parse.db.upgrade().expect("db");
    debug_assert!(p_from.p_select.is_some());
    let p_tab: TableRef = Rc::new(RefCell::new(Table::default()));
    p_from.p_tab = Some(p_tab.clone());
    let mut t = p_tab.borrow_mut();
    t.n_tab_ref = 1;
    if !p_from.z_alias.is_empty() {
        t.z_name = p_from.z_alias.clone();
    } else {
        t.z_name = m_printf_item_name(&db, p_from);
    }
    {
        let mut p_sel: &Select = p_from.p_select.as_deref().expect("p_select");
        while let Some(prior) = p_sel.p_prior.as_deref() {
            p_sel = prior;
        }
        let t = &mut *t;
        columns_from_expr_list(p_parse, p_sel.p_elist.as_deref(), &mut t.n_col, &mut t.a_col);
    }
    t.i_p_key = -1;
    t.e_tab_type = TABTYP_VIEW;
    t.n_row_log_est = 200;
    debug_assert!(200 == log_est(1048576));
    // O caso usual (sem SQLITE_ALLOW_ROWID_IN_VIEW): não permitir ROWID numa subconsulta
    t.tab_flags |= TF_EPHEMERAL | TF_NO_VISIBLE_ROWID;
    if p_parse.n_err != 0 {
        SQLITE_ERROR
    } else {
        SQLITE_OK
    }
}

/// Verifica os N objetos SrcItem à direita de pBase. (N pode ser zero!)
/// Se algum desses N objetos SrcItem tiver uma cláusula USING contendo zName
/// retorna verdadeiro.
///
/// Se N é zero, ou nenhum dos N objetos SrcItem à direita de pBase
/// contém uma cláusula USING, ou se nenhuma das cláusulas USING contém zName,
/// retorna falso.
///
/// `items[0]` é o próprio pBase; os candidatos são `items[1..=n]`.
fn in_any_using_clause(z_name: &[u8], items: &[SrcItem], n: i32) -> i32 {
    for item in items.iter().skip(1).take(n.max(0) as usize) {
        if item.fg.is_using == 0 {
            continue;
        }
        if let SrcItemU3::Using(p_using) = &item.u3 {
            if id_list_index(p_using, z_name) >= 0 {
                return 1;
            }
        }
    }
    0
}

/// Esta rotina é um callback Walker para "expandir" uma instrução SELECT.
/// "Expandir" significa fazer o seguinte:
///
///    (1)  Garanta que números de cursor VDBE foram atribuídos a cada
///         elemento da cláusula FROM.
///
///    (2)  Preencha os campos pTabList.a[].p_tab na SrcList que
///         define a cláusula FROM. Quando visões aparecem na cláusula FROM,
///         preencha pTabList.a[].p_select com uma cópia da instrução SELECT
///         que implementa a visão. Uma cópia é feita da instrução SELECT
///         da visão para que possamos modificá-la ou apagá-la livremente
///         sem nos preocupar em danificar a representação persistente
///         da visão.
///
///    (3)  Adicione termos à cláusula WHERE para acomodar a palavra chave NATURAL
///         em joins e as cláusulas ON e USING de joins.
///
///    (4)  Examine a lista de colunas no conjunto de resultados (pEList) procurando
///         por instâncias do operador "*" ou do operador TABLE.*
///         Se encontrado, expanda cada "*" para ser toda coluna em cada tabela
///         e TABLE.* para ser toda coluna em TABLE.
pub fn select_expander(p_walker: &mut Walker, p: &mut Select) -> i32 {
    let p_parse = p_walker.p_parse.clone().expect("p_parse");
    let db = p_parse.borrow().db.upgrade().expect("db");
    let sel_flags = p.sel_flags;
    let mut elist_flags: u32 = 0;

    p.sel_flags |= SF_EXPANDED;
    if db.borrow().malloc_failed != 0 {
        return WRC_ABORT;
    }
    debug_assert!(p.p_src.is_some());
    if (sel_flags & SF_EXPANDED) != 0 {
        return WRC_PRUNE;
    }
    if p_walker.e_code != 0 {
        // Renumere selId porque foi copiado de uma visão
        let mut pp = p_parse.borrow_mut();
        pp.n_select += 1;
        p.sel_id = pp.n_select as u32;
    }
    let tem_with = p_parse.borrow().p_with.is_some();
    if tem_with && (p.sel_flags & SF_VIEW) != 0 {
        if p.p_with.is_none() {
            p.p_with = Some(Rc::new(RefCell::new(With::default())));
        }
        p.p_with.as_ref().expect("p_with").borrow_mut().b_view = 1;
    }
    with_push(&mut p_parse.borrow_mut(), p.p_with.clone(), 0);

    // Garanta que números de cursor foram atribuídos a todos os itens
    // da cláusula FROM da instrução SELECT.
    src_list_assign_cursors(&mut p_parse.borrow_mut(), p.p_src.as_deref_mut());

    // Procure cada tabela nomeada na cláusula FROM do select. Se
    // uma entrada da cláusula FROM é uma subconsulta em vez de uma tabela ou visão,
    // crie uma estrutura de tabela transitória para descrever a subconsulta.
    let n_src = p.p_src.as_ref().expect("p_src").n_src as usize;
    for i in 0..n_src {
        let p_tab_list: &mut SrcList = p.p_src.as_mut().expect("p_src");
        debug_assert!(p_tab_list.a[i].fg.is_recursive == 0 || p_tab_list.a[i].p_tab.is_some());
        if p_tab_list.a[i].p_tab.is_some() {
            continue;
        }
        debug_assert!(p_tab_list.a[i].fg.is_recursive == 0);
        if p_tab_list.a[i].z_name.is_empty() {
            // Uma subconsulta na cláusula FROM de um SELECT
            let p_from = &mut p_tab_list.a[i];
            debug_assert!(p_from.p_select.is_some());
            debug_assert!(p_from.p_tab.is_none());
            if walk_select(p_walker, p_from.p_select.as_mut().expect("p_select")) != 0 {
                return WRC_ABORT;
            }
            if expand_subquery(&mut p_parse.borrow_mut(), p_from) != 0 {
                return WRC_ABORT;
            }
        } else {
            let rc = resolve_from_term_to_cte(&p_parse, p_walker, p_tab_list, i);
            if rc != 0 {
                if rc > 1 {
                    return WRC_ABORT;
                }
                debug_assert!(p_tab_list.a[i].p_tab.is_some());
            } else {
                // Uma tabela ou visão ordinária na cláusula FROM
                let p_from = &mut p_tab_list.a[i];
                debug_assert!(p_from.p_tab.is_none());
                let p_tab = locate_table_item(&mut p_parse.borrow_mut(), 0, p_from);
                let p_tab = match p_tab {
                    None => return WRC_ABORT,
                    Some(t) => t,
                };
                p_from.p_tab = Some(p_tab.clone());
                if p_tab.borrow().n_tab_ref >= 0xffff {
                    let z_name = p_tab.borrow().z_name.clone();
                    error_msg(
                        &mut p_parse.borrow_mut(),
                        b"too many references to \"%s\": max 65535",
                        &[PrintfArg::Text(&z_name)],
                    );
                    p_from.p_tab = None;
                    return WRC_ABORT;
                }
                p_tab.borrow_mut().n_tab_ref += 1;
                if !is_virtual(&p_tab.borrow())
                    && cannot_be_function(&mut p_parse.borrow_mut(), p_from) != 0
                {
                    return WRC_ABORT;
                }
                if !is_ordinary_table(&p_tab.borrow()) {
                    let e_code_orig = p_walker.e_code;
                    if view_get_column_names(&mut p_parse.borrow_mut(), &p_tab) != 0 {
                        return WRC_ABORT;
                    }
                    debug_assert!(p_from.p_select.is_none());
                    if is_view(&p_tab.borrow()) {
                        let schema_ok = {
                            let d = db.borrow();
                            let tab = p_tab.borrow();
                            let same_temp = match (&tab.p_schema, &d.a_db[1].p_schema) {
                                (Some(a), Some(b)) => a.upgrade().map_or(false, |a| Rc::ptr_eq(&a, b)),
                                (None, None) => true,
                                _ => false,
                            };
                            (d.flags & SQLITE_ENABLE_VIEW) != 0 || same_temp
                        };
                        if !schema_ok {
                            let z_name = p_tab.borrow().z_name.clone();
                            error_msg(
                                &mut p_parse.borrow_mut(),
                                b"access to view \"%s\" prohibited",
                                &[PrintfArg::Text(&z_name)],
                            );
                        }
                        p_from.p_select = {
                            let tab = p_tab.borrow();
                            match &tab.u {
                                TableU::View(v) => select_dup(&db, v.p_select.as_deref(), 0),
                                _ => None,
                            }
                        };
                    } else if is_virtual(&p_tab.borrow()) && p_from.fg.from_ddl != 0 {
                        let risk = {
                            let tab = p_tab.borrow();
                            match &tab.u {
                                TableU::VTab(v) => v.p.as_ref().map(|vt| vt.borrow().e_vtab_risk),
                                _ => None,
                            }
                        };
                        let trusted = ((db.borrow().flags & SQLITE_TRUSTED_SCHEMA) != 0) as u8;
                        if let Some(risk) = risk {
                            if risk > trusted {
                                let z_name = p_tab.borrow().z_name.clone();
                                error_msg(
                                    &mut p_parse.borrow_mut(),
                                    b"unsafe use of virtual table \"%s\"",
                                    &[PrintfArg::Text(&z_name)],
                                );
                            }
                        }
                    }
                    debug_assert!(SQLITE_VTABRISK_NORMAL == 1 && SQLITE_VTABRISK_HIGH == 2);
                    let n_col = p_tab.borrow().n_col;
                    p_tab.borrow_mut().n_col = -1;
                    p_walker.e_code = 1; // Liga a renumeração de Select.selId
                    // sqlite3WalkSelect(pWalker, NULL) é no-op no C
                    if let Some(sel) = p_from.p_select.as_mut() {
                        walk_select(p_walker, sel);
                    }
                    p_walker.e_code = e_code_orig;
                    p_tab.borrow_mut().n_col = n_col;
                }
            }
        }

        // Localize o índice nomeado pela cláusula INDEXED BY, se houver.
        let p_from = &mut p.p_src.as_mut().expect("p_src").a[i];
        if p_from.fg.is_indexed_by != 0
            && indexed_by_lookup(&mut p_parse.borrow_mut(), p_from) != 0
        {
            return WRC_ABORT;
        }
    }

    // Processe palavras chave NATURAL, e cláusulas ON e USING de joins.
    debug_assert!(db.borrow().malloc_failed == 0 || p_parse.borrow().n_err != 0);
    if p_parse.borrow().n_err != 0 || process_join(&mut p_parse.borrow_mut(), p) != 0 {
        return WRC_ABORT;
    }

    // Para todo "*" que ocorre na lista de colunas, insira os nomes de
    // todas as colunas em todas as tabelas. E para todo TABLE.* insira os nomes
    // de todas as colunas em TABLE. O analisador inseriu uma expressão especial
    // com o operador TK_ASTERISK para cada "*" que encontrou na lista de colunas.
    // O código a seguir precisa apenas localizar as expressões TK_ASTERISK
    // e expandir cada uma para a lista de todas as colunas em
    // todas as tabelas.
    //
    // O primeiro laço apenas verifica se existem operadores "*"
    // que precisam ser expandidos.
    let n_expr_ini = p.p_elist.as_ref().map_or(0, |el| el.n_expr);
    let mut k: i32 = 0;
    while k < n_expr_ini {
        let p_e = p.p_elist.as_ref().expect("p_elist").a[k as usize].p_expr.as_ref().expect("p_expr");
        if p_e.op == TK_ASTERISK {
            break;
        }
        debug_assert!(p_e.op != TK_DOT || p_e.p_right.is_some());
        debug_assert!(
            p_e.op != TK_DOT
                || (p_e.p_left.is_some() && p_e.p_left.as_ref().expect("p_left").op == TK_ID)
        );
        if p_e.op == TK_DOT && p_e.p_right.as_ref().expect("p_right").op == TK_ASTERISK {
            break;
        }
        elist_flags |= p_e.flags;
        k += 1;
    }
    if k < n_expr_ini {
        // Se chegamos aqui, o conjunto de resultados contém um ou mais operadores "*"
        // que precisam ser expandidos. Percorra cada expressão do conjunto de
        // resultados e expanda uma a uma.
        let mut a: Vec<ExprListItem> = std::mem::take(&mut p.p_elist.as_mut().expect("p_elist").a);
        let n_expr = a.len();
        let mut p_new: Option<Box<ExprList>> = None;
        let flags = db.borrow().flags;
        let long_names = (flags & SQLITE_FULL_COL_NAMES) != 0 && (flags & SQLITE_SHORT_COL_NAMES) == 0;
        let in_rename = in_rename_object(&p_parse.borrow());

        for k in 0..n_expr {
            let p_e_op = a[k].p_expr.as_ref().expect("p_expr").op;
            elist_flags |= a[k].p_expr.as_ref().expect("p_expr").flags;
            let p_right_op = a[k].p_expr.as_ref().expect("p_expr").p_right.as_ref().map(|r| r.op);
            debug_assert!(p_e_op != TK_DOT || p_right_op.is_some());
            if p_e_op != TK_ASTERISK && (p_e_op != TK_DOT || p_right_op != Some(TK_ASTERISK)) {
                // Esta expressão em particular não precisa ser expandida.
                let p_expr_k = a[k].p_expr.take();
                p_new = expr_list_append(&mut p_parse.borrow_mut(), p_new, p_expr_k);
                if let Some(n) = p_new.as_mut() {
                    let last = n.a.last_mut().expect("last");
                    last.z_e_name = a[k].z_e_name.take();
                    last.fg.e_e_name = a[k].fg.e_e_name;
                }
            } else {
                // Esta expressão é um "*" ou um "TABLE.*" e precisa ser expandida.
                let mut table_seen = false; // Vira verdadeiro quando TABLE confere
                let z_t_name: Option<Vec<u8>>; // texto do nome de TABLE
                let i_err_ofst: i32;
                {
                    let p_e = a[k].p_expr.as_ref().expect("p_expr");
                    if p_e.op == TK_DOT {
                        debug_assert!((sel_flags & SF_NESTEDFROM) == 0);
                        debug_assert!(p_e.p_left.is_some());
                        z_t_name = match &p_e.p_left.as_ref().expect("p_left").u {
                            ExprU::Token(z) => Some(z.clone()),
                            _ => None,
                        };
                        i_err_ofst = p_e.p_right.as_ref().expect("p_right").w_ofst();
                    } else {
                        z_t_name = None;
                        i_err_ofst = p_e.w_ofst();
                    }
                }
                let n_from = p.p_src.as_ref().expect("p_src").n_src as usize;
                for i in 0..n_from {
                    let p_tab_list: &SrcList = p.p_src.as_ref().expect("p_src");
                    let p_from = &p_tab_list.a[i];
                    let p_tab_rc: TableRef = p_from.p_tab.clone().expect("p_tab"); // Tabela desta fonte de dados
                    let p_tab = p_tab_rc.borrow();
                    let z_tab_name: Vec<u8> = if !p_from.z_alias.is_empty() {
                        p_from.z_alias.clone()
                    } else {
                        p_tab.z_name.clone()
                    };
                    if db.borrow().malloc_failed != 0 {
                        break;
                    }
                    debug_assert!(
                        (p_from.fg.is_nested_from != 0) == is_nested_from(p_from.p_select.as_deref())
                    );
                    let p_nested_from: Option<&ExprList>; // Conjunto de resultados de um FROM aninhado
                    let mut z_schema_name: Option<Vec<u8>> = None; // Nome do schema desta fonte
                    if p_from.fg.is_nested_from != 0 {
                        debug_assert!(p_from.p_select.is_some());
                        p_nested_from = p_from.p_select.as_ref().expect("p_select").p_elist.as_deref();
                        debug_assert!(p_nested_from.is_some());
                        debug_assert!(p_nested_from.expect("nf").n_expr == p_tab.n_col as i32);
                        debug_assert!(!visible_rowid(&p_tab) || VIEW_CAN_HAVE_ROWID);
                    } else {
                        if let Some(zt) = &z_t_name {
                            if str_i_cmp(zt, &z_tab_name) != 0 {
                                continue;
                            }
                        }
                        p_nested_from = None;
                        let i_db = schema_to_index(&db.borrow(), p_tab.p_schema.as_ref().and_then(|w| w.upgrade()).as_ref().map(|s| &*s.borrow()));
                        z_schema_name = Some(if i_db >= 0 {
                            db.borrow().a_db[i_db as usize].z_db_s_name.clone()
                        } else {
                            b"*".to_vec()
                        });
                    }
                    let p_using: Option<&IdList>; // Cláusula USING de pFrom[1]
                    if i + 1 < p_tab_list.n_src as usize
                        && p_tab_list.a[i + 1].fg.is_using != 0
                        && (sel_flags & SF_NESTEDFROM) != 0
                    {
                        let p_u = match &p_tab_list.a[i + 1].u3 {
                            SrcItemU3::Using(u) => u.as_ref(),
                            _ => unreachable!("is_using sem USING"),
                        };
                        for ii in 0..p_u.n_id as usize {
                            let z_u_name = &p_u.a[ii].z_name;
                            let mut p_right = expr(&db, TK_ID as i32, Some(z_u_name));
                            expr_set_error_offset(p_right.as_deref_mut(), i_err_ofst);
                            p_new = expr_list_append(&mut p_parse.borrow_mut(), p_new, p_right);
                            if let Some(n) = p_new.as_mut() {
                                let p_x = n.a.last_mut().expect("last");
                                debug_assert!(p_x.z_e_name.is_none());
                                let mut z = b"..".to_vec();
                                z.extend_from_slice(z_u_name);
                                p_x.z_e_name = Some(z);
                                p_x.fg.e_e_name = ENAME_TAB;
                                p_x.fg.b_using_term = 1;
                            }
                        }
                        p_using = Some(p_u);
                    } else {
                        p_using = None;
                    }

                    let mut n_add = p_tab.n_col as i32; // Número de colunas incluindo rowid
                    if visible_rowid(&p_tab) && (sel_flags & SF_NESTEDFROM) != 0 {
                        n_add += 1;
                    }
                    for j in 0..n_add {
                        let z_name: Vec<u8>;
                        if j == p_tab.n_col as i32 {
                            match rowid_alias(&p_tab) {
                                None => continue,
                                Some(z) => z_name = z.to_vec(),
                            }
                        } else {
                            z_name = p_tab.a_col[j as usize].z_cn_name.clone();

                            // Se pTab é na verdade uma subconsulta SF_NestedFrom, não
                            // expanda nenhuma coluna ENAME_ROWID.
                            if let Some(nf) = p_nested_from {
                                if nf.a[j as usize].fg.e_e_name == ENAME_ROWID {
                                    continue;
                                }
                            }
                            if let (Some(zt), Some(nf)) = (&z_t_name, p_nested_from) {
                                if match_e_name(&nf.a[j as usize], None, Some(zt), None, None) == 0 {
                                    continue;
                                }
                            }

                            // Se uma coluna é marcada como 'hidden', omita-a da lista de
                            // resultados expandida, a menos que o SELECT tenha o bit
                            // SF_IncludeHidden ligado.
                            if (p.sel_flags & SF_INCLUDEHIDDEN) == 0
                                && is_hidden_column(&p_tab.a_col[j as usize])
                            {
                                continue;
                            }
                            if (p_tab.a_col[j as usize].col_flags & COLFLAG_NOEXPAND) != 0
                                && z_t_name.is_none()
                                && (sel_flags & SF_NESTEDFROM) == 0
                            {
                                continue;
                            }
                        }
                        debug_assert!(!z_name.is_empty());
                        table_seen = true;

                        if i > 0 && z_t_name.is_none() && (sel_flags & SF_NESTEDFROM) == 0 {
                            if p_from.fg.is_using != 0 {
                                if let SrcItemU3::Using(u) = &p_from.u3 {
                                    if id_list_index(u, &z_name) >= 0 {
                                        // Num join com cláusula USING, omita colunas da
                                        // cláusula using da tabela à direita.
                                        continue;
                                    }
                                }
                            }
                        }
                        let p_right = expr(&db, TK_ID as i32, Some(&z_name));
                        let p_expr: Option<Box<Expr>>;
                        if (p_tab_list.n_src > 1
                            && ((p_from.fg.jointype & JT_LTORJ) == 0
                                || (sel_flags & SF_NESTEDFROM) != 0
                                || in_any_using_clause(&z_name, &p_tab_list.a[i..], p_tab_list.n_src - i as i32 - 1) == 0))
                            || in_rename
                        {
                            let mut p_left = expr(&db, TK_ID as i32, Some(&z_tab_name));
                            let remap_left = p_left.as_deref().map(|e| e as *const Expr as usize);
                            let mut p_e2 = p_expr_dot(&mut p_parse.borrow_mut(), p_left.take(), p_right);
                            if in_rename {
                                if let (Some(pl), Some(old)) = (remap_left, a[k].p_expr.as_ref().and_then(|e| e.p_left.as_deref())) {
                                    rename_token_remap(&p_parse, pl, old as *const Expr as usize);
                                }
                            }
                            if let Some(zs) = &z_schema_name {
                                let p_left2 = expr(&db, TK_ID as i32, Some(zs));
                                p_e2 = p_expr_dot(&mut p_parse.borrow_mut(), p_left2, p_e2);
                            }
                            p_expr = p_e2;
                        } else {
                            p_expr = p_right;
                        }
                        let mut p_expr = p_expr;
                        expr_set_error_offset(p_expr.as_deref_mut(), i_err_ofst);
                        p_new = expr_list_append(&mut p_parse.borrow_mut(), p_new, p_expr);
                        let p_new_ref = match p_new.as_mut() {
                            None => break, // OOM
                            Some(n) => n,
                        };
                        let p_x = p_new_ref.a.last_mut().expect("last"); // Termo recém adicionado
                        debug_assert!(p_x.z_e_name.is_none());
                        if (sel_flags & SF_NESTEDFROM) != 0 && !in_rename {
                            if let Some(nf) = p_nested_from {
                                if !VIEW_CAN_HAVE_ROWID || j < nf.n_expr {
                                    debug_assert!(j < nf.n_expr);
                                    p_x.z_e_name = nf.a[j as usize].z_e_name.clone();
                                } else {
                                    p_x.z_e_name = Some(fmt_schema_tab_name(
                                        z_schema_name.as_deref().unwrap_or(b""),
                                        &z_tab_name,
                                        &z_name,
                                    ));
                                }
                            } else {
                                p_x.z_e_name = Some(fmt_schema_tab_name(
                                    z_schema_name.as_deref().unwrap_or(b""),
                                    &z_tab_name,
                                    &z_name,
                                ));
                            }
                            p_x.fg.e_e_name = if j == p_tab.n_col as i32 { ENAME_ROWID } else { ENAME_TAB };
                            let in_from_using = p_from.fg.is_using != 0
                                && matches!(&p_from.u3, SrcItemU3::Using(u) if id_list_index(u, &z_name) >= 0);
                            let in_next_using = p_using.map_or(false, |u| id_list_index(u, &z_name) >= 0);
                            if in_from_using
                                || in_next_using
                                || (j < p_tab.n_col as i32
                                    && (p_tab.a_col[j as usize].col_flags & COLFLAG_NOEXPAND) != 0)
                            {
                                p_x.fg.b_no_expand = 1;
                            }
                        } else if long_names {
                            let mut z = z_tab_name.clone();
                            z.push(b'.');
                            z.extend_from_slice(&z_name);
                            p_x.z_e_name = Some(z);
                            p_x.fg.e_e_name = ENAME_NAME;
                        } else {
                            p_x.z_e_name = Some(z_name.clone());
                            p_x.fg.e_e_name = ENAME_NAME;
                        }
                    }
                }
                if !table_seen {
                    if let Some(zt) = &z_t_name {
                        error_msg(
                            &mut p_parse.borrow_mut(),
                            b"no such table: %s",
                            &[PrintfArg::Text(zt)],
                        );
                    } else {
                        error_msg(&mut p_parse.borrow_mut(), b"no tables specified", &[]);
                    }
                }
            }
        }
        // sqlite3ExprListDelete(db, pEList): o Vec `a` e as Expr restantes caem aqui.
        drop(a);
        p.p_elist = p_new;
    }
    if let Some(el) = p.p_elist.as_ref() {
        if el.n_expr > db.borrow().a_limit[SQLITE_LIMIT_COLUMN as usize] {
            error_msg(&mut p_parse.borrow_mut(), b"too many columns in result set", &[]);
            return WRC_ABORT;
        }
        if (elist_flags & (EP_HAS_FUNC | EP_SUBQUERY)) != 0 {
            p.sel_flags |= SF_COMPLEXRESULT;
        }
    }
    WRC_CONTINUE
}

/// `sqlite3MPrintf(db, "%!S", pFrom)`: nome de exibição de um item da cláusula FROM.
fn m_printf_item_name(db: &Sqlite3Ref, p_from: &SrcItem) -> Vec<u8> {
    m_printf(db, b"%!S", &mut VaList::from_src_item(p_from)).unwrap_or_default()
}

/// `sqlite3MPrintf(db, "%s.%s.%s", zSchemaName, zTabName, zName)`.
fn fmt_schema_tab_name(z_schema: &[u8], z_tab: &[u8], z_name: &[u8]) -> Vec<u8> {
    let mut z = z_schema.to_vec();
    z.push(b'.');
    z.extend_from_slice(z_tab);
    z.push(b'.');
    z.extend_from_slice(z_name);
    z
}

/// `sqlite3PExpr(pParse, TK_DOT, pLeft, pRight)`.
fn p_expr_dot(p_parse: &mut Parse, p_left: Option<Box<Expr>>, p_right: Option<Box<Expr>>) -> Option<Box<Expr>> {
    p_expr(p_parse, TK_DOT as i32, p_left, p_right)
}


// ---- part_013.rs ----

// Convenções desta parte: as rotinas de geração de código recebem `p_parse: &mut Parse` e acham o
// `Vdbe` em `p_parse.p_vdbe` (clonado antes, para poder emprestar `Parse` e `Vdbe` juntos).
// `AggInfo` é compartilhado com `Expr.p_agg_info` (`AggInfoRef`), então as rotinas que o leem
// enquanto geram código recebem `&AggInfoRef` e só emprestam o `RefCell` por trechos curtos.

/// Mensagem de EXPLAIN QUERY PLAN "USE TEMP B-TREE FOR <função>(<sufixo>)".
fn explain_agg_temp_btree(p_parse: &mut Parse, z_func: &[u8], z_suffix: &[u8]) {
    let mut z = b"USE TEMP B-TREE FOR ".to_vec();
    z.extend_from_slice(z_func);
    z.push(b'(');
    z.extend_from_slice(z_suffix);
    z.push(b')');
    vdbe_explain(p_parse, 0, z);
}

/// `(char*)pKeyInfo, P4_KEYINFO`: o ponteiro NULL do C (OOM) vira `P4Value::NotUsed`.
fn p4_key_info(p_key_info: Option<KeyInfoRef>) -> P4Value {
    match p_key_info {
        Some(k) => P4Value::KeyInfo(k),
        None => P4Value::NotUsed,
    }
}

/// Esta rotina "expande" uma instrução SELECT e todas as suas subconsultas.
/// Para informações adicionais sobre o que significa "expandir" uma instrução SELECT,
/// consulte o comentário no callback do trabalhador selectExpander acima.
///
/// Expandir uma instrução SELECT é a primeira etapa no processamento de uma
/// instrução SELECT. A instrução SELECT deve ser expandida antes da resolução
/// de nomes ser realizada.
///
/// Se algo der errado, uma mensagem de erro é escrita em pParse.
/// A função chamadora pode detectar o problema olhando para pParse.n_err
/// e/ou pParse.db.malloc_failed.
fn select_expand(p_parse: &ParseRef, p_select: &mut Select) {
    let mut w = Walker {
        p_parse: Some(p_parse.clone()),
        x_expr_callback: Some(expr_walk_noop),
        x_select_callback: None,
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WalkerU::None,
    };
    if p_parse.borrow().has_compound != 0 {
        w.x_select_callback = Some(convert_compound_select_to_subquery);
        w.x_select_callback2 = None;
        walk_select(&mut w, p_select);
    }
    w.x_select_callback = Some(select_expander);
    w.x_select_callback2 = Some(select_pop_with);
    w.e_code = 0;
    walk_select(&mut w, p_select);
}

/// Este é um callback Walker.xSelectCallback para a interface sqlite3SelectTypeInfo().
///
/// Para cada subconsulta da cláusula FROM, adicione informações Column.zType,
/// Column.zColl e Column.affinity à estrutura Table que representa o conjunto
/// de resultados dessa subconsulta.
///
/// A estrutura Table que representa o conjunto de resultados foi construída
/// pelo selectExpander(), mas as informações de tipo, colação e afinidade foram
/// omitidas naquele ponto porque os identificadores ainda não tinham sido resolvidos.
/// Esta rotina é chamada após a resolução de identificadores.
fn select_add_subquery_type_info(p_walker: &mut Walker, p: &mut Select) {
    if (p.sel_flags & SF_HASTYPEINFO) != 0 {
        return;
    }
    p.sel_flags |= SF_HASTYPEINFO;
    let p_parse = p_walker.p_parse.clone().expect("p_parse");
    debug_assert!((p.sel_flags & SF_RESOLVED) != 0);
    let p_tab_list = p.p_src.as_ref().expect("p_src");
    for i in 0..p_tab_list.n_src as usize {
        let p_from = &p_tab_list.a[i];
        let p_tab = p_from.p_tab.clone().expect("p_tab");
        if (p_tab.borrow().tab_flags & TF_EPHEMERAL) != 0 {
            // Uma subconsulta na cláusula FROM de um SELECT
            if let Some(p_sel) = p_from.p_select.as_deref() {
                subquery_column_types(&mut p_parse.borrow_mut(), &p_tab, p_sel, SQLITE_AFF_NONE);
            }
        }
    }
}

/// Esta rotina adiciona informações de tipo de dado e sequência de colação às
/// estruturas Table de todas as subconsultas da cláusula FROM em uma
/// instrução SELECT.
///
/// Use esta rotina após a resolução de nomes.
fn select_add_type_info(p_parse: &ParseRef, p_select: &mut Select) {
    let mut w = Walker {
        p_parse: Some(p_parse.clone()),
        x_expr_callback: Some(expr_walk_noop),
        x_select_callback: Some(select_walk_noop),
        x_select_callback2: Some(select_add_subquery_type_info),
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WalkerU::None,
    };
    walk_select(&mut w, p_select);
}

/// Esta rotina configura uma instrução SELECT para processamento. O
/// seguinte é realizado:
///
///     *  Os números do cursor VDBE são atribuídos a todos os termos da cláusula FROM.
///     *  Objetos Table efêmeros são criados para todas as subconsultas da cláusula FROM.
///     *  As cláusulas ON e USING são deslocadas para instruções WHERE
///     *  Curingas "*" e "TABLE.*" em conjuntos de resultados são expandidos.
///     *  Identificadores em expressões são casados com as tabelas.
///
/// Esta rotina atua recursivamente em todas as subconsultas dentro do SELECT.
pub fn select_prep(p_parse: &ParseRef, p: &mut Select, p_outer_nc: Option<&mut NameContext>) {
    let db = p_parse.borrow().db.upgrade().expect("db");
    if db.borrow().malloc_failed != 0 {
        return;
    }
    if (p.sel_flags & SF_HASTYPEINFO) != 0 {
        return;
    }
    select_expand(p_parse, p);
    if p_parse.borrow().n_err != 0 {
        return;
    }
    resolve_select_names(p_parse, p, p_outer_nc);
    if p_parse.borrow().n_err != 0 {
        return;
    }
    select_add_type_info(p_parse, p);
}

/// Analisa os argumentos das funções de agregação. Cria novas entradas pAggInfo.a_col[]
/// para colunas que são argumentos de funções de agregação, mas que
/// não são usadas de outra forma.
///
/// As entradas a_col[] em AggInfo antes de n_accumulator são colunas que
/// são referenciadas fora de funções de agregação. Estas podem ser colunas
/// que fazem parte da cláusula GROUP BY, por exemplo. Outros mecanismos de banco
/// de dados lançariam um erro se houvesse uma referência de coluna que não estivesse
/// na cláusula GROUP BY e que não fizesse parte de um argumento de função de agregação.
/// Mas o SQLite permite isto.
///
/// As entradas a_col[] começando com a_col[n_accumulator] e seguintes
/// são referências de coluna que são usadas exclusivamente como argumentos para
/// funções de agregação. Esta rotina é responsável por calcular
/// (ou recalcular) essas entradas a_col[].
fn analyze_agg_func_args(p_agg_info: &AggInfoRef, p_nc: &mut NameContext) {
    debug_assert!(p_agg_info.borrow().i_first_reg == 0);
    p_nc.nc_flags |= NC_INAGGFUNC;
    let n_func = p_agg_info.borrow().n_func;
    for i in 0..n_func as usize {
        let p_expr_rc = p_agg_info.borrow().a_func[i].p_f_expr.clone().expect("p_f_expr");
        let mut p_expr = p_expr_rc.borrow_mut();
        debug_assert!(p_expr.op == TK_FUNCTION || p_expr.op == TK_AGG_FUNCTION);
        debug_assert!(expr_use_x_list(&p_expr));
        expr_analyze_agg_list(p_nc, p_expr.x.p_list.as_deref_mut());
        if let Some(p_left) = p_expr.p_left.as_mut() {
            debug_assert!(p_left.op == TK_ORDER);
            debug_assert!(expr_use_x_list(p_left));
            expr_analyze_agg_list(p_nc, p_left.x.p_list.as_deref_mut());
        }
        debug_assert!(!is_window_func(&p_expr));
        if expr_has_property(&p_expr, EP_WINFUNC) {
            if let Some(p_win) = p_expr.y.p_win.clone() {
                expr_analyze_aggregates(p_nc, p_win.borrow_mut().p_filter.as_deref_mut());
            }
        }
    }
    p_nc.nc_flags &= !NC_INAGGFUNC;
}

/// Um índice em expressões está sendo usado no loop interno de uma
/// consulta de agregação com uma cláusula GROUP BY. Esta rotina tenta
/// ajustar o objeto AggInfo para aproveitar o índice e talvez
/// usar o índice como um índice de cobertura.
fn optimize_aggregate_use_of_indexed_expr(
    _p_parse: &mut Parse,
    p_select: &Select,
    p_agg_info: &AggInfoRef,
    p_nc: &mut NameContext,
) {
    debug_assert!(p_agg_info.borrow().i_first_reg == 0);
    debug_assert!(p_select.p_group_by.is_some());
    {
        let mut ai = p_agg_info.borrow_mut();
        ai.n_column = ai.n_accumulator;
        if ai.n_sorting_column > 0 {
            let mut mx = p_select.p_group_by.as_ref().expect("p_group_by").n_expr - 1;
            for j in 0..ai.n_column as usize {
                let k = ai.a_col[j].i_sorter_column as i32;
                if k > mx {
                    mx = k;
                }
            }
            ai.n_sorting_column = (mx + 1) as u16;
        }
    }
    analyze_agg_func_args(p_agg_info, p_nc);
}

/// Callback Walker para aggregate_convert_indexed_expr_ref_to_column().
fn aggregate_idx_epr_ref_to_col_callback(_p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    let p_agg_info = match p_expr.p_agg_info.clone() {
        None => return WRC_CONTINUE,
        Some(a) => a,
    };
    if p_expr.op == TK_AGG_COLUMN {
        return WRC_CONTINUE;
    }
    if p_expr.op == TK_AGG_FUNCTION {
        return WRC_CONTINUE;
    }
    if p_expr.op == TK_IF_NULL_ROW {
        return WRC_CONTINUE;
    }
    let ai = p_agg_info.borrow();
    if p_expr.i_agg as i32 >= ai.n_column {
        return WRC_CONTINUE;
    }
    debug_assert!(p_expr.i_agg >= 0);
    let p_col = &ai.a_col[p_expr.i_agg as usize];
    p_expr.op = TK_AGG_COLUMN;
    p_expr.i_table = p_col.i_table;
    p_expr.i_column = p_col.i_column;
    expr_clear_property(p_expr, EP_SKIP | EP_COLLATE | EP_UNLIKELY);
    WRC_PRUNE
}

/// Converte cada pAggInfo.a_func[].p_expr de forma que qualquer nó dentro
/// dessas expressões que tenha pAggInfo definido seja alterado para um opcode TK_AGG_COLUMN.
fn aggregate_convert_indexed_expr_ref_to_column(p_agg_info: &AggInfoRef) {
    let mut w = Walker {
        p_parse: None,
        x_expr_callback: Some(aggregate_idx_epr_ref_to_col_callback),
        x_select_callback: None,
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WalkerU::None,
    };
    let n_func = p_agg_info.borrow().n_func;
    for i in 0..n_func as usize {
        let p_expr_rc = p_agg_info.borrow().a_func[i].p_f_expr.clone();
        if let Some(rc) = p_expr_rc {
            walk_expr(&mut w, Some(&mut *rc.borrow_mut()));
        }
    }
}

/// Aloca um bloco de registros de forma que haja um registro para cada
/// entrada pAggInfo.a_col[] e pAggInfo.a_func[] em pAggInfo. O primeiro
/// registro neste bloco é armazenado em pAggInfo.i_first_reg.
///
/// Esta rotina pode ser chamada apenas uma vez para cada objeto AggInfo. Antes
/// de chamar esta rotina:
///
///     *  Os arrays a_col[] e a_func[] podem ser modificados
///     *  As funções agg_info_column_reg() e agg_info_func_reg() não podem ser usadas
///
/// Após chamar esta rotina:
///
///     *  Os arrays a_col[] e a_func[] são fixos
///     *  As funções agg_info_column_reg() e agg_info_func_reg() podem ser usadas
fn assign_aggregate_registers(p_parse: &mut Parse, p_agg_info: &mut AggInfo) {
    debug_assert!(p_agg_info.i_first_reg == 0);
    p_agg_info.i_first_reg = p_parse.n_mem + 1;
    p_parse.n_mem += p_agg_info.n_column + p_agg_info.n_func;
}

/// Redefine o acumulador de agregação.
///
/// O acumulador de agregação é um conjunto de células de memória que mantêm
/// resultados intermediários ao calcular uma agregação. Esta rotina gera código
/// que armazena NULOs em todas essas células de memória.
fn reset_accumulator(p_parse: &mut Parse, p_agg_info: &AggInfoRef) {
    let v = p_parse.p_vdbe.clone().expect("p_vdbe");
    let (n_reg, i_first_reg, n_func) = {
        let ai = p_agg_info.borrow();
        (ai.n_func + ai.n_column, ai.i_first_reg, ai.n_func)
    };
    debug_assert!(i_first_reg > 0);
    debug_assert!(p_parse.db.upgrade().map_or(true, |d| d.borrow().malloc_failed == 0) || p_parse.n_err != 0);
    if n_reg == 0 {
        return;
    }
    if p_parse.n_err != 0 {
        return;
    }
    vdbe_add_op3(&mut v.borrow_mut(), OP_NULL as i32, 0, i_first_reg, i_first_reg + n_reg - 1);
    for i in 0..n_func as usize {
        let (i_distinct, i_ob_tab, p_f_expr_rc, p_func) = {
            let ai = p_agg_info.borrow();
            let f = &ai.a_func[i];
            (f.i_distinct, f.i_ob_tab, f.p_f_expr.clone().expect("p_f_expr"), f.p_func.clone())
        };
        let z_func_name: Vec<u8> = p_func.as_ref().map(|f| f.borrow().z_name.clone()).unwrap_or_default();
        if i_distinct >= 0 {
            let p_e = p_f_expr_rc.borrow();
            debug_assert!(expr_use_x_list(&p_e));
            let list_ok = matches!(p_e.x.p_list.as_ref(), Some(l) if l.n_expr == 1);
            if !list_ok {
                error_msg(
                    p_parse,
                    b"DISTINCT aggregates must have exactly one argument",
                    &[],
                );
                p_agg_info.borrow_mut().a_func[i].i_distinct = -1;
            } else {
                let p_key_info = key_info_from_expr_list(p_parse, p_e.x.p_list.as_deref().expect("p_list"), 0, 0);
                let addr = vdbe_add_op4(
                    &mut v.borrow_mut(),
                    OP_OPENEPHEMERAL as i32,
                    i_distinct,
                    0,
                    0,
                    p4_key_info(p_key_info),
                    P4_KEYINFO,
                );
                p_agg_info.borrow_mut().a_func[i].i_dist_addr = addr;
                explain_agg_temp_btree(p_parse, &z_func_name, b"DISTINCT");
            }
        }
        if i_ob_tab >= 0 {
            let (b_ob_unique, b_ob_payload, b_use_subtype) = {
                let ai = p_agg_info.borrow();
                let f = &ai.a_func[i];
                (f.b_ob_unique, f.b_ob_payload, f.b_use_subtype)
            };
            let p_f_expr = p_f_expr_rc.borrow();
            let p_left = p_f_expr.p_left.as_ref().expect("p_left");
            debug_assert!(p_left.op == TK_ORDER);
            debug_assert!(expr_use_x_list(p_left));
            debug_assert!(p_func.is_some());
            let p_ob_list = p_left.x.p_list.as_deref().expect("p_ob_list");
            let mut n_extra = 0;
            if b_ob_unique == 0 {
                n_extra += 1; // Uma coluna extra para o OP_Sequence
            }
            if b_ob_payload != 0 {
                // colunas extras para os argumentos da função
                debug_assert!(expr_use_x_list(&p_f_expr));
                n_extra += p_f_expr.x.p_list.as_ref().expect("p_list").n_expr;
            }
            if b_use_subtype != 0 {
                n_extra += p_f_expr.x.p_list.as_ref().expect("p_list").n_expr;
            }
            let p_key_info = key_info_from_expr_list(p_parse, p_ob_list, 0, n_extra);
            if b_ob_unique == 0 && p_parse.n_err == 0 {
                if let Some(ki) = p_key_info.as_ref() {
                    ki.borrow_mut().n_key_field += 1;
                }
            }
            vdbe_add_op4(
                &mut v.borrow_mut(),
                OP_OPENEPHEMERAL as i32,
                i_ob_tab,
                p_ob_list.n_expr + n_extra,
                0,
                p4_key_info(p_key_info),
                P4_KEYINFO,
            );
            explain_agg_temp_btree(p_parse, &z_func_name, b"ORDER BY");
        }
    }
}


// ---- part_014.rs ----

/// `P4_FUNCDEF`: a `FuncDef` do C é compartilhada; o `P4Value` guarda um `Rc<FuncDef>`.
fn p4_func_def(p_func: &Option<FuncDefRef>) -> P4Value {
    match p_func {
        Some(f) => P4Value::FuncDef(Rc::new(f.borrow().clone())),
        None => P4Value::NotUsed,
    }
}

/// Invoca o opcode OP_AggFinal para toda função de agregação
/// na estrutura AggInfo.
fn finalize_agg_functions(p_parse: &mut Parse, p_agg_info: &AggInfoRef) {
    let v = p_parse.p_vdbe.clone().expect("p_vdbe");
    let n_func = p_agg_info.borrow().n_func;
    for i in 0..n_func {
        let (i_ob_tab, b_ob_payload, b_ob_unique, b_use_subtype, p_func, p_f_expr_rc) = {
            let ai = p_agg_info.borrow();
            let f = &ai.a_func[i as usize];
            (
                f.i_ob_tab,
                f.b_ob_payload,
                f.b_ob_unique,
                f.b_use_subtype,
                f.p_func.clone(),
                f.p_f_expr.clone().expect("p_f_expr"),
            )
        };
        let p_f_expr = p_f_expr_rc.borrow();
        debug_assert!(expr_use_x_list(&p_f_expr));
        let p_list: Option<&ExprList> = p_f_expr.x.p_list.as_deref();
        let reg_func = agg_info_func_reg(&p_agg_info.borrow(), i);
        if i_ob_tab >= 0 {
            // Para uma agregação ORDER BY, as chamadas a OP_AggStep foram adiadas. As
            // entradas foram guardadas na tabela efêmera pF.i_ob_tab. Aqui extraímos essas
            // entradas (na ordem do ORDER BY) e fazemos todas as chamadas a OP_AggStep
            // antes de fazer a chamada a OP_AggFinal.
            debug_assert!(p_func.is_some());
            let n_arg = p_list.expect("p_list").n_expr; // Número de colunas a extrair
            let reg_agg = get_temp_range(p_parse, n_arg); // Extrai para este array
            let n_key: i32; // Colunas de chave a pular
            if b_ob_payload == 0 {
                n_key = 0;
            } else {
                let p_left = p_f_expr.p_left.as_ref().expect("p_left");
                debug_assert!(expr_use_x_list(p_left));
                debug_assert!(p_left.x.p_list.is_some());
                let mut k = p_left.x.p_list.as_ref().expect("p_list").n_expr;
                if b_ob_unique == 0 {
                    k += 1;
                }
                n_key = k;
            }
            let i_top = vdbe_add_op1(&mut v.borrow_mut(), OP_REWIND as i32, i_ob_tab); // Início do laço de extração
            for j in (0..n_arg).rev() {
                vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, i_ob_tab, n_key + j, reg_agg + j);
            }
            if b_use_subtype != 0 {
                let reg_subtype = get_temp_reg(p_parse);
                let i_base_col = n_key + n_arg + ((b_ob_payload == 0 && b_ob_unique == 0) as i32);
                for j in (0..n_arg).rev() {
                    vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, i_ob_tab, i_base_col + j, reg_subtype);
                    vdbe_add_op2(&mut v.borrow_mut(), OP_SETSUBTYPE as i32, reg_subtype, reg_agg + j);
                }
                release_temp_reg(p_parse, reg_subtype);
            }
            vdbe_add_op3(&mut v.borrow_mut(), OP_AGGSTEP as i32, 0, reg_agg, reg_func);
            vdbe_append_p4(&mut v.borrow_mut(), p4_func_def(&p_func), P4_FUNCDEF as i32);
            vdbe_change_p5(&mut v.borrow_mut(), (n_arg as u8) as u16);
            vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, i_ob_tab, i_top + 1);
            vdbe_jump_here(&mut v.borrow_mut(), i_top);
            release_temp_range(p_parse, reg_agg, n_arg);
        }
        vdbe_add_op2(
            &mut v.borrow_mut(),
            OP_AGGFINAL as i32,
            reg_func,
            p_list.map_or(0, |l| l.n_expr),
        );
        vdbe_append_p4(&mut v.borrow_mut(), p4_func_def(&p_func), P4_FUNCDEF as i32);
    }
}

/// Gera código que atualiza as células de memória do acumulador de uma
/// agregação com base na posição atual do cursor.
///
/// Se reg_acc é diferente de zero e não há agregações min() ou max()
/// em pAggInfo, então só preencha os n_accumulator registros acumuladores de
/// pAggInfo se o registro reg_acc contém 0. O chamador cuida de
/// ligar e desligar reg_acc.
///
/// Para uma agregação ORDER BY, a atualização real da célula de memória do
/// acumulador é adiada até que todas as linhas de entrada tenham sido recebidas,
/// para que possam ser processadas na ordem pedida. Nesse caso, em vez de invocar
/// OP_AggStep para atualizar o acumulador, apenas adicione os argumentos que
/// teriam sido passados a OP_AggStep na tabela efêmera de ordenação
/// (junto com a chave de ordenação apropriada).
fn update_accumulator(
    p_parse: &mut Parse,
    reg_acc: i32,
    p_agg_info: &AggInfoRef,
    e_distinct_type: i32,
) {
    let v = p_parse.p_vdbe.clone().expect("p_vdbe");
    let mut reg_hit: i32 = 0;
    let mut addr_hit_test: i32 = 0;

    debug_assert!(p_agg_info.borrow().i_first_reg > 0);
    if p_parse.n_err != 0 {
        return;
    }
    p_agg_info.borrow_mut().direct_mode = 1;
    let n_func = p_agg_info.borrow().n_func;
    for i in 0..n_func {
        let mut n_arg: i32;
        let mut addr_next: i32 = 0;
        let reg_agg: i32;
        let mut reg_agg_sz: i32 = 0;
        let mut reg_distinct: i32 = 0;
        let (i_ob_tab, i_distinct, b_ob_payload, b_ob_unique, b_use_subtype, p_func, p_f_expr_rc, n_accumulator) = {
            let ai = p_agg_info.borrow();
            let f = &ai.a_func[i as usize];
            (
                f.i_ob_tab,
                f.i_distinct,
                f.b_ob_payload,
                f.b_ob_unique,
                f.b_use_subtype,
                f.p_func.clone(),
                f.p_f_expr.clone().expect("p_f_expr"),
                ai.n_accumulator,
            )
        };
        let p_f_expr = p_f_expr_rc.borrow();
        debug_assert!(expr_use_x_list(&p_f_expr));
        debug_assert!(!is_window_func(&p_f_expr));
        debug_assert!(p_func.is_some());
        let func_flags = p_func.as_ref().expect("p_func").borrow().func_flags;
        let p_list: Option<&ExprList> = p_f_expr.x.p_list.as_deref();
        if expr_has_property(&p_f_expr, EP_WINFUNC) {
            let p_win = p_f_expr.y.p_win.clone().expect("p_win");
            let p_win = p_win.borrow();
            let p_filter = p_win.p_filter.as_deref().expect("p_filter");
            if n_accumulator != 0 && (func_flags & SQLITE_FUNC_NEEDCOLL) != 0 && reg_acc != 0 {
                // Se reg_acc==0, existe alguma função min() ou max()
                // sem cláusula FILTER que garantirá que os registros "magnet"
                // sejam preenchidos.
                if reg_hit == 0 {
                    p_parse.n_mem += 1;
                    reg_hit = p_parse.n_mem;
                }
                // Se esta é a primeira linha do grupo (reg_acc contém 0), limpe o
                // registro "magnet" reg_hit para que os registros acumuladores
                // sejam preenchidos se a cláusula FILTER pular por cima da
                // invocação de min() ou max(). Ou, se esta não é
                // a primeira linha (reg_acc contém 1), ligue o registro magnet para que
                // os acumuladores não sejam preenchidos a menos que min()/max() seja invocado
                // e indique que devem ser.
                vdbe_add_op2(&mut v.borrow_mut(), OP_COPY as i32, reg_acc, reg_hit);
            }
            addr_next = vdbe_make_label(p_parse);
            expr_if_false(p_parse, p_filter, addr_next, SQLITE_JUMPIFNULL);
        }
        if i_ob_tab >= 0 {
            // Em vez de invocar AggStep, devemos empurrar os argumentos que teriam
            // sido passados a AggStep para a tabela de ordenação.
            let p_list = p_list.expect("p_list");
            n_arg = p_list.n_expr;
            debug_assert!(n_arg > 0);
            let p_left = p_f_expr.p_left.as_ref().expect("p_left");
            debug_assert!(p_left.op == TK_ORDER);
            debug_assert!(expr_use_x_list(p_left));
            let p_ob_list = p_left.x.p_list.as_deref().expect("p_ob_list"); // A cláusula ORDER BY
            debug_assert!(p_ob_list.n_expr > 0);
            reg_agg_sz = p_ob_list.n_expr;
            if b_ob_unique == 0 {
                reg_agg_sz += 1; // Um registro para OP_Sequence
            }
            if b_ob_payload != 0 {
                reg_agg_sz += n_arg;
            }
            if b_use_subtype != 0 {
                reg_agg_sz += n_arg;
            }
            reg_agg_sz += 1; // Um registro extra para o resultado de MakeRecord
            reg_agg = get_temp_range(p_parse, reg_agg_sz);
            reg_distinct = reg_agg;
            expr_code_expr_list(p_parse, p_ob_list, reg_agg, 0, SQLITE_ECEL_DUP);
            let mut jj = p_ob_list.n_expr; // Registros usados até agora na montagem do registro
            if b_ob_unique == 0 {
                vdbe_add_op2(&mut v.borrow_mut(), OP_SEQUENCE as i32, i_ob_tab, reg_agg + jj);
                jj += 1;
            }
            if b_ob_payload != 0 {
                reg_distinct = reg_agg + jj;
                expr_code_expr_list(p_parse, p_list, reg_distinct, 0, SQLITE_ECEL_DUP);
                jj += n_arg;
            }
            if b_use_subtype != 0 {
                let reg_base = if b_ob_payload != 0 { reg_distinct } else { reg_agg };
                for kk in 0..n_arg {
                    vdbe_add_op2(&mut v.borrow_mut(), OP_GETSUBTYPE as i32, reg_base + kk, reg_agg + jj);
                    jj += 1;
                }
            }
        } else if let Some(p_list) = p_list {
            n_arg = p_list.n_expr;
            reg_agg = get_temp_range(p_parse, n_arg);
            reg_distinct = reg_agg;
            expr_code_expr_list(p_parse, p_list, reg_agg, 0, SQLITE_ECEL_DUP);
        } else {
            n_arg = 0;
            reg_agg = 0;
        }
        if i_distinct >= 0 && p_list.is_some() {
            if addr_next == 0 {
                addr_next = vdbe_make_label(p_parse);
            }
            let novo = code_distinct(
                p_parse,
                e_distinct_type,
                i_distinct,
                addr_next,
                p_list.expect("p_list"),
                reg_distinct,
            );
            p_agg_info.borrow_mut().a_func[i as usize].i_distinct = novo;
        }
        if i_ob_tab >= 0 {
            // Insere um novo registro na tabela do ORDER BY
            vdbe_add_op3(
                &mut v.borrow_mut(),
                OP_MAKERECORD as i32,
                reg_agg,
                reg_agg_sz - 1,
                reg_agg + reg_agg_sz - 1,
            );
            vdbe_add_op4_int(
                &mut v.borrow_mut(),
                OP_IDXINSERT as i32,
                i_ob_tab,
                reg_agg + reg_agg_sz - 1,
                reg_agg,
                reg_agg_sz - 1,
            );
            release_temp_range(p_parse, reg_agg, reg_agg_sz);
        } else {
            // Invoca a função AggStep
            if (func_flags & SQLITE_FUNC_NEEDCOLL) != 0 {
                let mut p_coll: Option<CollSeqRef> = None;
                debug_assert!(p_list.is_some()); // p_list!=0 se pF.p_func tem NEEDCOLL
                let p_list = p_list.expect("p_list");
                let mut j = 0;
                while p_coll.is_none() && j < n_arg {
                    let p_item_expr = p_list.a[j as usize].p_expr.as_deref().expect("p_expr");
                    p_coll = expr_coll_seq(p_parse, p_item_expr);
                    j += 1;
                }
                if p_coll.is_none() {
                    p_coll = p_parse.db.upgrade().expect("db").borrow().p_dflt_coll.clone();
                }
                if reg_hit == 0 && n_accumulator != 0 {
                    p_parse.n_mem += 1;
                    reg_hit = p_parse.n_mem;
                }
                vdbe_add_op4(
                    &mut v.borrow_mut(),
                    OP_COLLSEQ as i32,
                    reg_hit,
                    0,
                    0,
                    match p_coll {
                        Some(c) => P4Value::CollSeq(c),
                        None => P4Value::NotUsed,
                    },
                    P4_COLLSEQ,
                );
            }
            let reg_func = agg_info_func_reg(&p_agg_info.borrow(), i);
            vdbe_add_op3(&mut v.borrow_mut(), OP_AGGSTEP as i32, 0, reg_agg, reg_func);
            vdbe_append_p4(&mut v.borrow_mut(), p4_func_def(&p_func), P4_FUNCDEF as i32);
            vdbe_change_p5(&mut v.borrow_mut(), (n_arg as u8) as u16);
            release_temp_range(p_parse, reg_agg, n_arg);
        }
        if addr_next != 0 {
            vdbe_resolve_label(&mut v.borrow_mut(), addr_next);
        }
    }
    let n_accumulator = p_agg_info.borrow().n_accumulator;
    if reg_hit == 0 && n_accumulator != 0 {
        reg_hit = reg_acc;
    }
    if reg_hit != 0 {
        addr_hit_test = vdbe_add_op1(&mut v.borrow_mut(), OP_IF as i32, reg_hit);
    }
    for i in 0..n_accumulator {
        let p_c_expr = p_agg_info.borrow().a_col[i as usize].p_c_expr.clone();
        let reg = agg_info_column_reg(&p_agg_info.borrow(), i);
        expr_code(p_parse, p_c_expr.as_ref().map(|e| e.borrow()).as_deref(), reg);
    }

    p_agg_info.borrow_mut().direct_mode = 0;
    if addr_hit_test != 0 {
        vdbe_jump_here_or_pop_inst(&mut v.borrow_mut(), addr_hit_test);
    }
}

/// Adiciona uma única instrução OP_Explain ao VDBE para explicar uma
/// consulta count(*) simples ("SELECT count(*) FROM pTab").
fn explain_simple_count(p_parse: &mut Parse, p_tab: &Table, p_idx: Option<&Index>) {
    if p_parse.explain == 2 {
        let b_cover = p_idx.map_or(false, |i| has_rowid(p_tab) || !is_primary_key_index(i));
        let mut z = b"SCAN ".to_vec();
        z.extend_from_slice(&p_tab.z_name);
        if b_cover {
            z.extend_from_slice(b" USING COVERING INDEX ");
            z.extend_from_slice(&p_idx.expect("p_idx").z_name);
        }
        vdbe_explain(p_parse, 0, z);
    }
}

/// Callback de sqlite3WalkExpr() usado por having_to_where().
///
/// Se o nó passado ao callback é um nó TK_AND, retorna
/// WRC_CONTINUE para dizer a sqlite3WalkExpr() que itere pelos nós filhos.
///
/// Caso contrário, retorna WRC_PRUNE. Neste caso, verifique também se a
/// subexpressão satisfaz os critérios para ser movida para a cláusula
/// WHERE. Se sim, adicione-a à cláusula WHERE e substitua a subexpressão
/// dentro da expressão HAVING por uma constante "1".
fn having_to_where_expr_cb(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    if p_expr.op != TK_AND {
        let p_parse = p_walker.p_parse.clone().expect("p_parse");
        let db = p_parse.borrow().db.upgrade().expect("db");
        let mut moved = false;
        if let WalkerU::Select(p_s) = &mut p_walker.u {
            // Esta rotina é chamada antes de a cláusula HAVING do SELECT atual ser
            // analisada em busca de agregados. Então, se pExpr.p_agg_info está definido
            // aqui, isso indica que a expressão é uma referência correlacionada a uma
            // coluna de uma consulta de agregação externa, ou uma função de agregação que
            // pertence a uma consulta externa. Não mova a expressão para a cláusula
            // WHERE neste caso obscuro, pois isso pode corromper a estrutura AggInfo do
            // Select externo.
            if expr_is_constant_or_group_by(&p_parse, p_expr, p_s.p_group_by.as_deref()) != 0
                && !expr_always_false(p_expr)
                && p_expr.p_agg_info.is_none()
            {
                if let Some(mut p_new) = expr(&db, TK_INTEGER as i32, Some(b"1")) {
                    let p_where = p_s.p_where.take();
                    std::mem::swap(&mut *p_new, p_expr);
                    p_s.p_where = expr_and(&mut p_parse.borrow_mut(), p_where, Some(p_new));
                    moved = true;
                }
            }
        }
        if moved {
            p_walker.e_code = 1;
        }
        return WRC_PRUNE;
    }
    WRC_CONTINUE
}

/// Transfere termos elegíveis da cláusula HAVING de uma consulta, que é
/// processada após o agrupamento, para a cláusula WHERE, que é processada antes do
/// agrupamento. Por exemplo, a consulta:
///
///   SELECT * FROM <tables> WHERE a=? GROUP BY b HAVING b=? AND c=?
///
/// pode ser reescrita como:
///
///   SELECT * FROM <tables> WHERE a=? AND b=? GROUP BY b HAVING c=?
///
/// Um termo da expressão HAVING é elegível para transferência se consiste
/// inteiramente de constantes e expressões que também são termos do GROUP BY que
/// usam a sequência de colação "BINARY".
fn having_to_where(p_parse: &ParseRef, p: &mut Select) {
    let mut s_walker = Walker {
        p_parse: Some(p_parse.clone()),
        x_expr_callback: Some(having_to_where_expr_cb),
        x_select_callback: None,
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WalkerU::None,
    };
    // `sWalker.u.pSelect = p` do C é um ponteiro: aqui o Select inteiro viaja dentro do
    // Walker durante a caminhada (o HAVING sai antes, para ser percorrido sem alias) e volta
    // para `p` ao final.
    let mut p_having = p.p_having.take();
    s_walker.u = WalkerU::Select(Box::new(std::mem::take(p)));
    walk_expr(&mut s_walker, p_having.as_deref_mut());
    if let WalkerU::Select(s) = std::mem::replace(&mut s_walker.u, WalkerU::None) {
        *p = *s;
    }
    p.p_having = p_having;
}

/// Compara dois `p_schema` de `Table` (ponteiros fracos); `None` só casa com `None`.
fn same_schema(a: &Option<Weak<RefCell<Schema>>>, b: &Option<Weak<RefCell<Schema>>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Weak::ptr_eq(a, b),
        _ => false,
    }
}

/// Verifica se a entrada p_this de p_tab_list é um auto-join de outra visão.
/// Procura entradas da cláusula FROM no intervalo i_first..i_end, incluindo i_first
/// mas parando antes de i_end.
///
/// Se p_this é um auto-join, retorna o índice do SrcItem da primeira outra
/// instância dessa visão encontrada. Se p_this não é um auto-join, retorna None.
fn is_self_join_view(
    p_tab_list: &SrcList, // Procura auto-joins nesta cláusula FROM
    p_this: &SrcItem,     // Procura referência anterior a esta subconsulta
    i_first: i32,
    i_end: i32, // Intervalo de entradas da cláusula FROM a procurar
) -> Option<usize> {
    debug_assert!(p_this.p_select.is_some());
    if (p_this.p_select.as_ref().expect("p_select").sel_flags & SF_PUSHDOWN) != 0 {
        return None;
    }
    let mut i_first = i_first;
    while i_first < i_end {
        let idx = i_first as usize;
        i_first += 1;
        let p_item = &p_tab_list.a[idx];
        let p_s1 = match p_item.p_select.as_ref() {
            None => continue,
            Some(s) => s,
        };
        if p_item.fg.via_coroutine != 0 {
            continue;
        }
        if p_item.z_name.is_empty() {
            continue;
        }
        let tab_item = p_item.p_tab.as_ref().expect("p_tab").borrow();
        let tab_this = p_this.p_tab.as_ref().expect("p_tab").borrow();
        if !same_schema(&tab_item.p_schema, &tab_this.p_schema) {
            continue;
        }
        if stricmp(Some(&p_item.z_name), Some(&p_this.z_name)) != 0 {
            continue;
        }
        if tab_item.p_schema.is_none()
            && p_this.p_select.as_ref().expect("p_select").sel_id != p_s1.sel_id
        {
            // O achatador de consultas deixou duas tabelas CTE diferentes com nomes
            // idênticos na mesma cláusula FROM.
            continue;
        }
        if (p_s1.sel_flags & SF_PUSHDOWN) != 0 {
            // A visão foi modificada por alguma outra otimização, como
            // push_down_where_terms()
            continue;
        }
        return Some(idx);
    }
    None
}


// ---- part_015.rs ----

/// `sqlite3PExpr(pParse, op, pLeft, pRight)`; nome próprio para não colidir com as locais `p_expr`.
fn p_expr_new(
    p_parse: &mut Parse,
    op: i32,
    p_left: Option<Box<Expr>>,
    p_right: Option<Box<Expr>>,
) -> Option<Box<Expr>> {
    p_expr(p_parse, op, p_left, p_right)
}

/// Desaloca um único objeto AggInfo.
///
/// No C é o callback de `sqlite3ParserAddCleanup()`; com `AggInfoRef` a memória de `a_col`,
/// `a_func` e do próprio objeto é liberada pelo `Drop` quando a última referência sai.
fn agginfo_free(_db: &Sqlite3Ref, p_arg: AggInfoRef) {
    drop(p_arg);
}

/// Tenta transformar uma consulta da forma
///
///    SELECT count(*) FROM (SELECT x FROM t1 UNION ALL SELECT y FROM t2)
///
/// Nisto:
///
///    SELECT (SELECT count(*) FROM t1)+(SELECT count(*) FROM t2)
///
/// A transformação só funciona se todas as condições abaixo forem verdadeiras:
///
///   *  A subconsulta é um UNION ALL de dois ou mais termos
///   *  A subconsulta não tem cláusula LIMIT
///   *  Não há cláusulas WHERE, GROUP BY ou HAVING nas subconsultas
///   *  A consulta externa é um count(*) simples, sem cláusula WHERE nem outra
///      sintaxe estranha.
///
/// Retorna VERDADEIRO se a otimização é realizada.
fn count_of_view_optimization(p_parse: &mut Parse, p: &mut Select) -> i32 {
    if (p.sel_flags & SF_AGGREGATE) == 0 {
        return 0; // Este é um agregado
    }
    if p.p_elist.as_ref().expect("p_elist").n_expr != 1 {
        return 0; // Uma única coluna de resultado
    }
    if p.p_where.is_some() {
        return 0;
    }
    if p.p_having.is_some() {
        return 0;
    }
    if p.p_group_by.is_some() {
        return 0;
    }
    if p.p_order_by.is_some() {
        return 0;
    }
    {
        let p_expr = p.p_elist.as_ref().expect("p_elist").a[0].p_expr.as_ref().expect("p_expr");
        if p_expr.op != TK_AGG_FUNCTION {
            return 0; // O resultado é um agregado
        }
        let z_token: &[u8] = match &p_expr.u {
            ExprU::Token(z) => z,
            _ => &[],
        };
        if stricmp(Some(z_token), Some(b"count")) != 0 {
            return 0; // É count()
        }
        debug_assert!(expr_use_x_list(p_expr));
        if p_expr.x.p_list.is_some() {
            return 0; // Deve ser count(*)
        }
        if p.p_src.as_ref().expect("p_src").n_src != 1 {
            return 0; // Uma tabela no FROM
        }
        if expr_has_property(p_expr, EP_WINFUNC) {
            return 0; // Não é uma função de janela
        }
    }
    {
        let mut p_sub: &Select = match p.p_src.as_ref().expect("p_src").a[0].p_select.as_deref() {
            None => return 0, // O FROM é uma subconsulta
            Some(s) => s,
        };
        if p_sub.p_prior.is_none() {
            return 0; // Deve ser um composto
        }
        if (p_sub.sel_flags & SF_COPYCTE) != 0 {
            return 0; // Não é uma CTE
        }
        loop {
            if p_sub.op != TK_ALL && p_sub.p_prior.is_some() {
                return 0; // Deve ser UNION ALL
            }
            if p_sub.p_where.is_some() {
                return 0; // Sem cláusula WHERE
            }
            if p_sub.p_limit.is_some() {
                return 0; // Sem cláusula LIMIT
            }
            if (p_sub.sel_flags & SF_AGGREGATE) != 0 {
                return 0; // Não é um agregado
            }
            debug_assert!(p_sub.p_having.is_none()); // Devido à condição anterior
            match p_sub.p_prior.as_deref() {
                Some(prior) => p_sub = prior, // Repete sobre o composto
                None => break,
            }
        }
    }

    // Se chegamos aqui então é OK realizar a transformação

    let db = p_parse.db.upgrade().expect("db");
    let mut p_count: Option<Box<Expr>> = p.p_elist.as_mut().expect("p_elist").a[0].p_expr.take();
    let mut p_expr: Option<Box<Expr>> = None;
    let mut p_sub: Option<Box<Select>> = p.p_src.as_mut().expect("p_src").a[0].p_select.take();
    // sqlite3SrcListDelete(db, p->pSrc); p->pSrc = sqlite3DbMallocZero(...)
    p.p_src = Some(Box::new(SrcList { n_src: 0, n_alloc: 0, a: Vec::new() }));
    while let Some(mut sub) = p_sub.take() {
        let p_prior = sub.p_prior.take();
        sub.p_next = None;
        sub.sel_flags |= SF_AGGREGATE;
        sub.sel_flags &= !SF_COMPOUND;
        sub.n_select_row = 0;
        // sqlite3ParserAddCleanup(pParse, sqlite3ExprListDeleteGeneric, pSub->pEList): com
        // propriedade única a lista antiga cai aqui, sem precisar adiar a liberação.
        drop(sub.p_elist.take());
        let p_term: Option<Box<Expr>> = if p_prior.is_some() {
            expr_dup(&db, p_count.as_deref(), 0)
        } else {
            p_count.take()
        };
        sub.p_elist = expr_list_append(p_parse, None, p_term);
        let mut p_term = p_expr_new(p_parse, TK_SELECT as i32, None, None);
        p_expr_add_select(p_parse, p_term.as_deref_mut(), sub);
        p_expr = match p_expr {
            None => p_term,
            Some(prev) => p_expr_new(p_parse, TK_PLUS as i32, p_term, Some(prev)),
        };
        p_sub = p_prior;
    }
    p.p_elist.as_mut().expect("p_elist").a[0].p_expr = p_expr;
    p.sel_flags &= !SF_AGGREGATE;
    1
}

/// Se algum termo de pSrc, ou qualquer subconsulta SF_NestedFrom, não é o mesmo
/// que pSrcItem mas tem o mesmo alias que p0, retorna verdadeiro.
/// Caso contrário retorna falso.
fn same_src_alias(p0: &SrcItem, p_src: &SrcList) -> i32 {
    for i in 0..p_src.n_src as usize {
        let p1 = &p_src.a[i];
        if std::ptr::eq(p1, p0) {
            continue;
        }
        let same_tab = match (&p0.p_tab, &p1.p_tab) {
            (None, None) => true,
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        };
        let alias_of = |s: &SrcItem| -> Option<&[u8]> {
            if s.z_alias.is_empty() { None } else { Some(&s.z_alias[..]) }
        };
        if same_tab && 0 == stricmp(alias_of(p0), alias_of(p1)) {
            return 1;
        }
        if let Some(p1_sel) = p1.p_select.as_ref() {
            if (p1_sel.sel_flags & SF_NESTEDFROM) != 0 {
                if let Some(sub_src) = p1_sel.p_src.as_ref() {
                    if same_src_alias(p0, sub_src) != 0 {
                        return 1;
                    }
                }
            }
        }
    }
    0
}

/// Retorna VERDADEIRO (não zero) se a i-ésima entrada da SrcList p_tab_list pode
/// ser implementada como co-rotina. A i-ésima entrada tem garantia de ser
/// uma subconsulta.
///
/// A subconsulta é implementada como co-rotina se todas as condições a seguir
/// são verdadeiras:
///
///    (1)  A subconsulta provavelmente será implementada no laço externo da
///         consulta. Este será o caso se qualquer uma das condições a seguir valer:
///         (a)  A subconsulta é o único termo da cláusula FROM
///         (b)  A subconsulta é o termo mais à esquerda e um CROSS JOIN ou similar
///              exige que ela seja o laço externo
///         (c)  Todas as seguintes são verdadeiras:
///                (i) A subconsulta é a subconsulta mais à esquerda da cláusula FROM
///               (ii) Nada impede que a subconsulta seja usada como laço
///                    externo se a rotina where_begin() a nomear para essa posição.
///              (iii) A consulta não é um UPDATE ... FROM
///    (2)  A subconsulta não é uma CTE que deva ser materializada porque
///         (a) a palavra-chave AS MATERIALIZED é usada, ou
///         (b) a CTE é usada várias vezes e não tem a palavra-chave
///             NOT MATERIALIZED
///    (3)  A subconsulta não faz parte do operando esquerdo de um RIGHT JOIN
///    (4)  O sinalizador de desabilitação de otimização SQLITE_Coroutines não está ligado
///    (5)  A subconsulta não é auto-unida
fn from_clause_term_can_be_coroutine(
    p_parse: &Parse,      // Contexto de análise
    p_tab_list: &SrcList, // Cláusula FROM
    i: i32,               // Qual termo da cláusula FROM tem a subconsulta
    sel_flags: u32,       // Sinalizadores do SELECT
) -> i32 {
    let mut i = i as usize;
    let p_item = &p_tab_list.a[i];
    if p_item.fg.is_cte != 0 {
        if let SrcItemU2::CteUse(p_cte_use) = &p_item.u2 {
            let u = p_cte_use.borrow();
            if u.e_m10d == M10D_YES {
                return 0; // (2a)
            }
            if u.n_use >= 2 && u.e_m10d != M10D_NO {
                return 0; // (2b)
            }
        }
    }
    if (p_tab_list.a[0].fg.jointype & JT_LTORJ) != 0 {
        return 0; // (3)
    }
    let db = p_parse.db.upgrade().expect("db");
    if optimization_disabled(&db.borrow(), SQLITE_COROUTINES) {
        return 0; // (4)
    }
    if is_self_join_view(p_tab_list, p_item, i as i32 + 1, p_tab_list.n_src).is_some() {
        return 0; // (5)
    }
    if i == 0 {
        if p_tab_list.n_src == 1 {
            return 1; // (1a)
        }
        if (p_tab_list.a[1].fg.jointype & JT_CROSS) != 0 {
            return 1; // (1b)
        }
        if (sel_flags & SF_UPDATEFROM) != 0 {
            return 0; // (1c-iii)
        }
        return 1;
    }
    if (sel_flags & SF_UPDATEFROM) != 0 {
        return 0; // (1c-iii)
    }
    let mut p_item = p_item;
    loop {
        if (p_item.fg.jointype & (JT_OUTER | JT_CROSS)) != 0 {
            return 0; // (1c-ii)
        }
        if i == 0 {
            break;
        }
        i -= 1;
        p_item = &p_tab_list.a[i];
        if p_item.p_select.is_some() {
            return 0; // (1c-i)
        }
    }
    1
}

/// Variáveis locais de `sqlite3Select()` que atravessam as chunks `select_c.015` a `select_c.019`.
///
/// O corpo do `sqlite3Select()` do C se estende por cinco trechos e usa `goto select_end` para
/// sair de qualquer ponto. Cada trecho vira uma função `select_stage_N` que recebe este contexto
/// e devolve um `SelectFlow`; o `select()` final (no trecho que contém o rótulo `select_end`) as
/// encadeia. As locais `pEList`, `pTabList`, `pWhere`, `pGroupBy` e `pHaving` são apelidos de
/// campos de `Select` (`p.p_elist`, `p.p_src`, ...) e são lidas direto de `p` em cada estágio.
#[derive(Default)]
pub struct SelectCtx {
    /// Retorno do WhereBegin().
    pub p_w_info: Option<Box<WhereInfo>>,
    /// A máquina virtual em construção.
    pub v: Option<VdbeRef>,
    /// Verdadeiro para listas de seleção como "count(*)".
    pub is_agg: bool,
    /// Informação de agregação.
    pub p_agg_info: Option<AggInfoRef>,
    /// Valor retornado pela função (inicia em 1).
    pub rc: i32,
    /// Como codificar a palavra-chave DISTINCT.
    pub s_distinct: DistinctCtx,
    /// Como codificar a cláusula ORDER BY.
    pub s_sort: SortCtx,
    /// Endereço do fim da consulta.
    pub i_end: i32,
    /// ORDER BY adicionado para consultas min/max.
    pub p_min_max_order_by: Option<Box<ExprList>>,
    /// Sinalizador de consultas min/max.
    pub min_max_flag: u8,
}

/// Como um trecho de `select()` termina: segue para o próximo estágio, retorna já (os
/// `return 1` do C antes de qualquer limpeza) ou vai para o rótulo `select_end`.
pub enum SelectFlow {
    /// Continua no próximo estágio.
    Continue,
    /// `return n;` direto.
    Return(i32),
    /// `goto select_end;`
    GotoSelectEnd,
}

/// Primeiro estágio de `sqlite3Select()`: do início até o ponto em que o C entra no laço
/// `for(i=0; !p->pPrior && i<pTabList->nSrc; i++)` das otimizações do FROM (que começa nesta chunk
/// e continua em `select_c.016`).
///
/// Gera código para a instrução SELECT dada no argumento p.
///
/// Os resultados são retornados conforme a estrutura SelectDest.
/// Veja os comentários em sqliteInt.h para mais informações.
///
/// Esta rotina retorna o número de erros. Se algum erro é
/// encontrado, uma mensagem de erro apropriada é deixada em
/// pParse.z_err_msg.
///
/// Esta rotina NÃO libera a estrutura Select passada. A
/// função chamadora precisa fazer isso.
pub fn select_stage_1(
    p_parse: &ParseRef,
    p: &mut Select,
    p_dest: &mut SelectDest,
    ctx: &mut SelectCtx,
) -> SelectFlow {
    ctx.rc = 1;
    let db = p_parse.borrow().db.upgrade().expect("db");
    ctx.v = get_vdbe(&mut p_parse.borrow_mut());
    if p_parse.borrow().n_err != 0 {
        return SelectFlow::Return(1);
    }
    debug_assert!(db.borrow().malloc_failed == 0);
    if auth_check(&mut p_parse.borrow_mut(), SQLITE_SELECT, None, None, None) != 0 {
        return SelectFlow::Return(1);
    }

    debug_assert!(p.p_order_by.is_none() || p_dest.e_dest != SRT_DISTFIFO);
    debug_assert!(p.p_order_by.is_none() || p_dest.e_dest != SRT_FIFO);
    debug_assert!(p.p_order_by.is_none() || p_dest.e_dest != SRT_DISTQUEUE);
    debug_assert!(p.p_order_by.is_none() || p_dest.e_dest != SRT_QUEUE);
    if ignorable_distinct(p_dest) {
        debug_assert!(
            p_dest.e_dest == SRT_EXISTS
                || p_dest.e_dest == SRT_UNION
                || p_dest.e_dest == SRT_EXCEPT
                || p_dest.e_dest == SRT_DISCARD
                || p_dest.e_dest == SRT_DISTQUEUE
                || p_dest.e_dest == SRT_DISTFIFO
        );
        // Todos esses destinos também conseguem ignorar a cláusula ORDER BY
        if p.p_order_by.is_some() {
            // sqlite3ParserAddCleanup(pParse, sqlite3ExprListDeleteGeneric, p->pOrderBy):
            // com propriedade única a lista cai aqui.
            p.p_order_by = None;
        }
        p.sel_flags &= !SF_DISTINCT;
        p.sel_flags |= SF_NOOPORDERBY;
    }
    select_prep(p_parse, p, None);
    if p_parse.borrow().n_err != 0 {
        return SelectFlow::GotoSelectEnd;
    }
    debug_assert!(db.borrow().malloc_failed == 0);
    debug_assert!(p.p_elist.is_some());

    // Se o sinalizador SF_UFSrcCheck está ligado, esta função está sendo chamada
    // como parte do preenchimento da tabela temporária de um UPDATE...FROM.
    // Neste caso, é um erro se o nome ou alias do objeto alvo (pSrc.a[0]) estiver
    // duplicado dentro da cláusula FROM (pSrc.a[1..n]).
    //
    // O Postgres também proíbe este caso. A razão é que alguns outros
    // sistemas tratam este caso de modo diferente, e nem todos do mesmo jeito,
    // o que só confunde. Para evitar isso, seguimos o Postgres e
    // o proibimos por completo.
    if (p.sel_flags & SF_UFSRCCHECK) != 0 {
        let p_src = p.p_src.as_ref().expect("p_src");
        let p0 = &p_src.a[0];
        if same_src_alias(p0, p_src) != 0 {
            let z_name: Vec<u8> = if !p0.z_alias.is_empty() {
                p0.z_alias.clone()
            } else {
                p0.p_tab.as_ref().expect("p_tab").borrow().z_name.clone()
            };
            error_msg(
                &mut p_parse.borrow_mut(),
                b"target object/alias may not appear in FROM clause: %s",
                &[PrintfArg::Text(&z_name)],
            );
            return SelectFlow::GotoSelectEnd;
        }

        // Limpa o sinalizador SF_UFSrcCheck. A verificação já foi feita,
        // e deixar o sinalizador ligado pode causar erros se uma subconsulta composta
        // em p.p_src for achatada nesta consulta e esta função for chamada
        // de novo como parte do processamento do SELECT composto.
        p.sel_flags &= !SF_UFSRCCHECK;
    }

    if p_dest.e_dest == SRT_OUTPUT {
        generate_column_names(&mut p_parse.borrow_mut(), p);
    }

    if window_rewrite(&mut p_parse.borrow_mut(), p) != 0 {
        debug_assert!(p_parse.borrow().n_err != 0);
        return SelectFlow::GotoSelectEnd;
    }
    ctx.is_agg = (p.sel_flags & SF_AGGREGATE) != 0;
    ctx.s_sort = SortCtx::default();
    // `sSort.pOrderBy = p->pOrderBy` é um apelido no C; aqui os dois lados têm cópia própria.
    ctx.s_sort.p_order_by = p.p_order_by.clone();

    // O laço `for(i=0; !p->pPrior && i<pTabList->nSrc; i++)` das otimizações do FROM
    // (simplificação de joins e achatamento de subconsultas) começa aqui e segue em select_c.016.
    SelectFlow::Continue
}

