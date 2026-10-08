// Mesclado das partes traduzidas de fkey_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Chaves estrangeiras adiadas e imediatas: o texto longo do cabeçalho do fkey.c (contador
// de violações por conexão, contador por instrução, passos I.1, I.2, D.1, D.2, convenção de
// chamada do VDBE) descreve o desenho e não tem código; fica resumido aqui. O registrador
// (x) guarda o rowid e (x+1), (x+2)... guardam as colunas na ordem da tabela.

/// Uma restrição de chave estrangeira exige que as colunas de chave na tabela pai sejam
/// coletivamente sujeitas a uma restrição UNIQUE ou PRIMARY KEY. Dado que `p_parent` é a
/// tabela pai da restrição `p_fkey`, procura no schema um índice único nas colunas de chave
/// do pai.
///
/// Se bem-sucedido, retorna zero. Se a chave do pai é uma coluna INTEGER PRIMARY KEY, então
/// `pp_idx` fica `None`. Caso contrário, `pp_idx` recebe o índice único.
///
/// Se a chave do pai tem uma só coluna (chave estrangeira não composta), `pai_col` fica
/// `None`. Caso contrário, recebe um vetor de tamanho N (N colunas da chave do pai): o
/// primeiro elemento é o índice da coluna da tabela filha mapeada pela restrição para a
/// coluna mais à esquerda de `pp_idx`, o segundo para a segunda coluna, e assim por diante.
/// `pai_col` é `None` quando o chamador não quer o mapa (o `NULL` do C).
///
/// Se o índice necessário não for encontrado (colunas inexistentes, sem UNIQUE ou PRIMARY
/// KEY, pai sem PRIMARY KEY, ou PRIMARY KEY com número de colunas diferente), retorna não
/// zero e carrega o erro "foreign key mismatch" em `p_parse`.
pub fn fk_locate_index(
    p_parse: &mut Parse,
    p_parent: &TableRef,
    p_fkey: &FKeyRef,
    pp_idx: &mut Option<IndexRef>,
    pai_col: Option<&mut Option<Vec<i32>>>,
) -> i32 {
    let parent = p_parent.borrow();
    let fkey = p_fkey.borrow();
    let mut p_idx: Option<IndexRef> = None; // valor retornado em *ppIdx
    let mut ai_col: Option<Vec<i32>> = None; // valor retornado em *paiCol
    let n_col = fkey.n_col as usize; // número de colunas da chave do pai
    let z_key: Option<Vec<u8>> = fkey.a_col[0].z_col.clone(); // coluna de chave mais à esquerda

    // O chamador é responsável por zerar os parâmetros de saída.
    debug_assert!(pp_idx.is_none());
    let want_map = match &pai_col {
        Some(out) => {
            debug_assert!(out.is_none());
            true
        }
        None => false,
    };

    // Se esta é uma chave estrangeira não composta (coluna única), verifica se ela mapeia
    // para a INTEGER PRIMARY KEY de `p_parent`. Se sim, deixa *ppIdx e *paiCol zerados e
    // retorna cedo.
    //
    // Caso contrário, para uma chave composta, aloca o vetor aiCol (devolvido em *paiCol).
    // Chaves não compostas não precisam do vetor.
    if n_col == 1 {
        // A FK mapeia para o IPK se uma das condições for verdadeira:
        //
        //   1) Há uma coluna INTEGER PRIMARY KEY e a FK está implicitamente mapeada para a
        //      chave primária de `p_parent`, ou
        //   2) A FK está explicitamente mapeada para uma coluna declarada INTEGER PRIMARY KEY.
        if parent.i_p_key >= 0 {
            match &z_key {
                None => return 0,
                Some(z) => {
                    if str_i_cmp(&parent.a_col[parent.i_p_key as usize].z_cn_name, z) == 0 {
                        return 0;
                    }
                }
            }
        }
    } else if want_map {
        debug_assert!(n_col > 1);
        // A alocação do C falha com OOM e retorna 1; um Vec não falha, o ramo não existe.
        ai_col = Some(vec![0i32; n_col]);
    }

    let mut cur = parent.p_index.clone();
    while let Some(idx_ref) = cur {
        let idx = idx_ref.borrow();
        if idx.n_key_col as usize == n_col
            && is_unique_index(&idx)
            && idx.p_part_idx_where.is_none()
        {
            // idx é um índice UNIQUE (ou PRIMARY KEY) e tem o número certo de colunas. Se
            // cada coluna indexada corresponde a uma coluna de chave estrangeira de pFKey,
            // então este índice é o vencedor.
            match &z_key {
                None => {
                    // Se zKey é NULL, esta chave estrangeira está implicitamente mapeada para
                    // a PRIMARY KEY de `p_parent`. O índice da PRIMARY KEY se identifica pelo
                    // teste.
                    if is_primary_key_index(&idx) {
                        if let Some(ac) = ai_col.as_mut() {
                            for i in 0..n_col {
                                ac[i] = fkey.a_col[i].i_from;
                            }
                        }
                        p_idx = Some(idx_ref.clone());
                        break;
                    }
                }
                Some(z_key_name) => {
                    // Se zKey não é NULL, esta chave estrangeira foi declarada para mapear
                    // para uma lista explícita de colunas de `p_parent`. Verifica se este
                    // índice casa com essas colunas. Verifica também se o índice usa as
                    // sequências de collation padrão de cada coluna.
                    let _ = z_key_name;
                    let mut i = 0usize;
                    while i < n_col {
                        let i_col = idx.ai_column[i]; // índice da coluna na tabela pai
                        if i_col < 0 {
                            break; // sem chaves estrangeiras contra índices de expressão
                        }

                        // Se o índice usa uma collation diferente da padrão da coluna, ele
                        // é inutilizável. Sai cedo neste caso.
                        let col = &parent.a_col[i_col as usize];
                        let z_dflt_coll: Vec<u8> = match column_coll(col) {
                            Some(z) => z,
                            None => STR_BINARY.to_vec(),
                        };
                        if str_i_cmp(&idx.az_coll[i], &z_dflt_coll) != 0 {
                            break;
                        }

                        let z_idx_col = &col.z_cn_name;
                        let mut j = 0usize;
                        while j < n_col {
                            let z_col_j: &[u8] = fkey.a_col[j].z_col.as_deref().unwrap_or(&[]);
                            if str_i_cmp(z_col_j, z_idx_col) == 0 {
                                if let Some(ac) = ai_col.as_mut() {
                                    ac[i] = fkey.a_col[j].i_from;
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
                        // idx é utilizável
                        p_idx = Some(idx_ref.clone());
                        break;
                    }
                }
            }
        }
        cur = idx.p_next.clone();
    }

    if p_idx.is_none() {
        if p_parse.disable_triggers == 0 {
            let z_from: Vec<u8> = match fkey.p_from.upgrade() {
                Some(t) => t.borrow().z_name.clone(),
                None => Vec::new(),
            };
            error_msg(
                p_parse,
                b"foreign key mismatch - \"%w\" referencing \"%w\"",
                &[FmtArg::Text(&z_from), FmtArg::Text(&fkey.z_to)],
            );
        }
        // aiCol é liberado aqui pelo Drop.
        return 1;
    }

    if let Some(out) = pai_col {
        *out = ai_col;
    }
    *pp_idx = p_idx;
    0
}

/// Esta função é chamada quando uma linha é inserida ou apagada da tabela filha da restrição
/// `p_fkey`. Se um UPDATE SQL roda na tabela filha, ela é invocada duas vezes por linha
/// afetada: uma para "apagar" a linha antiga e outra para "inserir" a nova.
///
/// A cada chamada, gera código VDBE que localiza, na tabela pai, a linha correspondente à
/// linha inserida ou apagada da filha. Se a linha do pai é encontrada, nenhuma ação especial.
/// Caso contrário:
///
///   Operação | Tipo de FK | Ação tomada
///   INSERT     imediata     Incrementa o "contador de restrição imediata".
///   DELETE     imediata     Decrementa o "contador de restrição imediata".
///   INSERT     adiada       Incrementa o "contador de restrição adiada".
///   DELETE     adiada       Decrementa o "contador de restrição adiada".
///
/// Essas operações são identificadas no comentário do topo do fkey.c como "I.1" e "D.1".
pub fn fk_lookup_parent(
    p_parse: &mut Parse,
    i_db: i32,
    p_tab: &TableRef,
    p_idx: Option<&IndexRef>,
    p_fkey: &FKeyRef,
    ai_col: &[i32],
    reg_data: i32,
    n_incr: i32,
    is_ignore: i32,
) {
    let v = get_vdbe(p_parse).expect("VDBE já alocado");
    let i_cur = p_parse.n_tab - 1; // número do cursor a usar
    let i_ok = vdbe_make_label(p_parse); // salta para cá se a chave do pai foi achada
    let (n_col, is_deferred, p_from) = {
        let fk = p_fkey.borrow();
        (fk.n_col, fk.is_deferred as i32, fk.p_from.upgrade().expect("tabela filha viva"))
    };

    // O sqlite3VdbeVerifyAbortable só existe sob SQLITE_DEBUG e some aqui.

    // Se nIncr é menor que zero, verifica em tempo de execução se há restrições pendentes a
    // resolver. Se não há, não é preciso checar se apagar esta linha resolve violações.
    //
    // Verifica se alguma das colunas de chave da linha da tabela filha é NULL. Se alguma é,
    // a restrição é considerada satisfeita e não é preciso procurar a linha no pai.
    if n_incr < 0 {
        vdbe_add_op2(&v, OP_FKIFZERO, is_deferred, i_ok);
    }
    for i in 0..n_col as usize {
        let i_reg = table_column_to_storage(&p_from.borrow(), ai_col[i] as i16) as i32 + reg_data + 1;
        vdbe_add_op2(&v, OP_ISNULL, i_reg, i_ok);
    }

    if is_ignore == 0 {
        match p_idx {
            None => {
                // Se pIdx é NULL, a chave do pai é a coluna INTEGER PRIMARY KEY da tabela
                // pai (tabela p_tab).
                let reg_temp = get_temp_reg(p_parse);

                // Invoca MustBeInt para forçar o valor da chave filha a inteiro (aplica a
                // afinidade da chave do pai). Se falhar, não há chave pai correspondente.
                // Antes de usar MustBeInt, faz uma cópia do valor. Caso contrário, o valor
                // inserido na coluna da chave filha receberia afinidade INTEGER, o que pode
                // não ser correto.
                let src_reg =
                    table_column_to_storage(&p_from.borrow(), ai_col[0] as i16) as i32 + 1 + reg_data;
                vdbe_add_op2(&v, OP_SCOPY, src_reg, reg_temp);
                let i_must_be_int = vdbe_add_op2(&v, OP_MUSTBEINT, reg_temp, 0);

                // Se a tabela pai é a mesma da filha e vamos incrementar o contador (ou seja,
                // é um INSERT), verifica se a linha inserida casa consigo mesma. Se sim, não
                // incrementa o contador.
                if Rc::ptr_eq(p_tab, &p_from) && n_incr == 1 {
                    vdbe_add_op3(&v, OP_EQ, reg_data, i_ok, reg_temp);
                    vdbe_change_p5(&v, SQLITE_NOTNULL as u16);
                }

                open_table(p_parse, i_cur, i_db, p_tab, OP_OPENREAD);
                vdbe_add_op3(&v, OP_NOTEXISTS, i_cur, 0, reg_temp);
                vdbe_goto(&v, i_ok);
                vdbe_jump_here(&v, vdbe_current_addr(&v) - 2);
                vdbe_jump_here(&v, i_must_be_int);
                release_temp_reg(p_parse, reg_temp);
            }
            Some(idx_ref) => {
                let reg_temp = get_temp_range(p_parse, n_col);

                vdbe_add_op3(&v, OP_OPENREAD, i_cur, idx_ref.borrow().tnum as i32, i_db);
                vdbe_set_p4_key_info(p_parse, idx_ref);
                for i in 0..n_col as usize {
                    let src_reg =
                        table_column_to_storage(&p_from.borrow(), ai_col[i] as i16) as i32 + 1 + reg_data;
                    vdbe_add_op2(&v, OP_COPY, src_reg, reg_temp + i as i32);
                }

                // Se a tabela pai é a mesma da filha e vamos incrementar o contador (ou seja,
                // é um INSERT), verifica se a linha inserida casa consigo mesma. Se sim, não
                // incrementa o contador.
                //
                // Se algum dos valores de chave do pai é NULL, a linha não pode casar consigo
                // mesma. Então liga JUMPIFNULL para garantir o OP_Found se algum valor de
                // chave do pai é NULL (a esta altura já se sabe que nenhum valor da chave
                // filha é).
                if Rc::ptr_eq(p_tab, &p_from) && n_incr == 1 {
                    let i_jump = vdbe_current_addr(&v) + n_col + 1;
                    let (idx_table, ai_column) = {
                        let idx = idx_ref.borrow();
                        (idx.p_table.upgrade().expect("tabela do índice viva"), idx.ai_column.clone())
                    };
                    let i_p_key = p_tab.borrow().i_p_key;
                    for i in 0..n_col as usize {
                        let i_child =
                            table_column_to_storage(&p_from.borrow(), ai_col[i] as i16) as i32 + 1 + reg_data;
                        let mut i_parent = 1 + reg_data;
                        i_parent += table_column_to_storage(&idx_table.borrow(), ai_column[i]) as i32;
                        debug_assert!(ai_column[i] >= 0);
                        debug_assert!(ai_col[i] != i_p_key as i32);
                        if ai_column[i] == i_p_key {
                            // A chave do pai é uma chave composta que inclui a coluna IPK
                            i_parent = reg_data;
                        }
                        vdbe_add_op3(&v, OP_NE, i_child, i_jump, i_parent);
                        vdbe_change_p5(&v, SQLITE_JUMPIFNULL as u16);
                    }
                    vdbe_goto(&v, i_ok);
                }

                let z_aff = {
                    let db = p_parse.db.clone();
                    index_affinity_str(&db, idx_ref)
                };
                vdbe_add_op4(&v, OP_AFFINITY, reg_temp, n_col, 0, P4Arg::Text(z_aff), n_col);
                vdbe_add_op4_int(&v, OP_FOUND, i_cur, i_ok, reg_temp, n_col);
                release_temp_range(p_parse, reg_temp, n_col);
            }
        }
    }

    let flags = p_parse.db.borrow().flags;
    if is_deferred == 0
        && (flags & SQLITE_DEFERFKS) == 0
        && p_parse.p_toplevel.is_none()
        && p_parse.is_multi_write == 0
    {
        // Caso especial: se este é um INSERT que insere exatamente uma linha na tabela,
        // levanta a restrição imediatamente em vez de incrementar um contador. Isso é
        // necessário porque o código VM gerado não abre uma transação de instrução.
        debug_assert!(n_incr == 1);
        halt_constraint(
            p_parse,
            SQLITE_CONSTRAINT_FOREIGNKEY,
            OE_ABORT,
            None,
            P4_STATIC,
            P5_CONSTRAINTFK,
        );
    } else {
        if n_incr > 0 && is_deferred == 0 {
            may_abort(p_parse);
        }
        vdbe_add_op2(&v, OP_FKCOUNTER, is_deferred, n_incr);
    }

    vdbe_resolve_label(&v, i_ok);
    vdbe_add_op1(&v, OP_CLOSE, i_cur);
}


// ---- part_001.rs ----

/// Retorna a primeira FKey da lista `u.tab.p_fkey` de uma tabela comum (a lista ligada por
/// `p_next_from`). Acessor da união `u.tab`, usado em várias funções deste arquivo.
pub fn table_p_fkey(p_tab: &Table) -> Option<FKeyRef> {
    match &p_tab.u {
        TableU::Tab(tab) => tab.p_fkey.clone(),
        _ => None,
    }
}

/// Retorna um objeto Expr que se refere a um registro de memória correspondente à coluna
/// `i_col` da tabela `p_tab`.
///
/// `reg_base` é o primeiro de um arranjo de registros que contém os dados de `p_tab`. O
/// próprio `reg_base` guarda o rowid. `reg_base+1` guarda a primeira coluna, `reg_base+2` a
/// segunda, e assim por diante.
fn expr_table_register(
    p_parse: &mut Parse,
    p_tab: &TableRef,
    reg_base: i32,
    i_col: i16,
) -> Option<Box<Expr>> {
    let db = p_parse.db.clone();

    let mut p_ret = expr(&db, TK_REGISTER, None);
    if let Some(e) = p_ret.as_mut() {
        let tab = p_tab.borrow();
        if i_col >= 0 && i_col != tab.i_p_key {
            let p_col = &tab.a_col[i_col as usize];
            e.i_table = reg_base + table_column_to_storage(&tab, i_col) as i32 + 1;
            e.aff_expr = p_col.affinity;
            let z_coll: Vec<u8> = match column_coll(p_col) {
                Some(z) => z,
                None => db.borrow().p_dflt_coll.z_name.clone(),
            };
            drop(tab);
            p_ret = expr_add_collate_string(p_parse, p_ret, &z_coll);
        } else {
            e.i_table = reg_base;
            e.aff_expr = SQLITE_AFF_INTEGER;
        }
    }
    p_ret
}

/// Retorna um objeto Expr que se refere à coluna `i_col` da tabela `p_tab`, que tem o cursor
/// `i_cursor`.
fn expr_table_column(
    db: &Sqlite3Ref,
    p_tab: &TableRef,
    i_cursor: i32,
    i_col: i16,
) -> Option<Box<Expr>> {
    let mut p_ret = expr(db, TK_COLUMN, None);
    if let Some(e) = p_ret.as_mut() {
        e.y.p_tab = Some(p_tab.clone());
        e.i_table = i_cursor;
        e.i_column = i_col;
    }
    p_ret
}

/// Esta função é chamada para gerar código executado quando uma linha é apagada da tabela
/// pai da restrição `p_fkey` e, se `p_fkey` é adiada, quando uma linha é inserida na mesma
/// tabela. Ao gerar código para um UPDATE SQL, pode ser chamada duas vezes: uma para
/// "apagar" a linha antiga e outra para "inserir" a nova.
///
/// O parâmetro `n_incr` é -1 ao inserir uma linha (pois isso pode diminuir o número de
/// violações de FK no banco) ou +1 ao apagar uma (pois isso pode aumentar o número de
/// problemas de restrição).
///
/// O código gerado varre as linhas da tabela filha que correspondem à linha do pai sendo
/// apagada ou inserida. Para cada linha filha encontrada:
///
///   Operação | Tipo de FK | Ação tomada
///   DELETE     imediata     Incrementa o "contador de restrição imediata".
///   INSERT     imediata     Decrementa o "contador de restrição imediata".
///   DELETE     adiada       Incrementa o "contador de restrição adiada".
///   INSERT     adiada       Decrementa o "contador de restrição adiada".
///
/// Essas operações são identificadas no comentário do topo do fkey.c como "I.2" e "D.2".
fn fk_scan_children(
    p_parse: &mut Parse,
    p_src: &mut SrcList,
    p_tab: &TableRef,
    p_idx: Option<&IndexRef>,
    p_fkey: &FKeyRef,
    ai_col: Option<&[i32]>,
    reg_data: i32,
    n_incr: i32,
) {
    let db = p_parse.db.clone(); // handle do banco
    let mut p_where: Option<Box<Expr>> = None; // cláusula WHERE da varredura
    let mut i_fk_if_zero: i32 = 0; // endereço do OP_FkIfZero
    let v = get_vdbe(p_parse).expect("VDBE já alocado");
    let (n_col, is_deferred, p_from) = {
        let fk = p_fkey.borrow();
        (fk.n_col, fk.is_deferred as i32, fk.p_from.upgrade().expect("tabela filha viva"))
    };

    debug_assert!(p_idx.map_or(true, |i| {
        let idx = i.borrow();
        idx.p_table.upgrade().map_or(false, |t| Rc::ptr_eq(&t, p_tab))
    }));
    debug_assert!(p_idx.map_or(true, |i| i.borrow().n_key_col as i32 == n_col));
    debug_assert!(p_idx.is_some() || n_col == 1);
    debug_assert!(p_idx.is_some() || has_rowid(&p_tab.borrow()));

    if n_incr < 0 {
        i_fk_if_zero = vdbe_add_op2(&v, OP_FKIFZERO, is_deferred, 0);
    }

    // Cria um objeto Expr que representa uma expressão SQL como:
    //
    //   <chave-pai1> = <chave-filha1> AND <chave-pai2> = <chave-filha2> ...
    //
    // A collation usada na comparação deve ser a das colunas da chave do pai. A afinidade da
    // coluna da chave do pai deve ser aplicada a cada valor da chave filha antes da
    // comparação.
    for i in 0..n_col as usize {
        let i_col_parent: i16 = match p_idx {
            Some(idx) => idx.borrow().ai_column[i],
            None => -1,
        };
        let p_left = expr_table_register(p_parse, p_tab, reg_data, i_col_parent);
        let i_col_child: i32 = match ai_col {
            Some(a) => a[i],
            None => p_fkey.borrow().a_col[0].i_from,
        };
        debug_assert!(i_col_child >= 0);
        let z_col: Vec<u8> = p_from.borrow().a_col[i_col_child as usize].z_cn_name.clone();
        let p_right = expr(&db, TK_ID, Some(&z_col));
        let p_eq = p_expr(p_parse, TK_EQ, p_left, p_right);
        p_where = expr_and(p_parse, p_where, p_eq);
    }

    // Se a tabela filha é a mesma que a tabela pai, acrescenta termos à cláusula WHERE que
    // impedem que esta entrada seja varrida. Os termos acrescentados são assim:
    //
    //     $current_rowid!=rowid
    //     NOT( $current_a==a AND $current_b==b AND ... )
    //
    // A primeira forma serve às tabelas com rowid. A segunda serve às tabelas WITHOUT ROWID.
    // Na segunda forma, a chave *pai* é (a,b,...). Tanto a chave pai quanto a primária
    // identificariam a linha atual de modo único, mas a chave pai é mais conveniente porque
    // os valores necessários já foram carregados em registros pelo chamador.
    if Rc::ptr_eq(p_tab, &p_from) && n_incr > 0 {
        let p_ne: Option<Box<Expr>>;
        if has_rowid(&p_tab.borrow()) {
            let p_left = expr_table_register(p_parse, p_tab, reg_data, -1);
            let p_right = expr_table_column(&db, p_tab, p_src.a[0].i_cursor, -1);
            p_ne = p_expr(p_parse, TK_NE, p_left, p_right);
        } else {
            let mut p_all: Option<Box<Expr>> = None;
            debug_assert!(p_idx.is_some());
            let idx = p_idx.expect("índice da chave do pai presente");
            let (n_key_col, ai_column) = {
                let b = idx.borrow();
                (b.n_key_col as usize, b.ai_column.clone())
            };
            for i in 0..n_key_col {
                let i_col = ai_column[i];
                debug_assert!(i_col >= 0);
                let p_left = expr_table_register(p_parse, p_tab, reg_data, i_col);
                let z_col: Vec<u8> = p_tab.borrow().a_col[i_col as usize].z_cn_name.clone();
                let p_right = expr(&db, TK_ID, Some(&z_col));
                let p_eq = p_expr(p_parse, TK_IS, p_left, p_right);
                p_all = expr_and(p_parse, p_all, p_eq);
            }
            p_ne = p_expr(p_parse, TK_NOT, p_all, None);
        }
        p_where = expr_and(p_parse, p_where, p_ne);
    }

    // Resolve as referências na cláusula WHERE.
    {
        let mut s_name_context = NameContext::new(p_parse, Some(&mut *p_src));
        resolve_expr_names(&mut s_name_context, p_where.as_deref_mut());
    }

    // Cria o VDBE que percorre as entradas de pSrc que casam com a cláusula WHERE. Para cada
    // linha encontrada, incrementa o contador de restrição de chave estrangeira, adiado ou
    // imediato.
    if p_parse.n_err == 0 {
        let p_w_info = where_begin(p_parse, p_src, p_where.as_deref_mut(), None, None, None, 0, 0);
        vdbe_add_op2(&v, OP_FKCOUNTER, is_deferred, n_incr);
        if let Some(w_info) = p_w_info {
            where_end(p_parse, w_info);
        }
    }

    // Limpa a cláusula WHERE construída acima (o Drop da árvore de Box libera tudo, no lugar
    // do sqlite3ExprDelete).
    drop(p_where);
    if i_fk_if_zero != 0 {
        vdbe_jump_here_or_pop_inst(&v, i_fk_if_zero);
    }
}

/// Esta função retorna uma lista ligada de objetos FKey (ligados por `FKey.p_next_to`) com
/// todos os filhos da tabela `p_tab`. Por exemplo, dado o esquema:
///
///   CREATE TABLE t1(a PRIMARY KEY);
///   CREATE TABLE t2(b REFERENCES t1(a);
///
/// chamar esta função com a tabela "t1" devolve a FKey que representa a restrição de chave
/// estrangeira da tabela "t2". Chamar com "t2" devolve `None` (nenhuma restrição de FK tem
/// t2 como tabela pai).
pub fn fk_references(p_tab: &TableRef) -> Option<FKeyRef> {
    let tab = p_tab.borrow();
    let schema = tab.p_schema.borrow();
    hash_find(&schema.fkey_hash, &tab.z_name).cloned()
}

/// O segundo argumento é um Trigger alocado pela rotina `fk_action_trigger()`. Esta função
/// apaga o Trigger e todos os seus subcomponentes.
///
/// O Trigger ou qualquer subcomponente pode ter sido alocado do lookaside da conexão
/// `db_mem`. No porte a memória é de Box, e o Drop libera tudo.
pub fn fk_trigger_delete(_db_mem: &Sqlite3Ref, p: Option<TriggerRef>) {
    if let Some(p) = p {
        let mut trig = p.borrow_mut();
        if let Some(mut p_step) = trig.step_list.take() {
            drop(p_step.p_where.take());
            drop(p_step.p_expr_list.take());
            drop(p_step.p_select.take());
        }
        drop(trig.p_when.take());
    }
}

/// Limpa o cache `ap_trigger[]` de triggers CASCADE de todas as chaves estrangeiras de um
/// banco de dados. Isso precisa acontecer quando o schema muda.
pub fn fk_clear_trigger_cache(db: &Sqlite3Ref, i_db: i32) {
    let p_schema = db.borrow().a_db[i_db as usize].p_schema.clone();
    let tables: Vec<TableRef> = hash_values(&p_schema.borrow().tbl_hash);
    for p_tab in tables {
        if !is_ordinary_table(&p_tab.borrow()) {
            continue;
        }
        let mut p_fkey = table_p_fkey(&p_tab.borrow());
        while let Some(fk) = p_fkey {
            let (t0, t1, next) = {
                let mut f = fk.borrow_mut();
                (f.ap_trigger[0].take(), f.ap_trigger[1].take(), f.p_next_from.clone())
            };
            fk_trigger_delete(db, t0);
            fk_trigger_delete(db, t1);
            p_fkey = next;
        }
    }
}

/// Esta função é chamada para gerar o código que roda quando a tabela `p_tab` está sendo
/// removida do banco de dados. A SrcList do segundo argumento tem uma única entrada,
/// garantida de resolver para a tabela `p_tab`.
///
/// Normalmente nenhum código é necessário. Mas se
///
///   (a) a tabela é a tabela pai de uma restrição de FK, ou
///   (b) a tabela é a filha de uma restrição de FK adiada e se determina em tempo de
///       execução que há violações de FK adiadas pendentes no banco,
///
/// então o equivalente de "DELETE FROM <tbl>" é executado antes de remover a tabela do
/// banco. Os triggers ficam desabilitados durante esse DELETE, mas as ações de chave
/// estrangeira não.
pub fn fk_drop_table(p_parse: &mut Parse, p_name: &SrcList, p_tab: &TableRef) {
    let db = p_parse.db.clone();
    if (db.borrow().flags & SQLITE_FOREIGNKEYS) != 0 && is_ordinary_table(&p_tab.borrow()) {
        let mut i_skip: i32 = 0;
        let v = get_vdbe(p_parse).expect("VDBE já alocado");

        debug_assert!(is_ordinary_table(&p_tab.borrow()));
        if fk_references(p_tab).is_none() {
            // Procura uma restrição de chave estrangeira adiada da qual esta tabela é a
            // filha. Se não houver, retorna sem gerar código VDBE. Se houver, pula o DELETE
            // inteiro caso não haja restrições adiadas pendentes quando esta instrução rodar.
            let mut p = table_p_fkey(&p_tab.borrow());
            while let Some(fk) = p.clone() {
                if fk.borrow().is_deferred != 0 || (db.borrow().flags & SQLITE_DEFERFKS) != 0 {
                    break;
                }
                p = fk.borrow().p_next_from.clone();
            }
            if p.is_none() {
                return;
            }
            i_skip = vdbe_make_label(p_parse);
            vdbe_add_op2(&v, OP_FKIFZERO, 1, i_skip);
        }

        p_parse.disable_triggers = 1;
        delete_from(p_parse, src_list_dup(&db, p_name, 0), None, None, None);
        p_parse.disable_triggers = 0;

        // Se o DELETE gerou violações imediatas de restrição de chave estrangeira, interrompe
        // o VDBE e devolve um erro neste ponto, antes de qualquer modificação do schema. Isso
        // porque transações de instrução não conseguem desfazer mudanças de schema.
        //
        // Se a flag SQLITE_DeferFKs está ligada isso não é necessário, pois a transação de
        // instrução não será desfeita mesmo que haja violação de restrição de FK.
        if (db.borrow().flags & SQLITE_DEFERFKS) == 0 {
            // O sqlite3VdbeVerifyAbortable só existe sob SQLITE_DEBUG e some aqui.
            let addr = vdbe_current_addr(&v);
            vdbe_add_op2(&v, OP_FKIFZERO, 0, addr + 2);
            halt_constraint(
                p_parse,
                SQLITE_CONSTRAINT_FOREIGNKEY,
                OE_ABORT,
                None,
                P4_STATIC,
                P5_CONSTRAINTFK,
            );
        }

        if i_skip != 0 {
            vdbe_resolve_label(&v, i_skip);
        }
    }
}

/// O segundo argumento é uma FKey que representa uma chave estrangeira cuja tabela filha é
/// `p_tab`. Um UPDATE contra `p_tab` está sendo processado. Para cada coluna da tabela que
/// é de fato atualizada, o elemento correspondente de `a_change[]` é zero ou maior (se a
/// coluna não é modificada, vale -1). Se a coluna rowid é modificada pelo UPDATE, o
/// argumento `b_chng_rowid` é diferente de zero.
///
/// Retorna verdadeiro se alguma das colunas que compõem a chave filha da restrição `p` é
/// modificada.
pub fn fk_child_is_modified(p_tab: &Table, p: &FKey, a_change: &[i32], b_chng_rowid: i32) -> i32 {
    for i in 0..p.n_col as usize {
        let i_child_key = p.a_col[i].i_from;
        if a_change[i_child_key as usize] >= 0 {
            return 1;
        }
        if i_child_key == p_tab.i_p_key as i32 && b_chng_rowid != 0 {
            return 1;
        }
    }
    0
}


// ---- part_002.rs ----

/// O segundo argumento é uma FKey que representa uma chave estrangeira cuja tabela pai é
/// `p_tab`. Um UPDATE contra `p_tab` está sendo processado. Para cada coluna da tabela que
/// é de fato atualizada, o elemento correspondente de `a_change[]` é zero ou maior (se a
/// coluna não é modificada, vale -1). Se a coluna rowid é modificada pelo UPDATE, o
/// argumento `b_chng_rowid` é diferente de zero.
///
/// Retorna verdadeiro se alguma das colunas que compõem a chave do pai da restrição `p` é
/// modificada.
pub fn fk_parent_is_modified(p_tab: &Table, p: &FKey, a_change: &[i32], b_chng_rowid: i32) -> i32 {
    for i in 0..p.n_col as usize {
        let z_key = &p.a_col[i].z_col;
        for i_key in 0..p_tab.n_col as usize {
            if a_change[i_key] >= 0 || (i_key as i32 == p_tab.i_p_key as i32 && b_chng_rowid != 0) {
                let p_col = &p_tab.a_col[i_key];
                match z_key {
                    Some(z) => {
                        if 0 == str_i_cmp(&p_col.z_cn_name, z) {
                            return 1;
                        }
                    }
                    None => {
                        if (p_col.col_flags & COLFLAG_PRIMKEY) != 0 {
                            return 1;
                        }
                    }
                }
            }
        }
    }
    0
}

/// Retorna verdadeiro se o parser passado como primeiro argumento está sendo usado para
/// codificar um trigger que é na verdade uma ação "SET NULL" pertencente ao trigger de
/// `p_fkey`.
fn is_set_null_action(p_parse: &Parse, p_fkey: &FKey) -> i32 {
    // O trigger do programa do parser de nível superior (pTop->pTriggerPrg->pTrigger).
    if let Some(p) = parse_toplevel_trigger(p_parse) {
        let is_p = |slot: &Option<TriggerRef>| slot.as_ref().map_or(false, |t| Rc::ptr_eq(t, &p));
        if (is_p(&p_fkey.ap_trigger[0]) && p_fkey.a_action[0] == OE_SETNULL)
            || (is_p(&p_fkey.ap_trigger[1]) && p_fkey.a_action[1] == OE_SETNULL)
        {
            debug_assert!((p_parse.db.borrow().flags & SQLITE_FKNOACTION) == 0);
            return 1;
        }
    }
    0
}

/// Esta função é chamada ao inserir, apagar ou atualizar uma linha da tabela `p_tab`, para
/// gerar o código VDBE do processamento de restrições de chave estrangeira da operação.
///
/// Num DELETE, `reg_old` é o índice do primeiro registro de um arranjo de (pTab->nCol+1)
/// registros com o rowid da linha apagada seguido de cada valor de coluna, da esquerda para
/// a direita. `reg_new` vale zero.
///
/// Num INSERT, `reg_old` vale zero e `reg_new` é o primeiro registro de um arranjo de
/// (pTab->nCol+1) registros com os dados da nova linha.
///
/// Num UPDATE, a função é chamada duas vezes: uma antes de a linha original ser apagada,
/// com a convenção do DELETE, e outra depois de apagada e antes de a nova ser inserida, com
/// a convenção do INSERT.
pub fn fk_check(
    p_parse: &mut Parse,
    p_tab: &TableRef,
    reg_old: i32,
    reg_new: i32,
    a_change: Option<&[i32]>,
    b_chng_rowid: i32,
) {
    let db = p_parse.db.clone(); // handle do banco
    let is_ignore_errors = p_parse.disable_triggers;

    // Exatamente um entre reg_old e reg_new deve ser diferente de zero.
    debug_assert!((reg_old == 0) != (reg_new == 0));

    // Se as chaves estrangeiras estão desabilitadas, esta função não faz nada.
    if (db.borrow().flags & SQLITE_FOREIGNKEYS) == 0 {
        return;
    }
    if !is_ordinary_table(&p_tab.borrow()) {
        return;
    }

    let i_db = schema_to_index(&db.borrow(), &p_tab.borrow().p_schema);
    let z_db: Vec<u8> = db.borrow().a_db[i_db as usize].z_db_s_name.clone();

    // Percorre todas as restrições de chave estrangeira em que `p_tab` é a tabela filha (a
    // tabela da qual a definição da chave estrangeira faz parte).
    let mut cur = table_p_fkey(&p_tab.borrow());
    while let Some(p_fkey) = cur {
        cur = p_fkey.borrow().p_next_from.clone();
        let mut p_idx: Option<IndexRef> = None; // índice nas colunas de chave de p_to
        let mut ai_free: Option<Vec<i32>> = None;
        let mut b_ignore: i32 = 0;
        let (z_to, n_col, is_deferred) = {
            let fk = p_fkey.borrow();
            (fk.z_to.clone(), fk.n_col as usize, fk.is_deferred as i32)
        };

        if let Some(a_chg) = a_change {
            if api::stricmp(&p_tab.borrow().z_name, &z_to) != 0
                && fk_child_is_modified(&p_tab.borrow(), &p_fkey.borrow(), a_chg, b_chng_rowid) == 0
            {
                continue;
            }
        }

        // Encontra a tabela pai desta chave estrangeira. Encontra também um índice único nas
        // colunas de chave do pai. Se um desses itens do schema não for localizado, grava um
        // erro em p_parse e retorna cedo.
        let p_to: Option<TableRef> = if p_parse.disable_triggers != 0 {
            find_table(&db, &z_to, &z_db)
        } else {
            locate_table(p_parse, 0, &z_to, &z_db)
        };
        let located = match &p_to {
            Some(t) => fk_locate_index(p_parse, t, &p_fkey, &mut p_idx, Some(&mut ai_free)) == 0,
            None => false,
        };
        if !located {
            debug_assert!(is_ignore_errors == 0 || (reg_old != 0 && reg_new == 0));
            if is_ignore_errors == 0 || db.borrow().malloc_failed != 0 {
                return;
            }
            if p_to.is_none() {
                // Se is_ignore_errors é verdadeiro, uma tabela está sendo removida. Nesse caso
                // o SQLite roda um "DELETE FROM xxx" na tabela removida antes de removê-la,
                // para checar as restrições de FK. Se a tabela pai de uma FK da tabela atual
                // não existe, comporta-se como se estivesse vazia, ou seja, decrementa o
                // contador de FK relevante para cada linha da tabela atual com chaves não
                // NULL.
                let v = get_vdbe(p_parse).expect("VDBE já alocado");
                let i_jump = vdbe_current_addr(&v) + n_col as i32 + 1;
                let p_from = p_fkey.borrow().p_from.upgrade().expect("tabela filha viva");
                for i in 0..n_col {
                    let i_from_col = p_fkey.borrow().a_col[i].i_from;
                    let i_reg = table_column_to_storage(&p_from.borrow(), i_from_col as i16) as i32
                        + reg_old
                        + 1;
                    vdbe_add_op2(&v, OP_ISNULL, i_reg, i_jump);
                }
                vdbe_add_op2(&v, OP_FKCOUNTER, is_deferred, -1);
            }
            continue;
        }
        let p_to = p_to.expect("tabela pai localizada");
        debug_assert!(n_col == 1 || (ai_free.is_some() && p_idx.is_some()));

        // aiCol aponta para aiFree ou para o iCol local de uma só coluna do C; aqui os dois
        // casos são um vetor próprio.
        let mut ai_col: Vec<i32> = match ai_free {
            Some(v) => v,
            None => vec![p_fkey.borrow().a_col[0].i_from],
        };
        let i_p_key = p_tab.borrow().i_p_key as i32;
        for i in 0..n_col {
            if ai_col[i] == i_p_key {
                ai_col[i] = -1;
            }
            debug_assert!(p_idx.as_ref().map_or(true, |x| x.borrow().ai_column[i] >= 0));
            // Pede permissão para ler as colunas de chave do pai. Se o callback de
            // autorização devolve SQLITE_IGNORE, comporta-se como se os valores lidos da
            // tabela pai fossem NULL.
            if db.borrow().x_auth.is_some() {
                let (z_col, z_to_name): (Vec<u8>, Vec<u8>) = {
                    let to = p_to.borrow();
                    let i_c = match &p_idx {
                        Some(x) => x.borrow().ai_column[i],
                        None => to.i_p_key,
                    };
                    (to.a_col[i_c as usize].z_cn_name.clone(), to.z_name.clone())
                };
                let rcauth = auth_read_col(p_parse, &z_to_name, &z_col, i_db);
                b_ignore = (rcauth == SQLITE_IGNORE) as i32;
            }
        }

        // Toma um read-lock consultivo de cache compartilhado na tabela pai. Aloca um cursor
        // para pesquisar o índice único nas colunas de chave do pai.
        let (to_tnum, to_name) = {
            let to = p_to.borrow();
            (to.tnum, to.z_name.clone())
        };
        table_lock(p_parse, i_db, to_tnum, 0, &to_name);
        p_parse.n_tab += 1;

        if reg_old != 0 {
            // Uma linha está sendo removida da tabela filha. Procura o pai. Se o pai não
            // existe, remover a linha filha resolve uma violação pendente de chave
            // estrangeira.
            fk_lookup_parent(p_parse, i_db, &p_to, p_idx.as_ref(), &p_fkey, &ai_col, reg_old, -1, b_ignore);
        }
        if reg_new != 0 && is_set_null_action(p_parse, &p_fkey.borrow()) == 0 {
            // Uma linha está sendo acrescentada à tabela filha. Se a linha do pai não é
            // encontrada, acrescentar a linha filha violou a restrição de FK.
            //
            // Se esta operação faz parte de um programa de trigger que é na verdade uma ação
            // "SET NULL" desta mesma chave estrangeira, omite a varredura por completo. Como
            // todos os valores da chave filha são NULL por garantia, acrescentar a linha não
            // pode causar violação.
            fk_lookup_parent(p_parse, i_db, &p_to, p_idx.as_ref(), &p_fkey, &ai_col, reg_new, 1, b_ignore);
        }
        // aiFree é liberado pelo Drop de ai_col.
    }

    // Percorre todas as restrições de chave estrangeira que se referem a esta tabela (as
    // restrições "filhas").
    let mut cur = fk_references(p_tab);
    while let Some(p_fkey) = cur {
        cur = p_fkey.borrow().p_next_to.clone();
        let mut p_idx: Option<IndexRef> = None; // índice da chave estrangeira de p_fkey
        let mut ai_col: Option<Vec<i32>> = None;

        if let Some(a_chg) = a_change {
            if fk_parent_is_modified(&p_tab.borrow(), &p_fkey.borrow(), a_chg, b_chng_rowid) == 0 {
                continue;
            }
        }

        if p_fkey.borrow().is_deferred == 0
            && (db.borrow().flags & SQLITE_DEFERFKS) == 0
            && p_parse.p_toplevel.is_none()
            && p_parse.is_multi_write == 0
        {
            debug_assert!(reg_old == 0 && reg_new != 0);
            // Inserir uma única linha numa tabela pai não pode causar (nem corrigir) uma
            // violação imediata de chave estrangeira. Então não faz nada neste caso.
            continue;
        }

        if fk_locate_index(p_parse, p_tab, &p_fkey, &mut p_idx, Some(&mut ai_col)) != 0 {
            if is_ignore_errors == 0 || db.borrow().malloc_failed != 0 {
                return;
            }
            continue;
        }
        debug_assert!(ai_col.is_some() || p_fkey.borrow().n_col == 1);

        // Cria uma SrcList contendo a tabela filha. Precisamos da tabela filha como SrcList
        // para o sqlite3WhereBegin().
        let p_src = src_list_append(p_parse, None, None, None);
        if let Some(mut p_src) = p_src {
            let p_from = p_fkey.borrow().p_from.upgrade().expect("tabela filha viva");
            {
                let item = &mut p_src.a[0];
                item.p_tab = Some(p_from.clone());
                item.z_name = Some(p_from.borrow().z_name.clone());
                p_from.borrow_mut().n_tab_ref += 1;
                item.i_cursor = p_parse.n_tab;
                p_parse.n_tab += 1;
            }

            if reg_new != 0 {
                fk_scan_children(
                    p_parse,
                    &mut p_src,
                    p_tab,
                    p_idx.as_ref(),
                    &p_fkey,
                    ai_col.as_deref(),
                    reg_new,
                    -1,
                );
            }
            if reg_old != 0 {
                let mut e_action = p_fkey.borrow().a_action[a_change.is_some() as usize];
                if (db.borrow().flags & SQLITE_FKNOACTION) != 0 {
                    e_action = OE_NONE;
                }

                fk_scan_children(
                    p_parse,
                    &mut p_src,
                    p_tab,
                    p_idx.as_ref(),
                    &p_fkey,
                    ai_col.as_deref(),
                    reg_old,
                    1,
                );
                // Se esta é uma restrição de FK adiada, ou se uma ação CASCADE ou SET NULL se
                // aplica, qualquer violação causada ao remover a chave do pai será retificada
                // pelo trigger da ação. Então não liga a flag "may-abort" neste caso.
                //
                // Nota 1: se a FK é declarada "ON UPDATE CASCADE", a flag may-abort acabará
                // ligada nesta instrução de qualquer forma (quando esta função for chamada no
                // processamento do UPDATE dentro do trigger da ação).
                //
                // Nota 2: à primeira vista parece que o SQLite poderia omitir todas as
                // varreduras do OP_FkCounter quando CASCADE ou SET NULL se aplica. O problema
                // começa se o trigger da ação CASCADE ou SET NULL faz disparar outros
                // triggers ou regras de ação ligados à tabela filha. Nesses casos os
                // contadores da restrição de FK podem ficar errados se alguma varredura do
                // OP_FkCounter for omitida.
                if p_fkey.borrow().is_deferred == 0
                    && e_action != OE_CASCADE
                    && e_action != OE_SETNULL
                {
                    may_abort(p_parse);
                }
            }
            // O nome da tabela não pertence ao item (no C é emprestado da tabela), então se
            // zera antes de a SrcList ser destruída.
            p_src.a[0].z_name = None;
            src_list_delete(&db, p_src);
        }
        // aiCol é liberado pelo Drop.
    }
}

/// Macro COLUMN_MASK do C: máscara de uma coluna, saturada em 0xffffffff acima da 31.
#[inline]
pub fn column_mask(x: i32) -> u32 {
    if x > 31 {
        0xffffffff
    } else {
        1u32 << x
    }
}

/// Esta função é chamada antes de gerar código para atualizar ou apagar uma linha contida
/// na tabela `p_tab`. Retorna a máscara das colunas da linha antiga exigidas pelo
/// processamento de chaves estrangeiras.
pub fn fk_oldmask(p_parse: &mut Parse, p_tab: &TableRef) -> u32 {
    let mut mask: u32 = 0;
    let flags = p_parse.db.borrow().flags;
    if (flags & SQLITE_FOREIGNKEYS) != 0 && is_ordinary_table(&p_tab.borrow()) {
        let mut cur = table_p_fkey(&p_tab.borrow());
        while let Some(p) = cur {
            let fk = p.borrow();
            for i in 0..fk.n_col as usize {
                mask |= column_mask(fk.a_col[i].i_from);
            }
            cur = fk.p_next_from.clone();
        }
        let mut cur = fk_references(p_tab);
        while let Some(p) = cur {
            cur = p.borrow().p_next_to.clone();
            let mut p_idx: Option<IndexRef> = None;
            fk_locate_index(p_parse, p_tab, &p, &mut p_idx, None);
            if let Some(idx_ref) = p_idx {
                let idx = idx_ref.borrow();
                for i in 0..idx.n_key_col as usize {
                    debug_assert!(idx.ai_column[i] >= 0);
                    mask |= column_mask(idx.ai_column[i] as i32);
                }
            }
        }
    }
    mask
}

/// Esta função é chamada antes de gerar código para atualizar ou apagar uma linha contida
/// na tabela `p_tab`. Se a operação é um DELETE, `a_change` é `None`. Para um UPDATE,
/// `a_change` aponta para um arranjo de tamanho N (N colunas de `p_tab`). Se a i-ésima
/// coluna não é modificada, a entrada vale -1. Se é modificada, vale 0 ou mais. `chng_rowid`
/// é verdadeiro se o UPDATE modifica o rowid da tabela.
///
/// Se algum processamento de chave estrangeira for necessário, retorna não zero. Se não há
/// processamento relacionado a chaves estrangeiras, retorna zero.
///
/// Para um UPDATE, retorna 2 se:
///
///   * há FKs em que `p_tab` é filha e pai ao mesmo tempo e qualquer processamento de FK é
///     necessário (mesmo de outra FK), ou
///
///   * o UPDATE modifica uma ou mais chaves de pai cuja ação não é "NO ACTION" (ou seja, é
///     CASCADE, SET DEFAULT ou SET NULL).
///
/// Ou, supondo que algum outro processamento de FK é necessário, 1.
pub fn fk_required(
    p_parse: &Parse,
    p_tab: &TableRef,
    a_change: Option<&[i32]>,
    chng_rowid: i32,
) -> i32 {
    let mut e_ret: i32 = 1; // valor retornado se b_have_fk é verdadeiro
    let mut b_have_fk: i32 = 0; // se o processamento de FK é necessário
    let flags = p_parse.db.borrow().flags;
    if (flags & SQLITE_FOREIGNKEYS) != 0 && is_ordinary_table(&p_tab.borrow()) {
        match a_change {
            None => {
                // Uma operação DELETE. O processamento de FK é necessário se a tabela é a
                // filha ou a pai de qualquer restrição de chave estrangeira.
                b_have_fk =
                    (fk_references(p_tab).is_some() || table_p_fkey(&p_tab.borrow()).is_some()) as i32;
            }
            Some(a_chg) => {
                // Este é um UPDATE. O processamento de FK só é necessário se a operação
                // modifica uma ou mais colunas de chave filha ou pai.

                // Verifica se alguma coluna de chave filha está sendo modificada.
                let mut cur = table_p_fkey(&p_tab.borrow());
                while let Some(p) = cur {
                    let fk = p.borrow();
                    if fk_child_is_modified(&p_tab.borrow(), &fk, a_chg, chng_rowid) != 0 {
                        if 0 == api::stricmp(&p_tab.borrow().z_name, &fk.z_to) {
                            e_ret = 2;
                        }
                        b_have_fk = 1;
                    }
                    cur = fk.p_next_from.clone();
                }

                // Verifica se alguma coluna de chave do pai está sendo modificada.
                let mut cur = fk_references(p_tab);
                while let Some(p) = cur {
                    let fk = p.borrow();
                    if fk_parent_is_modified(&p_tab.borrow(), &fk, a_chg, chng_rowid) != 0 {
                        if (flags & SQLITE_FKNOACTION) == 0 && fk.a_action[1] != OE_NONE {
                            return 2;
                        }
                        b_have_fk = 1;
                    }
                    cur = fk.p_next_to.clone();
                }
            }
        }
    }
    if b_have_fk != 0 {
        e_ret
    } else {
        0
    }
}


// ---- part_003.rs ----

/// Esta função é chamada quando uma operação UPDATE ou DELETE está sendo compilada na tabela
/// `p_tab`, que é a tabela pai da chave estrangeira `p_fkey`. Se a operação é um UPDATE,
/// `p_changes` recebe a lista de colunas modificadas. Se é um DELETE, recebe `None`.
///
/// Retorna um Trigger equivalente à ação ON UPDATE ou ON DELETE especificada por `p_fkey`.
/// Se a ação é "NO ACTION" retorna `None` (essas ações não exigem tratamento especial do
/// subsistema de triggers; o código delas é criado por `fk_scan_children()`).
///
/// Por exemplo, se `p_fkey` é a chave estrangeira e `p_tab` é a tabela "p" do esquema:
///
///   CREATE TABLE p(pk PRIMARY KEY);
///   CREATE TABLE c(ck REFERENCES p ON DELETE CASCADE);
///
/// então o Trigger devolvido equivale a:
///
///   CREATE TRIGGER ... DELETE ON p BEGIN
///     DELETE FROM c WHERE ck = old.pk;
///   END;
///
/// O Trigger devolvido fica em cache como parte do objeto da chave estrangeira. É liberado
/// junto com o resto do objeto por `fk_delete()`.
fn fk_action_trigger(
    p_parse: &mut Parse,
    p_tab: &TableRef,
    p_fkey: &FKeyRef,
    p_changes: Option<&ExprList>,
) -> Option<TriggerRef> {
    let db = p_parse.db.clone(); // handle do banco
    let i_action = p_changes.is_some() as usize; // 1 para UPDATE, 0 para DELETE

    let mut action = p_fkey.borrow().a_action[i_action]; // OE_None, OE_Cascade etc.
    if (db.borrow().flags & SQLITE_FKNOACTION) != 0 {
        action = OE_NONE;
    }
    if action == OE_RESTRICT && (db.borrow().flags & SQLITE_DEFERFKS) != 0 {
        return None;
    }
    let mut p_trigger: Option<TriggerRef> = p_fkey.borrow().ap_trigger[i_action].clone();

    if action != OE_NONE && p_trigger.is_none() {
        let mut p_idx: Option<IndexRef> = None; // índice da chave do pai desta FK
        let mut ai_col: Option<Vec<i32>> = None; // colunas filhas -> colunas da chave do pai
        let mut p_where: Option<Box<Expr>> = None; // WHERE do passo do trigger
        let mut p_list: Option<Box<ExprList>> = None; // lista de mudanças se ON UPDATE CASCADE
        let mut p_select: Option<Box<Select>> = None; // se RESTRICT, "SELECT RAISE(...)"
        let mut p_when: Option<Box<Expr>> = None; // cláusula WHEN do trigger

        if fk_locate_index(p_parse, p_tab, p_fkey, &mut p_idx, Some(&mut ai_col)) != 0 {
            return None;
        }
        let (n_col, p_from) = {
            let fk = p_fkey.borrow();
            (fk.n_col as usize, fk.p_from.upgrade().expect("tabela filha viva"))
        };
        debug_assert!(ai_col.is_some() || n_col == 1);

        for i in 0..n_col {
            let t_old = token_init(b"old"); // token literal "old"
            let t_new = token_init(b"new"); // token literal "new"

            let i_from_col: i32 = match &ai_col {
                Some(a) => a[i],
                None => p_fkey.borrow().a_col[0].i_from,
            };
            debug_assert!(i_from_col >= 0);
            let z_to_col: Vec<u8> = {
                let tab = p_tab.borrow();
                debug_assert!(p_idx.is_some() || (tab.i_p_key >= 0 && tab.i_p_key < tab.n_col));
                let i_c = match &p_idx {
                    Some(x) => {
                        let c = x.borrow().ai_column[i];
                        debug_assert!(c >= 0);
                        c
                    }
                    None => tab.i_p_key,
                };
                tab.a_col[i_c as usize].z_cn_name.clone()
            };
            let z_from_col: Vec<u8> = p_from.borrow().a_col[i_from_col as usize].z_cn_name.clone();
            let t_to_col = token_init(&z_to_col); // nome da coluna na tabela pai
            let t_from_col = token_init(&z_from_col); // nome da coluna na tabela filha

            // Cria a expressão "OLD.zToCol = zFromCol". É importante que o termo
            // "OLD.zToCol" fique à esquerda do operador =, para que a afinidade e a
            // collation da tabela pai sejam usadas na comparação.
            let p_old_id = expr_alloc(&db, TK_ID, &t_old, 0);
            let p_to_id = expr_alloc(&db, TK_ID, &t_to_col, 0);
            let p_dot = p_expr(p_parse, TK_DOT, p_old_id, p_to_id);
            let p_from_id = expr_alloc(&db, TK_ID, &t_from_col, 0);
            let p_eq = p_expr(p_parse, TK_EQ, p_dot, p_from_id);
            p_where = expr_and(p_parse, p_where, p_eq);

            // Para ON UPDATE, constrói o próximo termo da cláusula WHEN. A WHEN final fica
            // assim:
            //
            //    WHEN NOT(old.col1 IS new.col1 AND ... AND old.colN IS new.colN)
            if p_changes.is_some() {
                let p_old_id = expr_alloc(&db, TK_ID, &t_old, 0);
                let p_to_id = expr_alloc(&db, TK_ID, &t_to_col, 0);
                let p_left = p_expr(p_parse, TK_DOT, p_old_id, p_to_id);
                let p_new_id = expr_alloc(&db, TK_ID, &t_new, 0);
                let p_to_id2 = expr_alloc(&db, TK_ID, &t_to_col, 0);
                let p_right = p_expr(p_parse, TK_DOT, p_new_id, p_to_id2);
                let p_eq = p_expr(p_parse, TK_IS, p_left, p_right);
                p_when = expr_and(p_parse, p_when, p_eq);
            }

            if action != OE_RESTRICT && (action != OE_CASCADE || p_changes.is_some()) {
                let p_new: Option<Box<Expr>>;
                if action == OE_CASCADE {
                    let p_new_id = expr_alloc(&db, TK_ID, &t_new, 0);
                    let p_to_id = expr_alloc(&db, TK_ID, &t_to_col, 0);
                    p_new = p_expr(p_parse, TK_DOT, p_new_id, p_to_id);
                } else if action == OE_SETDFLT {
                    let from = p_from.borrow();
                    let p_col = &from.a_col[i_from_col as usize];
                    let p_dflt: Option<&Expr> = if (p_col.col_flags & COLFLAG_GENERATED) != 0 {
                        None
                    } else {
                        column_expr(&from, p_col)
                    };
                    p_new = match p_dflt {
                        Some(d) => expr_dup(&db, Some(d), 0),
                        None => expr_alloc(&db, TK_NULL, &Token::default(), 0),
                    };
                } else {
                    p_new = expr_alloc(&db, TK_NULL, &Token::default(), 0);
                }
                p_list = expr_list_append(p_parse, p_list, p_new);
                expr_list_set_name(p_parse, p_list.as_deref_mut(), &t_from_col, 0);
            }
        }
        drop(ai_col);

        let z_from: Vec<u8> = p_from.borrow().z_name.clone(); // nome da tabela filha

        if action == OE_RESTRICT {
            let i_db = schema_to_index(&db.borrow(), &p_tab.borrow().p_schema);

            let mut p_raise = expr(&db, TK_RAISE, Some(b"FOREIGN KEY constraint failed"));
            if let Some(r) = p_raise.as_mut() {
                r.aff_expr = OE_ABORT as i8;
            }
            let mut p_src = src_list_append(p_parse, None, None, None);
            if let Some(s) = p_src.as_mut() {
                debug_assert!(s.n_src == 1);
                s.a[0].z_name = Some(z_from.clone());
                s.a[0].z_database = Some(db.borrow().a_db[i_db as usize].z_db_s_name.clone());
            }
            let p_elist = expr_list_append(p_parse, None, p_raise);
            p_select = select_new(p_parse, p_elist, p_src, p_where.take(), None, None, None, 0, None);
        }

        // Desabilita a alocação de memória por lookaside.
        disable_lookaside(&db);

        // No C: sqlite3DbMallocZero(sizeof(Trigger) + sizeof(TriggerStep) + nFrom + 1), com o
        // passo e o zTarget na mesma alocação. Aqui o Trigger, o passo e o nome são donos
        // próprios, e a alocação não falha.
        let p_trig: TriggerRef = Rc::new(RefCell::new(Trigger::default()));
        {
            let mut t = p_trig.borrow_mut();
            let mut p_step = Box::new(TriggerStep::default());
            p_step.z_target = z_from.clone();
            p_step.p_where = expr_dup(&db, p_where.as_deref(), EXPRDUP_REDUCE);
            p_step.p_expr_list = expr_list_dup(&db, p_list.as_deref(), EXPRDUP_REDUCE);
            p_step.p_select = select_dup(&db, p_select.as_deref(), EXPRDUP_REDUCE);
            if p_when.is_some() {
                p_when = p_expr(p_parse, TK_NOT, p_when.take(), None);
                t.p_when = expr_dup(&db, p_when.as_deref(), EXPRDUP_REDUCE);
            }
            t.step_list = Some(p_step);
        }

        // Reabilita o buffer de lookaside, se foi desabilitado antes.
        enable_lookaside(&db);

        // Libera os temporários (expr_delete, expr_list_delete e select_delete do C são o
        // Drop).
        drop(p_where);
        drop(p_when);
        drop(p_list);
        drop(p_select);
        if db.borrow().malloc_failed == 1 {
            fk_trigger_delete(&db, Some(p_trig));
            return None;
        }

        {
            let mut t = p_trig.borrow_mut();
            let p_step = t.step_list.as_mut().expect("passo do trigger presente");
            match action {
                a if a == OE_RESTRICT => {
                    p_step.op = TK_SELECT;
                }
                a if a == OE_CASCADE && p_changes.is_none() => {
                    p_step.op = TK_DELETE;
                }
                _ => {
                    // OE_CASCADE com p_changes cai aqui (deliberate_fall_through do C).
                    p_step.op = TK_UPDATE;
                }
            }
            p_step.p_trig = Rc::downgrade(&p_trig);
            let p_schema = p_tab.borrow().p_schema.clone();
            t.p_schema = p_schema.clone();
            t.p_tab_schema = p_schema;
            t.op = if p_changes.is_some() { TK_UPDATE } else { TK_DELETE };
        }
        p_fkey.borrow_mut().ap_trigger[i_action] = Some(p_trig.clone());
        p_trigger = Some(p_trig);
    }

    p_trigger
}

/// Esta função é chamada ao apagar ou atualizar uma linha, para implementar as ações
/// CASCADE, SET NULL ou SET DEFAULT exigidas.
pub fn fk_actions(
    p_parse: &mut Parse,
    p_tab: &TableRef,
    p_changes: Option<&ExprList>,
    reg_old: i32,
    a_change: Option<&[i32]>,
    b_chng_rowid: i32,
) {
    // Se o suporte a chaves estrangeiras está ligado, percorre todas as FKs que se referem
    // à tabela `p_tab`. Se há uma ação associada à FK para esta operação (UPDATE ou DELETE),
    // invoca o subprograma de trigger associado.
    let flags = p_parse.db.borrow().flags;
    if (flags & SQLITE_FOREIGNKEYS) != 0 {
        let mut cur = fk_references(p_tab);
        while let Some(p_fkey) = cur {
            cur = p_fkey.borrow().p_next_to.clone();
            let modified = match a_change {
                None => true,
                Some(a_chg) => {
                    fk_parent_is_modified(&p_tab.borrow(), &p_fkey.borrow(), a_chg, b_chng_rowid) != 0
                }
            };
            if modified {
                let p_act = fk_action_trigger(p_parse, p_tab, &p_fkey, p_changes);
                if let Some(p_act) = p_act {
                    code_row_trigger_direct(p_parse, &p_act, p_tab, reg_old, OE_ABORT, 0);
                }
            }
        }
    }
}

/// Libera toda a memória associada às definições de chave estrangeira ligadas à tabela
/// `p_tab`. Remove as chaves estrangeiras apagadas da tabela hash `Schema.fkey_hash`.
pub fn fk_delete(db: &Sqlite3Ref, p_tab: &TableRef) {
    debug_assert!(is_ordinary_table(&p_tab.borrow()));

    // No C a lista fica pendurada em pTab após a liberação; aqui a lista é retirada da
    // tabela para o Drop dos nós acontecer e nenhuma referência sobrar.
    let mut cur: Option<FKeyRef> = match &mut p_tab.borrow_mut().u {
        TableU::Tab(tab) => tab.p_fkey.take(),
        _ => None,
    };
    while let Some(p_fkey) = cur {
        // Remove a FK da tabela hash fkeyHash.
        if db.borrow().p_n_bytes_freed.is_none() {
            let (p_prev_to, p_next_to, z_to) = {
                let fk = p_fkey.borrow();
                (fk.p_prev_to.clone(), fk.p_next_to.clone(), fk.z_to.clone())
            };
            match p_prev_to.as_ref().and_then(|w| w.upgrade()) {
                Some(prev) => {
                    prev.borrow_mut().p_next_to = p_next_to.clone();
                }
                None => {
                    let z: Vec<u8> = match &p_next_to {
                        Some(n) => n.borrow().z_to.clone(),
                        None => z_to,
                    };
                    let p_schema = p_tab.borrow().p_schema.clone();
                    hash_insert(&mut p_schema.borrow_mut().fkey_hash, &z, p_next_to.clone());
                }
            }
            if let Some(n) = &p_next_to {
                n.borrow_mut().p_prev_to = p_prev_to;
            }
        }

        // EV: R-30323-21917 Cada restrição de chave estrangeira no SQLite é classificada
        // como imediata ou adiada.
        debug_assert!(p_fkey.borrow().is_deferred == 0 || p_fkey.borrow().is_deferred == 1);

        // Apaga os triggers criados para implementar ações desta FK.
        let (t0, t1, next) = {
            let mut fk = p_fkey.borrow_mut();
            (fk.ap_trigger[0].take(), fk.ap_trigger[1].take(), fk.p_next_from.take())
        };
        fk_trigger_delete(db, t0);
        fk_trigger_delete(db, t1);

        cur = next;
        // sqlite3DbFree(db, pFKey): o Drop de p_fkey libera o nó.
    }
}

