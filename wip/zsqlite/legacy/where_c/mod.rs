// Mesclado das partes traduzidas de where_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Modelo adotado neste trecho (o mesmo dos trechos 002 e 003):
// - `WhereInfoRef`, `WhereClauseRef`, `WhereTermRef`, `ParseRef`, `VdbeRef` são `Rc<RefCell<_>>`;
//   o ponteiro de volta `WhereClause.p_outer` e `WhereClause.p_w_info` é `Weak`.
// - `WhereClause.a` é `Vec<WhereTermRef>` e `n_term` é o `nTerm` do C.
// - `WhereTerm.p_expr` é `Option<ExprRef>` (`Rc<RefCell<Expr>>`); `Expr.p_left`/`p_right` são
//   `Option<Box<Expr>>`; `ExprList.a[i].p_expr` é `Option<Box<Expr>>`.
// - `WhereTerm.u` é o enum `WhereTermUnion`, e o ramo `X { left_column, i_field }` é o `u.x` do C.
// - As seções `WHERETRACE_ENABLED` só existem sob SQLITE_DEBUG/SQLITE_TEST e foram omitidas.

/// Lê `pTerm->u.x.leftColumn`.
#[inline]
pub fn where_term_left_column(p_term: &WhereTerm) -> i32 {
    match &p_term.u {
        WhereTermUnion::X { left_column, .. } => *left_column,
        _ => unreachable!(),
    }
}

/// Lê `pTerm->u.x.iField`.
#[inline]
pub fn where_term_i_field(p_term: &WhereTerm) -> i32 {
    match &p_term.u {
        WhereTermUnion::X { i_field, .. } => *i_field,
        _ => unreachable!(),
    }
}

/// Informação extra anexada ao fim de sqlite3_index_info mas não visível
/// diretamente à função xBestIndex. A interface sqlite3_vtab_collation()
/// sabe como alcançá-la, porém.
///
/// Este objeto não é uma API e pode mudar de uma versão a outra.
/// Desde que allocate_index_info() e sqlite3_vtab_collation()
/// concordem sobre a estrutura, tudo bem.
pub struct HiddenIndexInfo {
    /// A cláusula WHERE sendo analisada
    pub p_wc: Option<WhereClauseRef>,
    /// O contexto de análise
    pub p_parse: Option<ParseRef>,
    /// Valor a retornar de sqlite3_vtab_distinct()
    pub e_distinct: i32,
    /// Máscara de termos que são <col> IN (...)
    pub m_in: u32,
    /// Termos que vtab tratará como <col> IN (...)
    pub m_handle_in: u32,
    /// Valores RHS das restrições. No C é o último campo porque espaço extra é
    /// alocado para até nTerm tais valores; aqui o `Vec` tem esse tamanho.
    pub a_rhs: Vec<Option<ValueRef>>,
}

/// Retorna o número estimado de linhas de saída de uma cláusula WHERE.
pub fn where_output_row_count(p_w_info: &WhereInfoRef) -> LogEst {
    p_w_info.borrow().n_row_out
}

/// Retorna um dos valores WHERE_DISTINCT_xxxxx para indicar como esta
/// cláusula WHERE retorna saídas para processamento de DISTINCT.
pub fn where_is_distinct(p_w_info: &WhereInfoRef) -> i32 {
    p_w_info.borrow().e_distinct as i32
}

/// Retorna o número de termos ORDER BY satisfeitos pela cláusula WHERE.
/// Um retorno de 0 significa que a saída deve ser completamente ordenada.
/// Um retorno igual ao número de termos ORDER BY significa que nenhuma
/// ordenação é necessária. Um retorno que é positivo mas menor que o
/// número de termos ORDER BY significa que ordenação de bloco é necessária.
pub fn where_is_ordered(p_w_info: &WhereInfoRef) -> i32 {
    let n_ob_sat = p_w_info.borrow().n_ob_sat;
    if n_ob_sat < 0 {
        0
    } else {
        n_ob_sat as i32
    }
}

/// Na otimização ORDER BY LIMIT, se o loop mais interno é conhecido
/// por emitir linhas em ordem crescente, e se a última linha emitida
/// pelo loop mais interno não se encaixou no ordenador, então podemos
/// pular todas as linhas subsequentes da iteração atual do loop interno
/// (porque também não se encaixarão no ordenador) e continuar com o
/// segundo loop interno, o loop imediatamente exterior do mais interno.
///
/// Quando uma linha não se encaixa no ordenador (porque o ordenador
/// já contém LIMIT+OFFSET linhas que são menores), então um salto é
/// feito para o rótulo retornado por esta função.
///
/// Se a otimização ORDER BY LIMIT se aplica, o destino do salto deve ser
/// a continuação do segundo loop mais interno. Se a otimização ORDER BY
/// LIMIT não se aplica, então o destino do salto deve ser a continuação
/// do loop mais interno.
///
/// É sempre seguro para esta rotina retornar a continuação do loop mais
/// interno, no sentido de que uma resposta correta resultará.
/// Retornar a continuação do segundo loop interno é uma otimização que
/// pode fazer o código rodar um pouco mais rápido, mas não deve mudar
/// a resposta final.
pub fn where_order_by_limit_opt_label(p_w_info: &WhereInfoRef) -> i32 {
    let info = p_w_info.borrow();
    if !info.b_ordered_inner_loop {
        // A otimização ORDER BY LIMIT não se aplica. Salta para a
        // continuação do loop mais interno.
        return info.i_continue;
    }
    let p_inner = &info.a[(info.n_level - 1) as usize];
    assert!(p_inner.addr_nxt != 0);
    if p_inner.p_rj.is_some() {
        info.i_continue
    } else {
        p_inner.addr_nxt
    }
}

/// Enquanto gerava código para a otimização min/max, após tratar a
/// chamada agregado-passo para min() ou max(), verifica se há laço
/// adicional necessário. Se a ordem de saída é tal que estamos certos
/// de que a resposta correta já foi encontrada, então codifica um OP_Goto
/// para desviar do processamento subsequente.
///
/// Qualquer OP_Goto extra codificado aqui é uma otimização. A resposta
/// correta deve ser obtida independentemente. Este OP_Goto apenas faz
/// a resposta aparecer mais rápido.
pub fn where_min_max_opt_early_out(v: &VdbeRef, p_w_info: &WhereInfoRef) {
    let info = p_w_info.borrow();
    if !info.b_ordered_inner_loop {
        return;
    }
    if info.n_ob_sat == 0 {
        return;
    }
    let mut i = info.n_level - 1;
    while i >= 0 {
        let p_inner = &info.a[i as usize];
        let ws_flags = p_inner.p_w_loop.as_ref().unwrap().borrow().ws_flags;
        if (ws_flags & WHERE_COLUMN_IN) != 0 {
            vdbe_goto(&mut v.borrow_mut(), p_inner.addr_nxt);
            return;
        }
        i -= 1;
    }
    vdbe_goto(&mut v.borrow_mut(), info.i_break);
}

/// Retorna o endereço ou rótulo VDBE para saltar a fim de continuar
/// imediatamente com a próxima linha de uma cláusula WHERE.
pub fn where_continue_label(p_w_info: &WhereInfoRef) -> i32 {
    let i_continue = p_w_info.borrow().i_continue;
    assert!(i_continue != 0);
    i_continue
}

/// Retorna o endereço ou rótulo VDBE para saltar a fim de sair de
/// um loop WHERE.
pub fn where_break_label(p_w_info: &WhereInfoRef) -> i32 {
    p_w_info.borrow().i_break
}

/// Retorna ONEPASS_OFF (0) se uma instrução UPDATE ou DELETE não conseguir
/// operar diretamente nos rowids retornados por uma cláusula WHERE.
/// Retorna ONEPASS_SINGLE (1) se a instrução pode operar diretamente
/// porque apenas uma linha será mudada. Retorna ONEPASS_MULTI (2) se a
/// otimização de uma passagem pode ser usada em múltiplas linhas.
///
/// Se a otimização ONEPASS é usada (se esta rotina retorna verdadeiro)
/// então também escreve os índices de cursores abertos usados por ONEPASS
/// em ai_cur[0] e ai_cur[1]. ai_cur[0] obtém o cursor da tabela de dados
/// e ai_cur[1] obtém o cursor usado por um índice auxiliar.
/// Qualquer valor pode ser -1, indicando que este cursor não é usado.
/// Qualquer cursor retornado terá sido aberto para escrita.
///
/// ai_cur[0] e ai_cur[1] ambos obtêm -1 se a lógica da cláusula where é
/// incapaz de usar a otimização ONEPASS.
pub fn where_ok_one_pass(p_w_info: &WhereInfoRef, ai_cur: &mut [i32; 2]) -> i32 {
    let info = p_w_info.borrow();
    ai_cur.copy_from_slice(&info.ai_cur_one_pass);
    info.e_one_pass as i32
}

/// Retorna VERDADEIRO se o loop WHERE usa o opcode OP_DeferredSeek para
/// mover o cursor de dados para a linha selecionada pelo cursor de índice.
pub fn where_uses_deferred_seek(p_w_info: &WhereInfoRef) -> i32 {
    p_w_info.borrow().b_deferred_seek as i32
}

/// Move o conteúdo de p_src para p_dest.
fn where_or_move(p_dest: &mut WhereOrSet, p_src: &WhereOrSet) {
    p_dest.n = p_src.n;
    for i in 0..(p_dest.n as usize) {
        p_dest.a[i] = p_src.a[i];
    }
}

/// Tenta inserir uma entrada nova de pré-requisito/custo no WhereOrSet p_set.
///
/// A entrada nova pode sobrescrever uma entrada existente, ou pode ser
/// anexada, ou pode ser descartada. Faça o que for certo para que p_set
/// mantenha as N_OR_COST melhores entradas vistas até agora.
fn where_or_insert(
    p_set: &mut WhereOrSet,
    prereq: Bitmask,
    r_run: LogEst,
    n_out: LogEst,
) -> i32 {
    // `p` é o índice do `WhereOrCost *p` do C.
    let mut p: usize = 0;
    let mut found = false;
    for i in 0..(p_set.n as usize) {
        p = i;
        if r_run <= p_set.a[i].r_run && (prereq & p_set.a[i].prereq) == prereq {
            // goto whereOrInsert_done
            found = true;
            break;
        }
        if p_set.a[i].r_run <= r_run && (p_set.a[i].prereq & prereq) == p_set.a[i].prereq {
            return 0;
        }
    }
    if !found {
        if (p_set.n as usize) < N_OR_COST {
            p = p_set.n as usize;
            p_set.n += 1;
            p_set.a[p].n_out = n_out;
        } else {
            p = 0;
            for i in 1..(p_set.n as usize) {
                if p_set.a[p].r_run > p_set.a[i].r_run {
                    p = i;
                }
            }
            if p_set.a[p].r_run <= r_run {
                return 0;
            }
        }
    }
    // whereOrInsert_done:
    p_set.a[p].prereq = prereq;
    p_set.a[p].r_run = r_run;
    if p_set.a[p].n_out > n_out {
        p_set.a[p].n_out = n_out;
    }
    1
}

/// Retorna a máscara de bits para o número de cursor dado. Retorna 0 se
/// i_cursor não está no conjunto.
pub fn where_get_mask(p_mask_set: &WhereMaskSet, i_cursor: i32) -> Bitmask {
    assert!((p_mask_set.n as usize) <= std::mem::size_of::<Bitmask>() * 8);
    assert!(p_mask_set.n > 0 || p_mask_set.ix[0] < 0);
    assert!(i_cursor >= -1);
    if p_mask_set.ix[0] == i_cursor {
        return 1;
    }
    for i in 1..(p_mask_set.n as usize) {
        if p_mask_set.ix[i] == i_cursor {
            return maskbit(i as u32);
        }
    }
    0
}

/// Aloca memória que é automaticamente liberada quando p_w_info é liberado.
///
/// No C o bloco entra na lista `pMemToFree` do WhereInfo e é liberado junto com ele.
/// Aqui o `Vec` devolvido pertence a quem chamou e some no `Drop`, então a lista
/// `p_mem_to_free` não é usada. O C devolve NULL se a alocação falha; em Rust a
/// alocação não falha, então o resultado é sempre `Some`.
pub fn where_malloc(_p_w_info: &WhereInfoRef, n_byte: u64) -> Option<Vec<u8>> {
    Some(vec![0u8; n_byte as usize])
}

/// Realoca um bloco obtido de where_malloc(). O tamanho do bloco antigo
/// (`pOldBlk->sz` do C) é o seu `len()`, e o novo copia todos esses bytes.
pub fn where_realloc(
    p_w_info: &WhereInfoRef,
    p_old: Option<&[u8]>,
    n_byte: u64,
) -> Option<Vec<u8>> {
    let mut p_new = where_malloc(p_w_info, n_byte);
    if let (Some(new), Some(old)) = (p_new.as_mut(), p_old) {
        assert!((old.len() as u64) < n_byte);
        new[..old.len()].copy_from_slice(old);
    }
    p_new
}

/// Cria uma nova máscara para o cursor i_cursor.
///
/// Há um cursor por tabela na cláusula FROM. O número de tabelas
/// na cláusula FROM é limitado por um teste no início da
/// rotina sqlite3WhereBegin(). Então sabemos que o array
/// p_mask_set.ix[] nunca vai transbordar.
fn create_mask(p_mask_set: &mut WhereMaskSet, i_cursor: i32) {
    assert!((p_mask_set.n as usize) < p_mask_set.ix.len());
    p_mask_set.ix[p_mask_set.n as usize] = i_cursor;
    p_mask_set.n += 1;
}

/// Se o ramo direito da expressão é um TK_COLUMN, então retorna uma
/// referência ao ramo direito. Caso contrário, retorna None.
fn where_right_subexpr_is_column(p: &Expr) -> Option<&Expr> {
    let p = expr_skip_collate_and_likely(p.p_right.as_deref());
    match p {
        Some(e) if e.op == TK_COLUMN && !expr_has_property(e, EP_FIXEDCOL) => Some(e),
        _ => None,
    }
}

/// O termo p_term é garantido ser um termo WO_IN. Pode ser um termo componente
/// de uma expressão IN vetorial da forma "(x, y, ...) IN (SELECT ...)".
/// Esta função verifica se o termo é compatível com uma coluna de índice
/// com afinidade idxaff (um dos valores SQLITE_AFF_XYZ). Se for, retorna o
/// nome da sequência de colação (ex: "BINARY" ou "NOCASE") usada pela
/// comparação em p_term. Se não é compatível com afinidade idxaff,
/// None é retornado.
fn index_in_affinity_ok(p_parse: &ParseRef, p_term: &WhereTerm, idxaff: u8) -> Option<Vec<u8>> {
    let p_x_rc = p_term.p_expr.clone().unwrap();
    let p_x_ref = p_x_rc.borrow();
    let mut inexpr = Expr::default();
    let mut use_inexpr = false;

    assert!((p_term.e_operator & WO_IN) != 0);

    if expr_is_vector(p_x_ref.p_left.as_deref()) {
        let i_field = (where_term_i_field(p_term) - 1) as usize;
        inexpr.flags = 0;
        inexpr.op = TK_EQ;
        // O C aponta inexpr para os nós originais; aqui os dois lados são copiados,
        // o que basta porque inexpr só é lido.
        inexpr.p_left = p_x_ref
            .p_left
            .as_ref()
            .unwrap()
            .x
            .p_list
            .as_ref()
            .unwrap()
            .a[i_field]
            .p_expr
            .clone();
        assert!(expr_use_x_select(&p_x_ref));
        inexpr.p_right = p_x_ref
            .x
            .p_select
            .as_ref()
            .unwrap()
            .p_e_list
            .as_ref()
            .unwrap()
            .a[i_field]
            .p_expr
            .clone();
        use_inexpr = true;
    }
    let p_x: &Expr = if use_inexpr { &inexpr } else { &p_x_ref };

    if index_affinity_ok(p_x, idxaff) {
        let p_ret = expr_compare_coll_seq(p_parse, p_x);
        return Some(match p_ret {
            Some(c) => c.z_name.clone(),
            None => STR_BINARY.to_vec(),
        });
    }
    None
}

/// Corpo do teste de um termo dentro de where_scan_next(): devolve verdadeiro se
/// `p_term` casa com os critérios do scanner. Cada `continue` do laço do C é um
/// `return false` aqui. Pode acrescentar equivalentes a `p_scan`, como o C.
fn where_scan_term_ok(
    p_scan: &mut WhereScan,
    p_wc: &WhereClauseRef,
    p_term: &WhereTerm,
    i_cur: i32,
    i_column: i32,
) -> bool {
    assert!((p_term.e_operator & (WO_OR | WO_AND)) == 0 || p_term.left_cursor < 0);
    if p_term.left_cursor != i_cur || where_term_left_column(p_term) != i_column {
        return false;
    }
    let p_expr_rc = p_term.p_expr.clone().unwrap();
    let p_expr = p_expr_rc.borrow();
    if i_column == XN_EXPR {
        let p_idx_expr = p_scan.p_idx_expr.as_deref().unwrap();
        if expr_compare_skip(p_expr.p_left.as_deref().unwrap(), p_idx_expr, i_cur) != 0 {
            return false;
        }
    }
    if p_scan.i_equiv > 1 && expr_has_property(&p_expr, EP_OUTERON) {
        return false;
    }
    if (p_term.e_operator & WO_EQUIV) != 0 && (p_scan.n_equiv as usize) < p_scan.ai_cur.len() {
        if let Some(p_x) = where_right_subexpr_is_column(&p_expr) {
            let mut j = 0usize;
            while j < p_scan.n_equiv as usize {
                if p_scan.ai_cur[j] == p_x.i_table && (p_scan.ai_column[j] as i32) == p_x.i_column {
                    break;
                }
                j += 1;
            }
            if j == p_scan.n_equiv as usize {
                p_scan.ai_cur[j] = p_x.i_table;
                p_scan.ai_column[j] = p_x.i_column as i16;
                p_scan.n_equiv += 1;
            }
        }
    }
    if ((p_term.e_operator as u32) & p_scan.op_mask) == 0 {
        return false;
    }
    // Verifica se a afinidade e a sequência de colação casam
    if p_scan.z_coll_name.is_some() && (p_term.e_operator & WO_ISNULL) == 0 {
        let p_parse: ParseRef = {
            let wc = p_wc.borrow();
            let w_info = wc.p_w_info.upgrade().unwrap();
            let p = w_info.borrow().p_parse.clone();
            p
        };
        let z_coll_name: Vec<u8>;
        if (p_term.e_operator & WO_IN) != 0 {
            match index_in_affinity_ok(&p_parse, p_term, p_scan.idxaff) {
                Some(z) => z_coll_name = z,
                None => return false,
            }
        } else {
            if !index_affinity_ok(&p_expr, p_scan.idxaff) {
                return false;
            }
            assert!(p_expr.p_left.is_some());
            z_coll_name = match expr_compare_coll_seq(&p_parse, &p_expr) {
                Some(c) => c.z_name.clone(),
                None => STR_BINARY.to_vec(),
            };
        }
        if str_i_cmp(&z_coll_name, p_scan.z_coll_name.as_ref().unwrap()) != 0 {
            return false;
        }
    }
    if (p_term.e_operator & (WO_EQ | WO_IS)) != 0 {
        if let Some(p_x) = p_expr.p_right.as_deref() {
            if p_x.op == TK_COLUMN
                && p_x.i_table == p_scan.ai_cur[0]
                && p_x.i_column == (p_scan.ai_column[0] as i32)
            {
                return false;
            }
        }
    }
    true
}

/// Avança para o próximo WhereTerm que corresponde aos critérios
/// estabelecidos quando o objeto p_scan foi inicializado por
/// where_scan_init(). Retorna None se não houver mais WhereTerm
/// correspondentes.
fn where_scan_next(p_scan: &mut WhereScan) -> Option<WhereTermRef> {
    let mut k: usize = p_scan.k as usize; // Onde começar a varredura
    let mut p_wc: Option<WhereClauseRef> = p_scan.p_wc.clone();

    assert!(p_scan.i_equiv <= p_scan.n_equiv);
    loop {
        let i_column: i32 = p_scan.ai_column[(p_scan.i_equiv - 1) as usize] as i32;
        let i_cur: i32 = p_scan.ai_cur[(p_scan.i_equiv - 1) as usize];
        assert!(p_wc.is_some());
        assert!(i_cur >= 0);
        loop {
            let wc_ref: WhereClauseRef = p_wc.take().unwrap();
            let n_term = wc_ref.borrow().n_term as usize;
            while k < n_term {
                let p_term_ref: WhereTermRef = wc_ref.borrow().a[k].clone();
                let hit = {
                    let p_term = p_term_ref.borrow();
                    where_scan_term_ok(p_scan, &wc_ref, &p_term, i_cur, i_column)
                };
                if hit {
                    p_scan.p_wc = Some(wc_ref.clone());
                    p_scan.k = (k + 1) as i32;
                    return Some(p_term_ref);
                }
                k += 1;
            }
            p_wc = wc_ref.borrow().p_outer.as_ref().and_then(|w| w.upgrade());
            k = 0;
            if p_wc.is_none() {
                break;
            }
        }
        if p_scan.i_equiv >= p_scan.n_equiv {
            break;
        }
        p_wc = p_scan.p_orig_wc.clone();
        k = 0;
        p_scan.i_equiv += 1;
    }
    None
}


// ---- part_001.rs ----

// As funções `whereTraceIndexInfoInputs` e `whereTraceIndexInfoOutputs` só existem sob
// WHERETRACE_ENABLED (SQLITE_DEBUG ou SQLITE_TEST); no Debian 13 viram macros vazias e não
// são traduzidas. O ramo SQLITE_DEBUG de `translateColumnToCopy` e o ramo
// SQLITE_ALLOW_ROWID_IN_VIEW também somem.

/// Este é o whereScanInit() para o caso de um índice sobre uma expressão.
/// É fatorado numa sub-rotina separada de cauda recursiva para que a rotina normal
/// where_scan_init(), que é muito executada, não precise empilhar registradores
/// como parte do seu prólogo.
fn where_scan_init_index_expr(p_scan: &mut WhereScan) -> Option<WhereTermRef> {
    p_scan.idxaff = expr_affinity(p_scan.p_idx_expr.as_deref().unwrap());
    where_scan_next(p_scan)
}

/// Inicializa um objeto scanner de cláusula WHERE. Retorna o primeiro
/// termo que casa. Retorna None se não houver correspondências.
///
/// O scanner vai varrer a cláusula WHERE p_wc. Procurará por termos da forma "X <op> <expr>"
/// onde X é a coluna i_column da tabela i_cur. Ou, se p_idx não for nulo, X é a
/// coluna i_column do índice p_idx. p_idx deve ser um dos índices da tabela i_cur.
///
/// O <op> deve ser um dos operadores descritos por op_mask.
///
/// Se a busca for por X e a cláusula WHERE contiver termos da forma X=Y, então esta rotina
/// também poderá retornar termos da forma "Y <op> <expr>". O número de níveis de
/// transitividade é limitado, mas é suficiente para tratar a maioria das instruções SQL
/// que ocorrem comumente.
///
/// Se X não for a INTEGER PRIMARY KEY, então X deve ser compatível com o índice p_idx.
fn where_scan_init(
    p_scan: &mut WhereScan,
    p_wc: &WhereClauseRef,
    i_cur: i32,
    i_column: i32,
    op_mask: u32,
    p_idx: Option<&Index>,
) -> Option<WhereTermRef> {
    p_scan.p_orig_wc = Some(p_wc.clone());
    p_scan.p_wc = Some(p_wc.clone());
    p_scan.p_idx_expr = None;
    p_scan.idxaff = 0;
    p_scan.z_coll_name = None;
    p_scan.op_mask = op_mask;
    p_scan.k = 0;
    p_scan.ai_cur[0] = i_cur;
    p_scan.n_equiv = 1;
    p_scan.i_equiv = 1;
    let mut i_column = i_column;
    if let Some(idx) = p_idx {
        let j = i_column as usize;
        i_column = idx.ai_column[j] as i32;
        let mut index_expr = false;
        {
            let p_table = idx.p_table.upgrade().unwrap();
            let tab = p_table.borrow();
            if i_column == tab.i_p_key as i32 {
                i_column = XN_ROWID;
            } else if i_column >= 0 {
                p_scan.idxaff = tab.a_col[i_column as usize].affinity;
                p_scan.z_coll_name = Some(idx.az_coll[j].clone());
            } else if i_column == XN_EXPR {
                p_scan.p_idx_expr = idx.a_col_expr.as_ref().unwrap().a[j].p_expr.clone();
                p_scan.z_coll_name = Some(idx.az_coll[j].clone());
                p_scan.ai_column[0] = XN_EXPR as i16;
                index_expr = true;
            }
        }
        if index_expr {
            return where_scan_init_index_expr(p_scan);
        }
    } else if i_column == XN_EXPR {
        return None;
    }
    p_scan.ai_column[0] = i_column as i16;
    where_scan_next(p_scan)
}

/// Procura por um termo na cláusula WHERE que tenha a forma "X <op> <expr>"
/// onde X é uma referência à coluna i_column da tabela i_cur ou do índice p_idx
/// se p_idx não for nulo, e <op> é um dos códigos de operador WO_xx especificados pelo
/// parâmetro op. Retorna o termo. Retorna None se não for encontrado.
///
/// Se p_idx não for nulo, então deve ser um dos índices da tabela i_cur.
/// Procura por termos que correspondam à coluna i_column-ésima de p_idx
/// em vez da coluna i_column-ésima da tabela i_cur.
///
/// O termo retornado pode ser Y=<expr> se houver outra restrição na cláusula WHERE
/// que especifique que X=Y. Quaisquer restrições desse tipo serão identificadas pelo
/// bit WO_EQUIV no campo e_operator de WhereTerm. Os vetores ai_cur[] e ai_column[]
/// contêm X e todos os seus equivalentes. Há 11 espaços em ai_cur[] e ai_column[],
/// o que significa que podemos procurar por X mais até 10 outros valores equivalentes.
/// Assim, uma busca por X retornará <expr> se X=A1 e A1=A2 e A2=A3 e ... e A9=A10 e
/// A10=<expr>.
///
/// Se houver múltiplos termos na cláusula WHERE da forma "X <op> <expr>",
/// então tenta-se obter o que não tem dependências em <expr>, em outras palavras, onde
/// <expr> é uma expressão constante de algum tipo. Só retorna entradas da forma
/// "X <op> Y", onde Y é uma coluna de outra tabela, se não existirem termos da forma
/// "X <op> <const-expr>". Se não existem termos com RHS constante,
/// tenta retornar um termo que não use WO_EQUIV.
pub fn where_find_term(
    p_wc: &WhereClauseRef,
    i_cur: i32,
    i_column: i32,
    not_ready: Bitmask,
    op: u32,
    p_idx: Option<&Index>,
) -> Option<WhereTermRef> {
    let mut p_result: Option<WhereTermRef> = None;
    let mut scan = WhereScan::default();

    let mut p = where_scan_init(&mut scan, p_wc, i_cur, i_column, op, p_idx);
    let op = op & ((WO_EQ | WO_IS) as u32);
    while let Some(p_term_ref) = p {
        {
            let t = p_term_ref.borrow();
            if (t.prereq_right & not_ready) == 0 {
                if t.prereq_right == 0 && ((t.e_operator as u32) & op) != 0 {
                    drop(t);
                    return Some(p_term_ref);
                }
                if p_result.is_none() {
                    p_result = Some(p_term_ref.clone());
                }
            }
        }
        p = where_scan_next(&mut scan);
    }
    p_result
}

/// Esta função procura em p_list por uma entrada que corresponda à coluna i_col-ésima
/// do índice p_idx.
///
/// Se tal expressão for encontrada, seu índice em p_list.a[] é retornado. Se
/// nenhuma expressão for encontrada, -1 é retornado.
fn find_index_col(
    p_parse: &ParseRef,
    p_list: &ExprList,
    i_base: i32,
    p_idx: &Index,
    i_col: usize,
) -> i32 {
    let z_coll = &p_idx.az_coll[i_col];

    for i in 0..(p_list.n_expr as usize) {
        let p_item_expr = p_list.a[i].p_expr.as_deref();
        if let Some(p) = expr_skip_collate_and_likely(p_item_expr) {
            if (p.op == TK_COLUMN || p.op == TK_AGG_COLUMN)
                && p.i_column == (p_idx.ai_column[i_col] as i32)
                && p.i_table == i_base
            {
                let p_coll = expr_nn_coll_seq(p_parse, p_item_expr.unwrap());
                if str_i_cmp(&p_coll.z_name, z_coll) == 0 {
                    return i as i32;
                }
            }
        }
    }

    -1
}

/// Retorna VERDADEIRO se a coluna i_col-ésima do índice p_idx é NOT NULL.
fn index_column_not_null(p_idx: &Index, i_col: usize) -> i32 {
    assert!(i_col < p_idx.n_column as usize);
    let j = p_idx.ai_column[i_col] as i32;
    if j >= 0 {
        let p_table = p_idx.p_table.upgrade().unwrap();
        let not_null = p_table.borrow().a_col[j as usize].not_null as i32;
        not_null
    } else if j == -1 {
        1
    } else {
        assert!(j == -2);
        0 // Assume que uma expressão indexada sempre pode produzir NULL
    }
}

/// Retorna verdadeiro se a lista de expressões DISTINCT passada como o terceiro argumento
/// é redundante.
///
/// Uma lista DISTINCT é redundante se qualquer subconjunto das colunas na lista DISTINCT
/// é coletivamente único e individualmente não nulo.
fn is_distinct_redundant(
    p_parse: &ParseRef,
    p_tab_list: &SrcList,
    p_wc: &WhereClauseRef,
    p_distinct: &ExprList,
) -> i32 {
    // Se há mais de uma tabela ou sub-select na cláusula FROM desta consulta,
    // não será possível mostrar que a cláusula DISTINCT é redundante.
    if p_tab_list.n_src != 1 {
        return 0;
    }
    let i_base = p_tab_list.a[0].i_cursor;
    let p_tab: TableRef = p_tab_list.a[0].p_tab.clone().unwrap();

    // Se qualquer uma das expressões é uma coluna IPK da tabela i_base, retorna
    // verdadeiro. Nota: a parte (p->iTable==iBase) deste teste pode ser falsa se o
    // SELECT atual é uma subconsulta correlacionada.
    for i in 0..(p_distinct.n_expr as usize) {
        let p = match expr_skip_collate_and_likely(p_distinct.a[i].p_expr.as_deref()) {
            Some(p) => p,
            None => continue,
        };
        if p.op != TK_COLUMN && p.op != TK_AGG_COLUMN {
            continue;
        }
        if p.i_table == i_base && p.i_column < 0 {
            return 1;
        }
    }

    // Percorre todos os índices da tabela, verificando se cada um torna o qualificador
    // DISTINCT redundante. Torna se:
    //
    //   1. O índice é ele próprio UNIQUE, e
    //
    //   2. Todas as colunas do índice fazem parte da lista p_distinct, ou então a cláusula
    //      WHERE contém um termo da forma "col=X", onde X é um valor constante. As
    //      sequências de colação da comparação e das expressões da lista de seleção devem
    //      casar com as do índice.
    //
    //   3. Todas as colunas do índice para as quais a cláusula WHERE não contém um termo
    //      "col=X" estão sujeitas a uma restrição NOT NULL.
    let mut p_idx_opt: Option<IndexRef> = p_tab.borrow().p_index.clone();
    while let Some(p_idx_ref) = p_idx_opt {
        let next;
        {
            let p_idx = p_idx_ref.borrow();
            next = p_idx.p_next.clone();
            if is_unique_index(&p_idx) && p_idx.p_part_idx_where.is_none() {
                let n_key_col = p_idx.n_key_col as usize;
                let mut i = 0usize;
                while i < n_key_col {
                    if where_find_term(p_wc, i_base, i as i32, !(0 as Bitmask), WO_EQ, Some(&p_idx))
                        .is_none()
                    {
                        if find_index_col(p_parse, p_distinct, i_base, &p_idx, i) < 0 {
                            break;
                        }
                        if index_column_not_null(&p_idx, i) == 0 {
                            break;
                        }
                    }
                    i += 1;
                }
                if i == n_key_col {
                    // Este índice implica que o qualificador DISTINCT é redundante.
                    return 1;
                }
            }
        }
        p_idx_opt = next;
    }

    0
}

/// Estima o logaritmo do valor de entrada na base 2.
fn est_log(n: LogEst) -> LogEst {
    if n <= 10 {
        0
    } else {
        log_est(n as u64) - 33
    }
}

/// Converte opcodes OP_Column para OP_Copy no código gerado previamente.
///
/// Esta rotina percorre o código VDBE gerado e traduz opcodes OP_Column para OP_Copy
/// quando a tabela está sendo acessada via corrotina em vez de via busca em tabela.
///
/// Se i_autoidx_cur não for zero, então qualquer instrução OP_Rowid no cursor i_tab_cur
/// é transformada em opcode OP_Sequence para o cursor i_autoidx_cur, a fim de gerar rowids
/// únicos para o índice automático sendo gerado.
fn translate_column_to_copy(
    p_parse: &Parse,
    i_start: i32,
    i_tab_cur: i32,
    i_register: i32,
    i_autoidx_cur: i32,
) {
    let v_ref: VdbeRef = p_parse.p_vdbe.clone().unwrap();
    let mut v = v_ref.borrow_mut();
    let i_end = vdbe_current_addr(&v);
    if p_parse.db.upgrade().unwrap().borrow().malloc_failed != 0 {
        return;
    }
    for i in i_start..i_end {
        let p_op = &mut v.a_op[i as usize];
        if p_op.p1 != i_tab_cur {
            continue;
        }
        if p_op.opcode == OP_COLUMN {
            p_op.opcode = OP_COPY;
            p_op.p1 = p_op.p2 + i_register;
            p_op.p2 = p_op.p3;
            p_op.p3 = 0;
            p_op.p5 = 2; // Faz o flag MEM_Subtype ser limpo
        } else if p_op.opcode == OP_ROWID {
            p_op.opcode = OP_SEQUENCE;
            p_op.p1 = i_autoidx_cur;
        }
    }
}


// ---- part_002.rs ----

// O Debian 13 não liga SQLITE_ENABLE_STMT_SCANSTATUS: `explainAutomaticIndex` é macro vazia,
// `addrExp` não existe e `sqlite3VdbeScanStatusCounters`/`sqlite3VdbeScanStatusRange` somem.
// `testcase`, `VdbeCoverage` e `VdbeComment` também não geram código.

/// Sabemos que p_src é um operando de um join externo. Retorna verdadeiro se
/// p_term é uma restrição compatível com esse join.
///
/// p_term deve ser EP_OUTERON se p_src é o operando direito de um
/// join externo. p_term pode ser EP_OUTERON ou EP_INNERON se p_src
/// é o operando esquerdo de um RIGHT JOIN.
///
/// Veja https://sqlite.org/forum/forumpost/206d99a16dd9212f
/// para um exemplo de restrições de cláusula WHERE que não podem ser usadas na
/// tabela direita de um RIGHT JOIN, porque a restrição implica uma
/// condição de não nulo na tabela esquerda do RIGHT JOIN.
fn constraint_compatible_with_outer_join(p_term: &WhereTerm, p_src: &SrcItem) -> bool {
    assert!((p_src.fg.jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0); // Pelo chamador
    let p_expr = p_term.p_expr.as_ref().unwrap().borrow();
    if !expr_has_property(&p_expr, EP_OUTERON | EP_INNERON) || p_expr.w.i_join != p_src.i_cursor {
        return false;
    }
    if (p_src.fg.jointype & (JT_LEFT | JT_RIGHT)) != 0 && expr_has_property(&p_expr, EP_INNERON) {
        return false;
    }
    true
}

/// Retorna verdadeiro se o termo p_term da cláusula WHERE é de uma forma que
/// poderia ser usada com um índice para acessar p_src, supondo que existisse
/// um índice apropriado.
fn term_can_drive_index(p_term: &WhereTerm, p_src: &SrcItem, not_ready: Bitmask) -> bool {
    if p_term.left_cursor != p_src.i_cursor {
        return false;
    }
    if (p_term.e_operator & (WO_EQ | WO_IS)) == 0 {
        return false;
    }
    assert!((p_src.fg.jointype & JT_RIGHT) == 0);
    if (p_src.fg.jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0
        && !constraint_compatible_with_outer_join(p_term, p_src)
    {
        return false; // Veja https://sqlite.org/forum/forumpost/51e6959f61
    }
    if (p_term.prereq_right & not_ready) != 0 {
        return false;
    }
    assert!((p_term.e_operator & (WO_OR | WO_AND)) == 0);
    let left_column = match &p_term.u {
        WhereTermUnion::X { left_column, .. } => *left_column,
        _ => unreachable!(),
    };
    if left_column < 0 {
        return false;
    }
    let aff = p_src.p_tab.as_ref().unwrap().borrow().a_col[left_column as usize].affinity;
    if !index_affinity_ok(&p_term.p_expr.as_ref().unwrap().borrow(), aff) {
        return false;
    }
    true
}

/// Gera código para construir o objeto Index de um índice automático
/// e para preparar o objeto WhereLevel p_level de modo que o gerador de código
/// use o índice automático.
fn construct_automatic_index(
    p_parse: &ParseRef,
    p_wc: &WhereClause,
    not_ready: Bitmask,
    p_level: &mut WhereLevel,
) {
    let mut n_key_col: i32; // Número de colunas no índice construído
    let mut id_x_cols: Bitmask; // Mapa das colunas usadas na indexação
    let extra_cols: Bitmask; // Mapa das colunas adicionais
    let mut sent_warning: u8 = 0; // Verdadeiro se um aviso foi emitido
    let mut use_bloom_filter: u8 = 0; // Verdadeiro para também adicionar um filtro de Bloom
    let mut p_partial: Option<Box<Expr>> = None; // Expressão do índice parcial
    let mut i_continue: i32 = 0; // Salta aqui para pular linhas excluídas
    let mut addr_counter: i32 = 0; // Endereço onde o contador inteiro é inicializado

    // Gera código para pular a criação e a inicialização do índice transitório
    // na 2a iteração e nas seguintes do laço.
    let v: VdbeRef = p_parse.borrow().p_vdbe.as_ref().unwrap().clone();
    let addr_init = vdbe_add_op0(&mut v.borrow_mut(), OP_ONCE as i32);

    // Conta o número de colunas que serão adicionadas ao índice
    // e usadas para casar restrições da cláusula WHERE.
    n_key_col = 0;
    let p_tab_list: SrcListRef = p_wc.p_w_info.upgrade().unwrap().borrow().p_tab_list.clone();
    let tab_list = p_tab_list.borrow();
    let p_src: &SrcItem = &tab_list.a[p_level.i_from as usize];
    let p_table: TableRef = p_src.p_tab.as_ref().unwrap().clone();
    let p_w_end = p_wc.n_term as usize;
    let p_loop: WhereLoopRef = p_level.p_w_loop.as_ref().unwrap().clone();
    id_x_cols = 0;
    let db: Rc<RefCell<Sqlite3>> = p_parse.borrow().db.upgrade().unwrap();

    'end_auto_index_create: {
        for i_term in 0..p_w_end {
            let p_term_ref = p_wc.a[i_term].clone();
            let p_term = p_term_ref.borrow();
            let p_expr = p_term.p_expr.as_ref().unwrap().clone();
            // Torna o índice automático um índice parcial se há termos na cláusula
            // WHERE (ou na cláusula ON de um LEFT join) que restringem quais linhas
            // da tabela alvo (p_src) podem ser usadas.
            if (p_term.wt_flags & TERM_VIRTUAL) == 0
                && expr_is_single_table_constraint(&p_expr.borrow(), &tab_list, p_level.i_from as i32, 0)
            {
                let p_dup = expr_dup(&db.borrow(), &p_expr.borrow(), 0);
                p_partial = expr_and(&mut p_parse.borrow_mut(), p_partial.take(), p_dup);
            }
            if term_can_drive_index(&p_term, p_src, not_ready) {
                assert!((p_term.e_operator & (WO_OR | WO_AND)) == 0);
                let i_col = match &p_term.u {
                    WhereTermUnion::X { left_column, .. } => *left_column,
                    _ => unreachable!(),
                };
                let c_mask: Bitmask = if i_col >= BMS { maskbit((BMS - 1) as u32) } else { maskbit(i_col as u32) };
                if sent_warning == 0 {
                    let mut z_msg: Vec<u8> = b"automatic index on ".to_vec();
                    z_msg.extend_from_slice(&p_table.borrow().z_name);
                    z_msg.push(b'(');
                    z_msg.extend_from_slice(&p_table.borrow().a_col[i_col as usize].z_cn_name);
                    z_msg.push(b')');
                    api::log(SQLITE_WARNING_AUTOINDEX, &z_msg);
                    sent_warning = 1;
                }
                if (id_x_cols & c_mask) == 0 {
                    if where_loop_resize(&db.borrow(), &mut p_loop.borrow_mut(), (n_key_col + 1) as u16) != 0 {
                        break 'end_auto_index_create;
                    }
                    p_loop.borrow_mut().a_l_term[n_key_col as usize] = Some(p_term_ref.clone());
                    n_key_col += 1;
                    id_x_cols |= c_mask;
                }
            }
        }
        assert!(n_key_col > 0 || db.borrow().malloc_failed != 0);
        {
            let mut lp = p_loop.borrow_mut();
            if let WhereLoopUnion::Btree { n_eq, .. } = &mut lp.u {
                *n_eq = n_key_col as u16;
            }
            lp.n_l_term = n_key_col as u16;
            lp.ws_flags = WHERE_COLUMN_EQ | WHERE_IDX_ONLY | WHERE_INDEXED | WHERE_AUTO_INDEX;
        }

        // Conta o número de colunas adicionais necessárias para criar um
        // índice de cobertura. Um "índice de cobertura" é um índice que contém todas
        // as colunas necessárias à consulta. Com um índice de cobertura, a tabela
        // original nunca precisa ser acessada. Os índices automáticos precisam ser
        // de cobertura porque o índice não será atualizado se a tabela original
        // mudar, e o índice e a tabela não podem ser usados juntos se saírem de
        // sincronia.
        if is_view(&p_table.borrow()) {
            extra_cols = ALLBITS & !id_x_cols;
        } else {
            extra_cols = p_src.col_used & (!id_x_cols | maskbit((BMS - 1) as u32));
        }
        let n_col = p_table.borrow().n_col as i32;
        let mx_bit_col: i32 = std::cmp::min(BMS - 1, n_col);
        for i in 0..mx_bit_col {
            if (extra_cols & maskbit(i as u32)) != 0 {
                n_key_col += 1;
            }
        }
        if (p_src.col_used & maskbit((BMS - 1) as u32)) != 0 {
            n_key_col += n_col - BMS + 1;
        }

        // Constrói o objeto Index para descrever este índice.
        let (p_idx_box, _z_not_used) = allocate_index_object(Some(&db.borrow()), (n_key_col + 1) as i16, 0);
        if p_idx_box.is_none() {
            break 'end_auto_index_create;
        }
        let p_idx: IndexRef = Rc::new(RefCell::new(*p_idx_box.unwrap()));
        {
            let mut idx = p_idx.borrow_mut();
            idx.z_name = b"auto-index".to_vec();
            idx.p_table = Rc::downgrade(&p_table);
        }
        if let WhereLoopUnion::Btree { p_index, .. } = &mut p_loop.borrow_mut().u {
            *p_index = Some(p_idx.clone());
        }
        let mut n: i32 = 0;
        id_x_cols = 0;
        for i_term in 0..p_w_end {
            let p_term_ref = p_wc.a[i_term].clone();
            let p_term = p_term_ref.borrow();
            if term_can_drive_index(&p_term, p_src, not_ready) {
                assert!((p_term.e_operator & (WO_OR | WO_AND)) == 0);
                let i_col = match &p_term.u {
                    WhereTermUnion::X { left_column, .. } => *left_column,
                    _ => unreachable!(),
                };
                let c_mask: Bitmask = if i_col >= BMS { maskbit((BMS - 1) as u32) } else { maskbit(i_col as u32) };
                if (id_x_cols & c_mask) == 0 {
                    let p_x = p_term.p_expr.as_ref().unwrap().borrow();
                    id_x_cols |= c_mask;
                    let mut idx = p_idx.borrow_mut();
                    idx.ai_column[n as usize] = i_col as i16;
                    let p_coll = expr_compare_coll_seq(p_parse, &p_x);
                    assert!(p_coll.is_some() || p_parse.borrow().n_err > 0); // TH3 collate01.800
                    idx.az_coll[n as usize] = match p_coll {
                        Some(c) => c.borrow().z_name.clone(),
                        None => b"BINARY".to_vec(),
                    };
                    n += 1;
                    if p_x.p_left.is_some() && expr_affinity(p_x.p_left.as_ref().unwrap()) != SQLITE_AFF_TEXT {
                        // TUNING: só usa um filtro de Bloom num índice automático
                        // se uma ou mais colunas-chave têm a capacidade de guardar
                        // valores numéricos, já que todas as strings têm o mesmo hash na
                        // implementação do filtro de Bloom e, portanto, um filtro de Bloom
                        // numa coluna de texto em geral não ajuda.
                        use_bloom_filter = 1;
                    }
                }
            }
        }
        {
            let lp = p_loop.borrow();
            if let WhereLoopUnion::Btree { n_eq, .. } = &lp.u {
                assert!(n as u32 == *n_eq as u32);
            }
        }

        // Adiciona as colunas adicionais necessárias para fazer do índice
        // automático um índice de cobertura.
        for i in 0..mx_bit_col {
            if (extra_cols & maskbit(i as u32)) != 0 {
                let mut idx = p_idx.borrow_mut();
                idx.ai_column[n as usize] = i as i16;
                idx.az_coll[n as usize] = b"BINARY".to_vec();
                n += 1;
            }
        }
        if (p_src.col_used & maskbit((BMS - 1) as u32)) != 0 {
            for i in (BMS - 1)..n_col {
                let mut idx = p_idx.borrow_mut();
                idx.ai_column[n as usize] = i as i16;
                idx.az_coll[n as usize] = b"BINARY".to_vec();
                n += 1;
            }
        }
        assert!(n == n_key_col);
        {
            let mut idx = p_idx.borrow_mut();
            idx.ai_column[n as usize] = XN_ROWID;
            idx.az_coll[n as usize] = b"BINARY".to_vec();
        }

        // Cria o índice automático.
        assert!(p_level.i_idx_cur >= 0);
        {
            let mut pp = p_parse.borrow_mut();
            p_level.i_idx_cur = pp.n_tab;
            pp.n_tab += 1;
        }
        vdbe_add_op2(&mut v.borrow_mut(), OP_OPENAUTOINDEX as i32, p_level.i_idx_cur, n_key_col + 1);
        vdbe_set_p4_key_info(&mut p_parse.borrow_mut(), &p_idx);
        if optimization_enabled(&db.borrow(), SQLITE_BLOOMFILTER) && use_bloom_filter != 0 {
            let p_w_info = p_wc.p_w_info.upgrade().unwrap();
            where_explain_bloom_filter(&p_parse.borrow(), &p_w_info.borrow(), p_level);
            let reg_filter = {
                let mut pp = p_parse.borrow_mut();
                pp.n_mem += 1;
                pp.n_mem
            };
            p_level.reg_filter = reg_filter;
            vdbe_add_op2(&mut v.borrow_mut(), OP_BLOB as i32, 10000, p_level.reg_filter);
        }

        // Preenche o índice automático com conteúdo.
        assert!(std::ptr::eq(p_src, &tab_list.a[p_level.i_from as usize]));
        let addr_top: i32;
        if p_src.fg.via_coroutine != 0 {
            let reg_yield = p_src.reg_return;
            addr_counter = vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, 0);
            vdbe_add_op3(&mut v.borrow_mut(), OP_INITCOROUTINE as i32, reg_yield, 0, p_src.addr_fill_sub);
            addr_top = vdbe_add_op1(&mut v.borrow_mut(), OP_YIELD as i32, reg_yield);
        } else {
            addr_top = vdbe_add_op1(&mut v.borrow_mut(), OP_REWIND as i32, p_level.i_tab_cur);
        }
        if p_partial.is_some() {
            i_continue = vdbe_make_label(p_parse);
            expr_if_false(
                &mut p_parse.borrow_mut(),
                p_partial.as_ref().unwrap(),
                i_continue,
                SQLITE_JUMPIFNULL as i32,
            );
            p_loop.borrow_mut().ws_flags |= WHERE_PARTIALIDX;
        }
        let reg_record = get_temp_reg(&mut p_parse.borrow_mut());
        let mut pi_part_idx_label: i32 = 0;
        let reg_base = generate_index_key(
            &mut p_parse.borrow_mut(),
            &p_idx.borrow(),
            p_level.i_tab_cur,
            reg_record,
            0,
            &mut pi_part_idx_label,
            None,
            0,
        );
        if p_level.reg_filter != 0 {
            let n_eq = match &p_loop.borrow().u {
                WhereLoopUnion::Btree { n_eq, .. } => *n_eq as i32,
                _ => 0,
            };
            vdbe_add_op4_int(
                &mut v.borrow_mut(),
                OP_FILTERADD as i32,
                p_level.reg_filter,
                0,
                reg_base,
                n_eq,
            );
        }
        vdbe_add_op2(&mut v.borrow_mut(), OP_IDXINSERT as i32, p_level.i_idx_cur, reg_record);
        vdbe_change_p5(&mut v.borrow_mut(), OPFLAG_USESEEKRESULT as u16);
        if p_partial.is_some() {
            vdbe_resolve_label(&mut v.borrow_mut(), i_continue);
        }
        if p_src.fg.via_coroutine != 0 {
            vdbe_change_p2(&mut v.borrow_mut(), addr_counter, reg_base + n);
            assert!(p_level.i_idx_cur > 0);
            translate_column_to_copy(
                &p_parse.borrow(),
                addr_top,
                p_level.i_tab_cur,
                p_src.reg_result,
                p_level.i_idx_cur,
            );
            vdbe_goto(&mut v.borrow_mut(), addr_top);
            // `p_src` empresta de `tab_list`: o empréstimo termina aqui, antes de gravar.
            let i_from = p_level.i_from as usize;
            drop(tab_list);
            p_tab_list.borrow_mut().a[i_from].fg.via_coroutine = 0;
        } else {
            vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, p_level.i_tab_cur, addr_top + 1);
            vdbe_change_p5(&mut v.borrow_mut(), SQLITE_STMTSTATUS_AUTOINDEX as u16);
        }
        vdbe_jump_here(&mut v.borrow_mut(), addr_top);
        release_temp_reg(&mut p_parse.borrow_mut(), reg_record);

        // Salta aqui ao pular a inicialização.
        vdbe_jump_here(&mut v.borrow_mut(), addr_init);
    }
    // end_auto_index_create: sqlite3ExprDelete(db, pPartial), feito pelo Drop do Box.
    drop(p_partial);
}


// ---- part_003.rs ----

/// Gera bytecode que inicializa um filtro de Bloom apropriado para `i_level`.
///
/// Se há laços internos dentro do nível com o flag WHERE_BLOOMFILTER ligado, inicializa
/// um filtro de Bloom para eles também. Exceto que essa inicialização recursiva não é feita
/// se a otimização SQLITE_BloomPulldown estiver desligada.
///
/// Quando o filtro de Bloom é inicializado, o flag WHERE_BLOOMFILTER é limpo do laço, mas o
/// valor de `reg_filter` passa a ser o registro que implementa o filtro. Quando `reg_filter`
/// é positivo, `where_code_one_loop_start()` gera código que testa o filtro e pula a busca
/// seguinte na B-Tree se o filtro indicar que nenhuma linha casa.
///
/// Esta rotina só pode ser chamada se já foi determinado que o laço se beneficia de um
/// filtro de Bloom e o bit WHERE_BLOOMFILTER está ligado.
///
/// No C o nível vem também como ponteiro `pLevel`, que sempre é `&pWInfo->a[iLevel]`; aqui
/// ele é derivado de `i_level`, porque o `WhereInfo` fica atrás de um `RefCell`.
fn construct_bloom_filter(p_w_info: &WhereInfoRef, mut i_level: i32, not_ready: Bitmask) {
    let p_parse = p_w_info.borrow().p_parse.clone();
    let db = p_parse.borrow().db.upgrade().unwrap();
    let v = p_parse.borrow().p_vdbe.clone();
    let n_level = p_w_info.borrow().n_level as i32;

    // Cópias salvas de Parse.p_idx_epr e Parse.p_idx_part_expr
    let saved_p_idx_epr = p_parse.borrow_mut().p_idx_epr.take();
    let saved_p_idx_part_expr = p_parse.borrow_mut().p_idx_part_expr.take();

    let p_loop = p_w_info.borrow().a[i_level as usize].p_w_loop.clone();
    assert!(p_loop.is_some());
    let mut p_loop: WhereLoopRef = p_loop.unwrap();
    assert!(v.is_some());
    let v: VdbeRef = v.unwrap();
    assert!((p_loop.borrow().ws_flags & WHERE_BLOOMFILTER) != 0);
    assert!((p_loop.borrow().ws_flags & WHERE_IDX_ONLY) == 0);

    let addr_once = vdbe_add_op0(&mut v.borrow_mut(), OP_ONCE as i32);
    loop {
        {
            let pp = p_parse.borrow();
            let wi = p_w_info.borrow();
            where_explain_bloom_filter(&pp, &wi, &wi.a[i_level as usize]);
        }
        let addr_cont = vdbe_make_label(&p_parse);
        let i_cur = p_w_info.borrow().a[i_level as usize].i_tab_cur;
        let reg_filter = {
            let mut pp = p_parse.borrow_mut();
            pp.n_mem += 1;
            pp.n_mem
        };
        p_w_info.borrow_mut().a[i_level as usize].reg_filter = reg_filter;

        // O filtro de Bloom é um Blob guardado num registro. Inicializa-o com um blob
        // preenchido com zeros de pelo menos 80K bits, ou mais se o tamanho estimado da
        // tabela for maior. Poderíamos medir o tamanho da tabela em tempo de execução com
        // OP_Count com P3==1 e usar esse valor para inicializar o blob, mas isso complicaria
        // os testes. Baseando o tamanho do blob no valor da tabela sqlite_stat1, os testes
        // ficam muito mais fáceis.
        let (p_tab_list, i_src) = {
            let wi = p_w_info.borrow();
            (wi.p_tab_list.clone(), wi.a[i_level as usize].i_from as usize)
        };
        let p_tab = p_tab_list.borrow().a[i_src].p_tab.clone();
        assert!(p_tab.is_some());
        let p_tab: TableRef = p_tab.unwrap();
        let mut sz: u64 = log_est_to_int(p_tab.borrow().n_row_log_est);
        if sz < 10000 {
            sz = 10000;
        } else if sz > 10000000 {
            sz = 10000000;
        }
        vdbe_add_op2(&mut v.borrow_mut(), OP_BLOB as i32, sz as i32, reg_filter);

        let addr_top = vdbe_add_op1(&mut v.borrow_mut(), OP_REWIND as i32, i_cur);
        let n_term = p_w_info.borrow().s_wc.n_term;
        for k in 0..n_term {
            let p_term = p_w_info.borrow().s_wc.a[k as usize].clone();
            let (wt_flags, p_expr) = {
                let t = p_term.borrow();
                (t.wt_flags, t.p_expr.clone())
            };
            if (wt_flags & TERM_VIRTUAL) == 0
                && expr_is_single_table_constraint(
                    &p_expr.as_ref().unwrap().borrow(),
                    &p_tab_list.borrow(),
                    i_src as i32,
                    0,
                )
            {
                expr_if_false(
                    &mut p_parse.borrow_mut(),
                    &p_expr.as_ref().unwrap().borrow(),
                    addr_cont,
                    SQLITE_JUMPIFNULL as i32,
                );
            }
        }
        if (p_loop.borrow().ws_flags & WHERE_IPK) != 0 {
            let r1 = get_temp_reg(&mut p_parse.borrow_mut());
            vdbe_add_op2(&mut v.borrow_mut(), OP_ROWID as i32, i_cur, r1);
            vdbe_add_op4_int(&mut v.borrow_mut(), OP_FILTERADD as i32, reg_filter, 0, r1, 1);
            release_temp_reg(&mut p_parse.borrow_mut(), r1);
        } else {
            let (p_idx, n) = match &p_loop.borrow().u {
                WhereLoopUnion::Btree { p_index, n_eq, .. } => {
                    (p_index.clone().unwrap(), *n_eq as i32)
                }
                _ => unreachable!(),
            };
            let r1 = get_temp_range(&mut p_parse.borrow_mut(), n);
            for jj in 0..n {
                assert!(std::ptr::eq(
                    p_idx.borrow().p_table.as_ptr(),
                    std::rc::Rc::as_ptr(&p_tab)
                ));
                expr_code_load_index_column(&p_parse, &p_idx, i_cur, jj, r1 + jj);
            }
            vdbe_add_op4_int(&mut v.borrow_mut(), OP_FILTERADD as i32, reg_filter, 0, r1, n);
            release_temp_range(&mut p_parse.borrow_mut(), r1, n);
        }
        vdbe_resolve_label(&mut v.borrow_mut(), addr_cont);
        let i_tab_cur = p_w_info.borrow().a[i_level as usize].i_tab_cur;
        vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, i_tab_cur, addr_top + 1);
        vdbe_jump_here(&mut v.borrow_mut(), addr_top);
        p_loop.borrow_mut().ws_flags &= !WHERE_BLOOMFILTER;
        if optimization_disabled(&db.borrow(), SQLITE_BLOOMPULLDOWN) {
            break;
        }
        loop {
            i_level += 1;
            if !(i_level < n_level) {
                break;
            }
            let (jointype, p_candidate) = {
                let wi = p_w_info.borrow();
                let p_level = &wi.a[i_level as usize];
                (
                    p_tab_list.borrow().a[p_level.i_from as usize].fg.jointype,
                    p_level.p_w_loop.clone(),
                )
            };
            if (jointype & (JT_LEFT | JT_LTORJ)) != 0 {
                continue;
            }
            let p_candidate = match p_candidate {
                Some(l) => l,
                None => continue,
            };
            let (prereq, ws_flags) = {
                let l = p_candidate.borrow();
                (l.prereq, l.ws_flags)
            };
            if (prereq & not_ready) != 0 {
                continue;
            }
            if (ws_flags & (WHERE_BLOOMFILTER | WHERE_COLUMN_IN)) == WHERE_BLOOMFILTER {
                // Este é um candidato para o pull-down do filtro de Bloom (avaliação
                // antecipada). O teste que omite WHERE_COLUMN_IN é importante, pois não
                // conseguimos fazer avaliação antecipada de filtros de Bloom que usam o
                // operador IN.
                p_loop = p_candidate;
                break;
            }
        }
        if !(i_level < n_level) {
            break;
        }
    }
    vdbe_jump_here(&mut v.borrow_mut(), addr_once);
    p_parse.borrow_mut().p_idx_epr = saved_p_idx_epr;
    p_parse.borrow_mut().p_idx_part_expr = saved_p_idx_part_expr;
}

/// Aloca e preenche uma estrutura sqlite3_index_info. É responsabilidade de quem chama
/// liberar depois o par devolvido, passando-o a `free_index_info()`.
///
/// No C a Sqlite3IndexInfo, a HiddenIndexInfo e as tabelas de restrições vivem numa só
/// alocação; aqui são devolvidas as duas estruturas, com as tabelas dentro delas. Se a
/// alocação falhasse o C emitiria "out of memory" e devolveria NULL; em Rust a alocação
/// não falha, então esse ramo não existe.
fn allocate_index_info(
    p_w_info: &WhereInfoRef,
    p_wc: &WhereClauseRef,
    m_unusable: Bitmask,
    p_src: &SrcItem,
    pm_no_omit: &mut u16,
) -> Option<(Sqlite3IndexInfo, HiddenIndexInfo)> {
    let p_parse = p_w_info.borrow().p_parse.clone();
    let mut m_no_omit: u16 = 0;
    let mut e_distinct: i32 = 0;
    let p_order_by = p_w_info.borrow().p_order_by.clone();

    let p_tab = p_src.p_tab.clone();
    assert!(p_tab.is_some());
    let p_tab: TableRef = p_tab.unwrap();
    assert!(is_virtual(&p_tab.borrow()));

    // Encontra todas as restrições da cláusula WHERE que se referem a esta tabela virtual.
    // Marca cada termo com o flag TERM_OK. Define n_term como o número de termos achados.
    let mut n_term: i32 = 0;
    let n_wc_term = p_wc.borrow().n_term;
    for i in 0..n_wc_term {
        let p_term_ref = p_wc.borrow().a[i as usize].clone();
        let mut p_term = p_term_ref.borrow_mut();
        p_term.wt_flags &= !TERM_OK;
        if p_term.left_cursor != p_src.i_cursor {
            continue;
        }
        if (p_term.prereq_right & m_unusable) != 0 {
            continue;
        }
        assert!(is_power_of_two((p_term.e_operator & !WO_EQUIV) as usize));
        if (p_term.e_operator & !WO_EQUIV) == 0 {
            continue;
        }
        if (p_term.wt_flags & TERM_VNULL) != 0 {
            continue;
        }

        assert!((p_term.e_operator & (WO_OR | WO_AND)) == 0);
        assert!(match &p_term.u {
            WhereTermUnion::X { left_column, .. } => {
                *left_column >= XN_ROWID as i32 && *left_column < p_tab.borrow().n_col as i32
            }
            _ => false,
        });
        if (p_src.fg.jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0
            && !constraint_compatible_with_outer_join(&p_term, p_src)
        {
            continue;
        }
        n_term += 1;
        p_term.wt_flags |= TERM_OK;
    }

    // Se a cláusula ORDER BY contém só colunas da tabela virtual atual, aloca espaço para
    // a parte a_order_by da estrutura sqlite3_index_info.
    let mut n_order_by: i32 = 0;
    if let Some(p_order_by) = &p_order_by {
        let mut ob = p_order_by.borrow_mut();
        let n = ob.n_expr;
        let mut broke = false;
        for i in 0..n {
            let sort_flags = ob.a[i as usize].fg.sort_flags;
            let p_expr = ob.a[i as usize].p_expr.as_mut().unwrap();

            // Pula os termos constantes da cláusula ORDER BY
            if expr_is_constant(None, p_expr) {
                continue;
            }

            // Tabelas virtuais não sabem lidar com NULLS FIRST
            if (sort_flags & KEYINFO_ORDER_BIGNULL) != 0 {
                broke = true;
                break;
            }

            // Primeiro caso: uma referência direta a coluna, sem operador COLLATE
            if p_expr.op == TK_COLUMN && p_expr.i_table == p_src.i_cursor {
                assert!(
                    p_expr.i_column >= XN_ROWID as i32
                        && p_expr.i_column < p_tab.borrow().n_col as i32
                );
                continue;
            }

            // Segundo caso: uma referência a coluna com operador COLLATE. Só casa se o
            // operador COLLATE casar com a colação da coluna.
            if p_expr.op == TK_COLLATE
                && p_expr.p_left.as_ref().unwrap().op == TK_COLUMN
                && p_expr.p_left.as_ref().unwrap().i_table == p_src.i_cursor
            {
                let e2_i_column = p_expr.p_left.as_ref().unwrap().i_column;
                assert!(!expr_has_property(p_expr, EP_INTVALUE));
                assert!(p_expr.u.z_token.is_some());
                assert!(e2_i_column >= XN_ROWID as i32 && e2_i_column < p_tab.borrow().n_col as i32);
                p_expr.i_column = e2_i_column;
                if e2_i_column < 0 {
                    continue; // A colação não importa para o rowid
                }
                let tab = p_tab.borrow();
                let z_coll: &[u8] = match column_coll(&tab.a_col[e2_i_column as usize]) {
                    Some(z) => z,
                    None => STR_BINARY,
                };
                if api::stricmp(p_expr.u.z_token.as_deref(), Some(z_coll)) == 0 {
                    continue;
                }
            }

            // Nenhuma correspondência interrompe o laço
            broke = true;
            break;
        }
        if !broke {
            n_order_by = n;
            let wctrl_flags = p_w_info.borrow().wctrl_flags as u32;
            if (wctrl_flags & WHERE_DISTINCTBY) != 0 && p_src.fg.rowid_used == 0 {
                e_distinct = 2 + ((wctrl_flags & WHERE_SORTBYGROUP) != 0) as i32;
            } else if (wctrl_flags & WHERE_GROUPBY) != 0 {
                e_distinct = 1;
            }
        }
    }

    // Aloca a estrutura sqlite3_index_info
    let mut p_idx_cons: Vec<Sqlite3IndexConstraint> = Vec::with_capacity(n_term as usize);
    let mut p_idx_order_by: Vec<Sqlite3IndexOrderBy> = Vec::with_capacity(n_order_by as usize);
    let p_usage: Vec<Sqlite3IndexConstraintUsage> = (0..n_term)
        .map(|_| Sqlite3IndexConstraintUsage { argv_index: 0, omit: 0 })
        .collect();
    let mut p_hidden = HiddenIndexInfo {
        p_wc: Some(p_wc.clone()),
        p_parse: Some(p_parse.clone()),
        e_distinct,
        m_in: 0,
        m_handle_in: 0,
        a_rhs: vec![None; n_term as usize],
    };
    let mut j: i32 = 0;
    for i in 0..n_wc_term {
        let p_term_ref = p_wc.borrow().a[i as usize].clone();
        let p_term = p_term_ref.borrow();
        if (p_term.wt_flags & TERM_OK) == 0 {
            continue;
        }
        let i_column = match &p_term.u {
            WhereTermUnion::X { left_column, .. } => *left_column,
            _ => unreachable!(),
        };
        let mut cons_op: u8;
        let mut op: u16 = p_term.e_operator & WO_ALL;
        if op == WO_IN {
            if (p_term.wt_flags & TERM_SLICE) == 0 {
                p_hidden.m_in |= smaskbit32(j as u32);
            }
            op = WO_EQ;
        }
        if op == WO_AUX {
            cons_op = p_term.e_match_op;
        } else if (op & (WO_ISNULL | WO_IS)) != 0 {
            if op == WO_ISNULL {
                cons_op = SQLITE_INDEX_CONSTRAINT_ISNULL as u8;
            } else {
                cons_op = SQLITE_INDEX_CONSTRAINT_IS as u8;
            }
        } else {
            cons_op = op as u8;
            // A atribuição direta da linha anterior só é possível porque os códigos WO_ e
            // SQLITE_INDEX_CONSTRAINT_ são idênticos. Os asserts abaixo verificam esse fato.
            assert!(WO_EQ as i32 == SQLITE_INDEX_CONSTRAINT_EQ);
            assert!(WO_LT as i32 == SQLITE_INDEX_CONSTRAINT_LT);
            assert!(WO_LE as i32 == SQLITE_INDEX_CONSTRAINT_LE);
            assert!(WO_GT as i32 == SQLITE_INDEX_CONSTRAINT_GT);
            assert!(WO_GE as i32 == SQLITE_INDEX_CONSTRAINT_GE);
            assert!((p_term.e_operator & (WO_IN | WO_EQ | WO_LT | WO_LE | WO_GT | WO_GE | WO_AUX)) != 0);

            if (op & (WO_LT | WO_LE | WO_GT | WO_GE)) != 0
                && expr_is_vector(
                    p_term
                        .p_expr
                        .as_ref()
                        .unwrap()
                        .borrow()
                        .p_right
                        .as_ref()
                        .unwrap(),
                )
            {
                if j < 16 {
                    m_no_omit |= 1 << j;
                }
                if op == WO_LT {
                    cons_op = WO_LE as u8;
                }
                if op == WO_GT {
                    cons_op = WO_GE as u8;
                }
            }
        }
        p_idx_cons.push(Sqlite3IndexConstraint {
            i_column,
            op: cons_op,
            usable: 0,
            i_term_offset: i,
        });

        j += 1;
    }
    assert!(j == n_term);
    let n_constraint = j;
    j = 0;
    if n_order_by > 0 {
        let ob = p_order_by.as_ref().unwrap().borrow();
        for i in 0..n_order_by {
            let p_expr = ob.a[i as usize].p_expr.as_ref().unwrap();
            if expr_is_constant(None, p_expr) {
                continue;
            }
            assert!(
                p_expr.op == TK_COLUMN
                    || (p_expr.op == TK_COLLATE
                        && p_expr.p_left.as_ref().unwrap().op == TK_COLUMN
                        && p_expr.i_column == p_expr.p_left.as_ref().unwrap().i_column)
            );
            p_idx_order_by.push(Sqlite3IndexOrderBy {
                i_column: p_expr.i_column,
                desc: ob.a[i as usize].fg.sort_flags & KEYINFO_ORDER_DESC,
            });
            j += 1;
        }
    }

    *pm_no_omit = m_no_omit;
    Some((
        Sqlite3IndexInfo {
            n_constraint,
            a_constraint: Some(p_idx_cons.into_boxed_slice()),
            n_order_by: j,
            a_order_by: Some(p_idx_order_by.into_boxed_slice()),
            a_constraint_usage: Some(p_usage.into_boxed_slice()),
            idx_num: 0,
            idx_str: None,
            need_to_free_idx_str: 0,
            order_by_consumed: 0,
            estimated_cost: 0.0,
            estimated_rows: 0,
            idx_flags: 0,
            col_used: 0,
        },
        p_hidden,
    ))
}

/// Libera uma estrutura sqlite3_index_info alocada por `allocate_index_info()` e possivelmente
/// modificada pelos métodos xBestIndex.
fn free_index_info(db: &Sqlite3Ref, p_idx_info: Sqlite3IndexInfo, mut p_hidden: HiddenIndexInfo) {
    assert!(p_hidden.p_parse.is_some());
    assert!(std::ptr::eq(
        p_hidden.p_parse.as_ref().unwrap().borrow().db.as_ptr(),
        std::rc::Rc::as_ptr(db)
    ));
    for i in 0..p_idx_info.n_constraint {
        value_free(p_hidden.a_rhs[i as usize].take()); // IMP: R-14553-25174
    }
    drop(p_idx_info);
    drop(p_hidden);
}


// ---- part_004.rs ----

// Trecho 4 de where.c (sqlite 3.46.1).
//
// `whereKeyStats`, `sqlite3IndexColumnAffinity` e `whereRangeSkipScanEst` ficam sob
// `#ifdef SQLITE_ENABLE_STAT4`, opção que a build do Debian 13 não define (ver
// CONVENTIONS.md), então esses ramos somem. `whereTraceIndexInfoInputs/Outputs` e os
// `WHERETRACE()` só existem sob SQLITE_DEBUG/SQLITE_TEST e também somem.

/// A referência de tabela passada como segundo argumento deve representar uma tabela
/// virtual. Esta função chama o método xBestIndex() da tabela virtual com o objeto
/// sqlite3_index_info recebido como terceiro argumento.
///
/// Se ocorrer um erro, p_parse recebe uma mensagem de erro e um código apropriado é
/// devolvido. Um retorno SQLITE_CONSTRAINT do xBestIndex não é considerado erro:
/// SQLITE_CONSTRAINT indica que a configuração atual das flags "unusable" em
/// sqlite3_index_info não pode resultar em um plano válido.
///
/// Haja ou não erro, é responsabilidade do chamador liberar p.idx_str se
/// p.need_to_free_idx_str indicar que isso é necessário.
fn vtab_best_index(p_parse: &mut Parse, p_tab: &Table, p: &mut Sqlite3IndexInfo) -> i32 {
    let db_ref = match p_parse.db.upgrade() {
        Some(d) => d,
        None => return SQLITE_OK,
    };
    let p_vtab = get_vtable(&db_ref.borrow(), p_tab)
        .and_then(|v| v.borrow().p_vtab.clone())
        .expect("vtab_best_index: tabela virtual sem VTable");
    let rc: i32;

    db_ref.borrow_mut().n_schema_lock += 1;
    let p_module = p_vtab.borrow().p_module.clone();
    rc = match p_module {
        Some(m) => match &m.x_best_index {
            Some(x_best_index) => x_best_index(&p_vtab, p),
            None => SQLITE_OK,
        },
        None => SQLITE_OK,
    };
    db_ref.borrow_mut().n_schema_lock -= 1;

    if rc != SQLITE_OK && rc != SQLITE_CONSTRAINT {
        if rc == SQLITE_NOMEM {
            oom_fault(&mut db_ref.borrow_mut());
        } else if p_vtab.borrow().z_err_msg.is_none() {
            error_msg(p_parse, b"%s", Some(err_str(rc)));
        } else {
            let z_err_msg = p_vtab.borrow().z_err_msg.clone();
            error_msg(p_parse, b"%s", z_err_msg.as_deref());
        }
    }
    let b_all_schemas = match &p_tab.u {
        TableU::VTab(v) => v.p.as_ref().map_or(0, |vt| vt.borrow().b_all_schemas),
        _ => 0,
    };
    if b_all_schemas != 0 {
        vtab_uses_all_schemas(p_parse);
    }
    p_vtab.borrow_mut().z_err_msg = None;
    rc
}

/// Se não for None, p_term é um termo que dá um limite superior ou inferior para uma
/// varredura de intervalo. Sem considerar p_term, estima-se que a varredura visitará
/// n_new linhas. Esta função devolve o número estimado de linhas visitadas depois de
/// levar p_term em conta.
///
/// Se o usuário especificou explicitamente um valor likelihood() para este termo, o
/// retorno é a probabilidade multiplicada pelo número de linhas de entrada. Caso
/// contrário, a função supõe que um termo "IS NOT NULL" tem probabilidade 0,50 e
/// qualquer outro termo, 0,25.
fn where_range_adjust(p_term: Option<&WhereTerm>, n_new: LogEst) -> LogEst {
    let mut n_ret: LogEst = n_new;
    if let Some(p_term) = p_term {
        if p_term.truth_prob <= 0 {
            n_ret += p_term.truth_prob;
        } else if (p_term.wt_flags & TERM_VNULL) == 0 {
            n_ret -= 20; // 20 == log_est(4)
        }
    }
    n_ret
}


// ---- part_005.rs ----

// Fora deste trecho por configuração do Debian 13: o bloco SQLITE_ENABLE_STAT4 de
// where_range_scan_est (o ramo com dados sqlite_stat4 e where_range_skip_scan_est),
// where_equal_scan_est e where_in_scan_est (inteiras sob SQLITE_ENABLE_STAT4) e
// sqlite3WhereTermPrint (inteira sob WHERETRACE_ENABLED, ferramenta de depuração).

/// Estima o número de linhas visitadas ao varrer um índice por uma faixa de valores.
/// A faixa pode ter limite superior, inferior ou ambos; os termos da cláusula WHERE que
/// os definem chegam em `p_lower` e `p_upper` (None quando o limite não existe).
///
/// O valor de `p_builder.p_new.u.btree.n_eq` é o número da coluna do índice sujeita à
/// restrição de faixa. Ao chamar, `p_loop.n_out` é o `log_est` do número de linhas que o
/// scan visitaria sem considerar a faixa; ao retornar, foi reduzido para refletir
/// `p_lower` e `p_upper`.
///
/// Sem dados de ANALYZE sqlite_stat4, uma única desigualdade reduz o espaço de busca por
/// um fator de 4, e um par (x>? AND x<?) reduz o número esperado de linhas por 64.
fn where_range_scan_est(
    p_parse: &ParseRef,
    p_builder: &WhereLoopBuilder,
    p_lower: Option<&WhereTermRef>,
    p_upper: Option<&WhereTermRef>,
    p_loop: &WhereLoopRef,
) -> i32 {
    let rc = SQLITE_OK;
    let mut n_out: i32 = p_loop.borrow().n_out as i32;
    let mut n_new: LogEst;

    unused_parameter(p_parse);
    unused_parameter(p_builder);
    debug_assert!(p_lower.is_some() || p_upper.is_some());

    // where_range_adjust recebe `Option<&WhereTerm>`, então os termos compartilhados
    // são emprestados antes da chamada.
    let lower_b = p_lower.map(|t| t.borrow());
    let upper_b = p_upper.map(|t| t.borrow());
    n_new = where_range_adjust(lower_b.as_deref(), n_out as LogEst);
    n_new = where_range_adjust(upper_b.as_deref(), n_new);

    // TUNING: havendo limite superior e inferior e nenhum deles com likelihood() definido
    // pela aplicação, assume-se que a faixa é reduzida em mais 75%. Assim, por padrão,
    // uma faixa aberta (col > ?) casa 1/4 das linhas do índice, enquanto uma faixa
    // fechada (col BETWEEN ? AND ?) casa 1/64.
    if let (Some(lower), Some(upper)) = (lower_b.as_deref(), upper_b.as_deref()) {
        if lower.truth_prob > 0 && upper.truth_prob > 0 {
            n_new -= 20;
        }
    }

    n_out -= (p_lower.is_some() as i32) + (p_upper.is_some() as i32);
    if n_new < 10 {
        n_new = 10;
    }
    if (n_new as i32) < n_out {
        n_out = n_new as i32;
    }
    p_loop.borrow_mut().n_out = n_out as LogEst;
    rc
}


// ---- part_006.rs ----

// Trecho 6 de where.c (sqlite 3.46.1).
//
// Fora deste trecho: `sqlite3WhereClausePrint`, `sqlite3WhereLoopPrint`,
// `sqlite3ShowWhereLoop` e `sqlite3ShowWhereLoopList`, todas sob WHERETRACE_ENABLED
// (ferramenta de depuração, desligada na build do Debian 13), e os `WHERETRACE()`.
//
// Modelo de memória: `a_l_term` é um `Vec` que já inclui o espaço inicial (`a_l_term_space`
// tem 3 itens); "a_l_term aponta para a_l_term_space" no C vira `n_l_slot == 3`.

/// Número de slots do espaço inicial de `a_l_term` (`ArraySize(aLTermSpace)`).
const L_TERM_SPACE: u16 = 3;

/// Compara dois `Option<IndexRef>` como o `==` de ponteiros do C.
fn same_index(a: &Option<IndexRef>, b: &Option<IndexRef>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        (None, None) => true,
        _ => false,
    }
}

/// Lê `u.btree.nEq` e `u.btree.pIndex` de um WhereLoop que não é de tabela virtual.
fn btree_eq_and_index(u: &WhereLoopUnion) -> (u16, Option<IndexRef>) {
    match u {
        WhereLoopUnion::Btree { n_eq, p_index, .. } => (*n_eq, p_index.clone()),
        WhereLoopUnion::Vtab { .. } => (0, None),
    }
}

/// Copia a união de `p_from` para `p_to` (o `memcpy` de WHERE_LOOP_XFER_SZ do C). O
/// `idx_str` é duplicado; quem passa a ser dono é decidido pelo chamador via `need_free`.
fn copy_loop_union(u: &WhereLoopUnion) -> WhereLoopUnion {
    match u {
        WhereLoopUnion::Btree { n_eq, n_btm, n_top, n_distinct_col, p_index } => {
            WhereLoopUnion::Btree {
                n_eq: *n_eq,
                n_btm: *n_btm,
                n_top: *n_top,
                n_distinct_col: *n_distinct_col,
                p_index: p_index.clone(),
            }
        }
        WhereLoopUnion::Vtab {
            idx_num,
            need_free,
            b_omit_offset,
            is_ordered,
            omit_mask,
            idx_str,
            m_handle_in,
        } => WhereLoopUnion::Vtab {
            idx_num: *idx_num,
            need_free: *need_free,
            b_omit_offset: *b_omit_offset,
            is_ordered: *is_ordered,
            omit_mask: *omit_mask,
            idx_str: idx_str.clone(),
            m_handle_in: *m_handle_in,
        },
    }
}

/// Converte memória em bruto num WhereLoop válido que pode ser passado a
/// where_loop_clear sem danos.
fn where_loop_init(p: &mut WhereLoop) {
    p.a_l_term = vec![None; L_TERM_SPACE as usize];
    p.n_l_term = 0;
    p.n_l_slot = L_TERM_SPACE;
    p.ws_flags = 0;
}

/// Limpa a união WhereLoop.u. Deixa WhereLoop.a_l_term intacta.
fn where_loop_clear_union(_db: &Sqlite3Ref, p: &mut WhereLoop) {
    if p.ws_flags & (WHERE_VIRTUALTABLE | WHERE_AUTO_INDEX) != 0 {
        if (p.ws_flags & WHERE_VIRTUALTABLE) != 0 {
            if let WhereLoopUnion::Vtab { need_free, idx_str, .. } = &mut p.u {
                if *need_free {
                    *idx_str = Vec::new();
                    *need_free = false;
                }
            }
        } else if (p.ws_flags & WHERE_AUTO_INDEX) != 0 {
            if let WhereLoopUnion::Btree { p_index, .. } = &mut p.u {
                // O índice automático pertence ao loop: soltar a referência o libera.
                *p_index = None;
            }
        }
    }
}

/// Desaloca a memória interna usada por um objeto WhereLoop. Deixa o objeto em estado
/// inicializado, como se tivesse sido alocado agora.
fn where_loop_clear(db: &Sqlite3Ref, p: &mut WhereLoop) {
    if p.n_l_slot > L_TERM_SPACE {
        p.a_l_term = vec![None; L_TERM_SPACE as usize];
        p.n_l_slot = L_TERM_SPACE;
    }
    where_loop_clear_union(db, p);
    p.n_l_term = 0;
    p.ws_flags = 0;
}

/// Aumenta a alocação de p.a_l_term[] para pelo menos n itens.
fn where_loop_resize(_db: &Sqlite3Ref, p: &mut WhereLoop, n: i32) -> i32 {
    if p.n_l_slot as i32 >= n {
        return SQLITE_OK;
    }
    let n = (n + 7) & !7;
    let mut pa_new: Vec<Option<WhereTermRef>> = vec![None; n as usize];
    for (dst, src) in pa_new.iter_mut().zip(p.a_l_term.iter().take(p.n_l_slot as usize)) {
        *dst = src.clone();
    }
    p.a_l_term = pa_new;
    p.n_l_slot = n as u16;
    SQLITE_OK
}

/// Transfere o conteúdo do segundo WhereLoop para o primeiro.
fn where_loop_xfer(db: &Sqlite3Ref, p_to: &mut WhereLoop, p_from: &mut WhereLoop) -> i32 {
    where_loop_clear_union(db, p_to);
    if p_from.n_l_term > p_to.n_l_slot
        && where_loop_resize(db, p_to, p_from.n_l_term as i32) != SQLITE_OK
    {
        // memset(pTo, 0, WHERE_LOOP_XFER_SZ)
        p_to.prereq = 0;
        p_to.mask_self = 0;
        p_to.i_tab = 0;
        p_to.i_sort_idx = 0;
        p_to.r_setup = 0;
        p_to.r_run = 0;
        p_to.n_out = 0;
        p_to.u = WhereLoopUnion::Btree {
            n_eq: 0,
            n_btm: 0,
            n_top: 0,
            n_distinct_col: 0,
            p_index: None,
        };
        p_to.ws_flags = 0;
        p_to.n_l_term = 0;
        p_to.n_skip = 0;
        return SQLITE_NOMEM_BKPT;
    }
    // memcpy(pTo, pFrom, WHERE_LOOP_XFER_SZ): tudo até n_skip.
    p_to.prereq = p_from.prereq;
    p_to.mask_self = p_from.mask_self;
    p_to.i_tab = p_from.i_tab;
    p_to.i_sort_idx = p_from.i_sort_idx;
    p_to.r_setup = p_from.r_setup;
    p_to.r_run = p_from.r_run;
    p_to.n_out = p_from.n_out;
    p_to.u = copy_loop_union(&p_from.u);
    p_to.ws_flags = p_from.ws_flags;
    p_to.n_l_term = p_from.n_l_term;
    p_to.n_skip = p_from.n_skip;
    for i in 0..p_to.n_l_term as usize {
        p_to.a_l_term[i] = p_from.a_l_term[i].clone();
    }
    if (p_from.ws_flags & WHERE_VIRTUALTABLE) != 0 {
        if let WhereLoopUnion::Vtab { need_free, .. } = &mut p_from.u {
            *need_free = false;
        }
    } else if (p_from.ws_flags & WHERE_AUTO_INDEX) != 0 {
        if let WhereLoopUnion::Btree { p_index, .. } = &mut p_from.u {
            *p_index = None;
        }
    }
    SQLITE_OK
}

/// Apaga um objeto WhereLoop.
fn where_loop_delete(db: &Sqlite3Ref, p: &mut WhereLoop) {
    where_loop_clear(db, p);
}

/// Libera uma estrutura WhereInfo.
fn where_info_free(db: &Sqlite3Ref, p_w_info: &mut WhereInfo) {
    where_clause_clear(&mut p_w_info.s_wc);
    while let Some(p_ref) = p_w_info.p_loops.take() {
        p_w_info.p_loops = p_ref.borrow_mut().p_next_loop.take();
        where_loop_delete(db, &mut p_ref.borrow_mut());
    }
    while let Some(mut p_block) = p_w_info.p_mem_to_free.take() {
        p_w_info.p_mem_to_free = p_block.p_next.take();
    }
}

/// Devolve verdadeiro se X é um subconjunto próprio de Y mas de custo igual ou menor.
/// Em outras palavras, se todas as restrições de X também fazem parte de Y e Y tem
/// restrições adicionais que podem acelerar a busca que X não tem, mas o custo de rodar
/// X não é maior que o de Y.
///
/// Em outras palavras, devolve verdadeiro se a relação de custo entre X e Y está
/// invertida e precisa ser ajustada.
///
/// Caso 1:
///   (1a)  X e Y usam o mesmo índice.
///   (1b)  X tem menos termos == que Y
///   (1c)  Nem X nem Y usam skip-scan
///   (1d)  X não tem custo maior que Y
///
/// Caso 2:
///   (2a)  X tem custo igual ou menor, ou devolve o mesmo número de linhas ou menos, que Y.
///   (2b)  X usa menos termos da cláusula WHERE que Y
///   (2c)  Todo termo da cláusula WHERE usado por X também é usado por Y
///   (2d)  X pula pelo menos tantas colunas quanto Y
///   (2e)  Se X é um índice de cobertura, então Y também é
fn where_loop_cheaper_proper_subset(p_x: &WhereLoop, p_y: &WhereLoop) -> i32 {
    if p_x.r_run > p_y.r_run && p_x.n_out > p_y.n_out {
        return 0; // (1d) e (2a)
    }
    debug_assert!((p_x.ws_flags & WHERE_VIRTUALTABLE) == 0);
    debug_assert!((p_y.ws_flags & WHERE_VIRTUALTABLE) == 0);
    let (n_eq_x, p_idx_x) = btree_eq_and_index(&p_x.u);
    let (n_eq_y, p_idx_y) = btree_eq_and_index(&p_y.u);
    if n_eq_x < n_eq_y                                  // (1b)
        && same_index(&p_idx_x, &p_idx_y)               // (1a)
        && p_x.n_skip == 0 && p_y.n_skip == 0           // (1c)
    {
        return 1; // O caso 1 é verdadeiro
    }
    if p_x.n_l_term as i32 - p_x.n_skip as i32 >= p_y.n_l_term as i32 - p_y.n_skip as i32 {
        return 0; // (2b)
    }
    if p_y.n_skip > p_x.n_skip {
        return 0; // (2d)
    }
    for i in (0..p_x.n_l_term as usize).rev() {
        let term_x = match &p_x.a_l_term[i] {
            Some(t) => t,
            None => continue,
        };
        let mut found = false;
        for j in (0..p_y.n_l_term as usize).rev() {
            if let Some(term_y) = &p_y.a_l_term[j] {
                if Rc::ptr_eq(term_y, term_x) {
                    found = true;
                    break;
                }
            }
        }
        if !found {
            return 0; // (2c)
        }
    }
    if (p_x.ws_flags & WHERE_IDX_ONLY) != 0 && (p_y.ws_flags & WHERE_IDX_ONLY) == 0 {
        return 0; // (2e)
    }
    1 // O caso 2 é verdadeiro
}

/// Tenta ajustar o custo e o número de linhas de saída do WhereLoop `p_template` para
/// cima ou para baixo de modo que:
///
///   (1) p_template custe menos que qualquer outro WhereLoop que seja um subconjunto
///       próprio de p_template
///
///   (2) p_template custe mais que qualquer outro WhereLoop do qual p_template seja
///       um subconjunto próprio.
///
/// Dizer "o WhereLoop X é um subconjunto próprio de Y" significa que X usa menos termos
/// da cláusula WHERE que Y e que todo termo usado por X também é usado por Y.
fn where_loop_adjust_cost(p_head: Option<WhereLoopRef>, p_template: &mut WhereLoop) {
    if (p_template.ws_flags & WHERE_INDEXED) == 0 {
        return;
    }
    let mut p = p_head;
    while let Some(p_ref) = p {
        let p_loop = p_ref.borrow();
        if p_loop.i_tab == p_template.i_tab && (p_loop.ws_flags & WHERE_INDEXED) != 0 {
            if where_loop_cheaper_proper_subset(&p_loop, p_template) != 0 {
                // Ajusta o custo de p_template para baixo, para que seja mais barato que
                // seu subconjunto p.
                p_template.r_run = std::cmp::min(p_loop.r_run, p_template.r_run);
                p_template.n_out = std::cmp::min(p_loop.n_out - 1, p_template.n_out);
            } else if where_loop_cheaper_proper_subset(p_template, &p_loop) != 0 {
                // Ajusta o custo de p_template para cima, para que seja mais caro que p,
                // já que p_template é um subconjunto próprio de p.
                p_template.r_run = std::cmp::max(p_loop.r_run, p_template.r_run);
                p_template.n_out = std::cmp::max(p_loop.n_out + 1, p_template.n_out);
            }
        }
        p = p_loop.p_next_loop.clone();
    }
}

/// Procura na lista de WhereLoop que começa em `p_head` um que possa ser substituído
/// por `p_template`.
///
/// O `WhereLoop **ppPrev` do C é um "elo" (o campo `pNextLoop` de um nó, ou a cabeça da
/// lista). Aqui o elo é representado pelo nó ANTERIOR: `None` significa a cabeça da
/// lista, `Some(n)` significa `n.p_next_loop`. O elo aponta para o nó a substituir, ou
/// para o fim da lista (valor `None` no elo) quando p_template deve ser acrescentado.
///
/// Devolve `None` (o NULL do C) se p_template não pertence à lista, ou seja, se deve ser
/// descartado. Caso contrário devolve `Some(anterior)` com o elo descrito acima.
fn where_loop_find_lesser(
    p_head: &Option<WhereLoopRef>,
    p_template: &WhereLoop,
) -> Option<Option<WhereLoopRef>> {
    let mut prev: Option<WhereLoopRef> = None;
    let mut cur = p_head.clone();
    while let Some(p_ref) = cur {
        let p = p_ref.borrow();
        if p.i_tab != p_template.i_tab || p.i_sort_idx != p_template.i_sort_idx {
            // Se o i_tab ou o i_sort_idx de dois WhereLoop diferem, eles precisam ser
            // considerados separadamente. Nenhum é candidato a substituir o outro.
            prev = Some(p_ref.clone());
            cur = p.p_next_loop.clone();
            continue;
        }
        // Na implementação atual, r_setup é zero ou o custo de construir um índice
        // automático (NlogN), e o NlogN é o mesmo para WhereLoop compatíveis.
        debug_assert!(p.r_setup == 0 || p_template.r_setup == 0 || p.r_setup == p_template.r_setup);

        // where_loop_add_btree() sempre gera e insere primeiro o caso do índice
        // automático. Logo, candidatos compatíveis nunca têm r_setup maior.
        // Chame isto de SETUP-INVARIANT.
        debug_assert!(p.r_setup >= p_template.r_setup);

        // Qualquer loop que use um índice definido pela aplicação (ou PRIMARY KEY ou
        // restrição UNIQUE) com uma ou mais restrições == é melhor que um índice
        // automático. A menos que seja um skip-scan.
        if (p.ws_flags & WHERE_AUTO_INDEX) != 0
            && p_template.n_skip == 0
            && (p_template.ws_flags & WHERE_INDEXED) != 0
            && (p_template.ws_flags & WHERE_COLUMN_EQ) != 0
            && (p.prereq & p_template.prereq) == p_template.prereq
        {
            break;
        }

        // Se o WhereLoop existente p é melhor que p_template, p_template pode ser
        // descartado. O WhereLoop p é melhor se:
        //   (1)  p não tem mais dependências que p_template, e
        //   (2)  p tem custo igual ou menor que p_template
        if (p.prereq & p_template.prereq) == p.prereq   // (1)
            && p.r_setup <= p_template.r_setup          // (2a)
            && p.r_run <= p_template.r_run              // (2b)
            && p.n_out <= p_template.n_out              // (2c)
        {
            return None; // Descarta p_template
        }

        // Se p_template é sempre melhor que p, faz p ser sobrescrito por p_template.
        // p_template é melhor que p se:
        //   (1)  p_template não tem mais dependências que p, e
        //   (2)  p_template tem custo igual ou menor que p.
        if (p.prereq & p_template.prereq) == p_template.prereq  // (1)
            && p.r_run >= p_template.r_run                      // (2a)
            && p.n_out >= p_template.n_out                      // (2b)
        {
            debug_assert!(p.r_setup >= p_template.r_setup); // SETUP-INVARIANT acima
            break; // Faz p ser sobrescrito por p_template
        }

        prev = Some(p_ref.clone());
        cur = p.p_next_loop.clone();
    }
    Some(prev)
}


// ---- part_007.rs ----

// Trecho 7 de where.c (sqlite 3.46.1).
//
// `whereLoopAddBtreeIndex` começa neste trecho do C (cabeçalho e locais, linhas 327 a 350),
// mas uma função Rust não pode ser cortada: ela está inteira em part_008.rs e NÃO é
// definida aqui. `ApplyCostMultiplier` (SQLITE_ENABLE_COSTMULT) e os blocos
// WHERETRACE_ENABLED somem na build do Debian 13.

/// Insere ou substitui uma entrada WhereLoop usando o modelo `p_template`.
///
/// Uma entrada WhereLoop existente pode ser sobrescrita se o novo modelo for melhor e
/// tiver menos dependências. Ou o modelo é ignorado, sem inserção, se um WhereLoop
/// existente for mais rápido e tiver menos dependências que ele. Caso contrário, um
/// novo WhereLoop é acrescentado a partir do modelo.
///
/// Se `p_builder.p_or_set` não é None, só importam os pré-requisitos e os custos r_run
/// e n_out dos N melhores loops. Essa informação é reunida no objeto p_or_set. Este modo
/// especial é usado só no processamento de cláusulas OR.
///
/// Ao acumular vários loops (p_or_set None) loops parecidos ainda podem ser sobrescritos
/// pelo novo modelo se ele for melhor. Um loop pode ser sobrescrito se:
///
///    (1)  Têm o mesmo i_tab.
///    (2)  Têm o mesmo i_sort_idx.
///    (3)  O modelo tem as mesmas dependências ou menos que o loop atual
///    (4)  O modelo tem o mesmo custo ou menor que o loop atual
fn where_loop_insert(p_builder: &mut WhereLoopBuilder, p_template: &WhereLoopRef) -> i32 {
    let p_w_info = p_builder.p_w_info.clone();
    let db = p_w_info
        .borrow()
        .p_parse
        .borrow()
        .db
        .upgrade()
        .expect("where_loop_insert: conexão já destruída");

    // Interrompe a busca ao atingir o limite de busca do planejador de consultas
    if p_builder.i_plan_limit == 0 {
        if let Some(p_or_set) = &p_builder.p_or_set {
            p_or_set.borrow_mut().n = 0;
        }
        return SQLITE_DONE;
    }
    p_builder.i_plan_limit -= 1;

    let p_loops = p_w_info.borrow().p_loops.clone();
    where_loop_adjust_cost(p_loops, &mut p_template.borrow_mut());

    // Se p_or_set está definido, só acompanha custos e pré-requisitos.
    if let Some(p_or_set) = &p_builder.p_or_set {
        let t = p_template.borrow();
        if t.n_l_term != 0 {
            where_or_insert(&mut p_or_set.borrow_mut(), t.prereq, t.r_run, t.n_out);
        }
        return SQLITE_OK;
    }

    // Procura um WhereLoop existente para substituir por p_template
    let p_head = p_w_info.borrow().p_loops.clone();
    let found = where_loop_find_lesser(&p_head, &p_template.borrow());
    let p_prev = match found {
        // Já existe na lista um WhereLoop melhor que p_template: ignora p_template
        None => return SQLITE_OK,
        Some(prev) => prev,
    };
    // `p_prev` é o nó anterior ao elo (None: cabeça da lista); `p` é o alvo do elo.
    let p: Option<WhereLoopRef> = match &p_prev {
        None => p_head.clone(),
        Some(n) => n.borrow().p_next_loop.clone(),
    };

    // Chegando aqui, ou p[] deve ser sobrescrito por p_template[] se p existe, ou, se p
    // é NULL, aloca-se um novo WhereLoop e o insere.
    let p: WhereLoopRef = match p {
        None => {
            // Aloca um novo WhereLoop para acrescentar ao fim da lista
            let p_new = Rc::new(RefCell::new(WhereLoop::default()));
            where_loop_init(&mut p_new.borrow_mut());
            p_new.borrow_mut().p_next_loop = None;
            match &p_prev {
                None => p_w_info.borrow_mut().p_loops = Some(p_new.clone()),
                Some(n) => n.borrow_mut().p_next_loop = Some(p_new.clone()),
            }
            p_new
        }
        Some(p) => {
            // Vamos sobrescrever o WhereLoop p[]. Antes, percorre o resto da lista e
            // apaga qualquer outra entrada, além de p[], que também seja suplantada por
            // p_template
            let mut p_tail_owner: WhereLoopRef = p.clone();
            loop {
                let tail_head = p_tail_owner.borrow().p_next_loop.clone();
                if tail_head.is_none() {
                    break;
                }
                let rel = where_loop_find_lesser(&tail_head, &p_template.borrow());
                let rel = match rel {
                    None => break,
                    Some(r) => r,
                };
                let link_owner = rel.unwrap_or_else(|| p_tail_owner.clone());
                let p_to_del = link_owner.borrow().p_next_loop.clone();
                let p_to_del = match p_to_del {
                    None => break,
                    Some(d) => d,
                };
                link_owner.borrow_mut().p_next_loop = p_to_del.borrow_mut().p_next_loop.take();
                where_loop_delete(&db, &mut p_to_del.borrow_mut());
                p_tail_owner = link_owner;
            }
            p
        }
    };
    let rc = where_loop_xfer(&db, &mut p.borrow_mut(), &mut p_template.borrow_mut());
    let mut p_ref = p.borrow_mut();
    if (p_ref.ws_flags & WHERE_VIRTUALTABLE) == 0 {
        if let WhereLoopUnion::Btree { p_index, .. } = &mut p_ref.u {
            let is_ipk = match p_index {
                Some(idx) => idx.borrow().idx_type == SQLITE_IDXTYPE_IPK,
                None => false,
            };
            if is_ipk {
                *p_index = None;
            }
        }
    }
    rc
}

/// Ajusta o valor WhereLoop.n_out para baixo, para levar em conta os termos da cláusula
/// WHERE que referenciam o loop mas não são usados por um índice.
///
/// Para todo termo da cláusula WHERE não usado pelo índice e que tenha uma
/// probabilidade de verdade atribuída por likelihood(), likely() ou unlikely(), reduz o
/// número estimado de linhas de saída pela probabilidade especificada.
///
/// TUNING: para todo termo não usado pelo índice e sem probabilidade atribuída, usam-se
/// as heurísticas abaixo para estimá-la.
///
/// Heurística 1: estima a probabilidade de verdade em 93,75%. Esse valor corresponde a -1
/// na notação LogEst, ou seja, decrementa WhereLoop.n_out para cada termo assim.
///
/// Heurística 2: se existem termos da forma "x==EXPR" e EXPR não é a constante 0 ou 1,
/// garante que a estimativa final de linhas de saída não passe de 1/4 do total de linhas
/// da tabela. Se EXPR for -1, 0 ou 1, a coluna "x" talvez seja booleana ou esses valores
/// sejam padrões comuns, e então a estimativa é limitada a 1/2 em vez de 1/4.
fn where_loop_output_adjust(p_wc: &WhereClauseRef, p_loop: &WhereLoopRef, n_row: LogEst) {
    let (prereq, mask_self, n_l_term, i_tab) = {
        let l = p_loop.borrow();
        (l.prereq, l.mask_self, l.n_l_term as i32, l.i_tab as usize)
    };
    let not_allowed: Bitmask = !(prereq | mask_self);
    let mut i_reduce: LogEst = 0; // p_loop.n_out não deve passar de n_row - i_reduce

    debug_assert!((p_loop.borrow().ws_flags & WHERE_AUTO_INDEX) == 0);
    let n_base = p_wc.borrow().n_base;
    for i in 0..n_base as usize {
        let p_term = p_wc.borrow().a[i].clone();
        let (prereq_all, wt_flags) = {
            let t = p_term.borrow();
            (t.prereq_all, t.wt_flags)
        };
        if (prereq_all & not_allowed) != 0 {
            continue;
        }
        if (prereq_all & mask_self) == 0 {
            continue;
        }
        if (wt_flags & TERM_VIRTUAL) != 0 {
            continue;
        }
        let mut found = false;
        for j in (0..n_l_term).rev() {
            let p_x = match &p_loop.borrow().a_l_term[j as usize] {
                Some(x) => x.clone(),
                None => continue,
            };
            if Rc::ptr_eq(&p_x, &p_term) {
                found = true;
                break;
            }
            let i_parent = p_x.borrow().i_parent;
            if i_parent >= 0 && Rc::ptr_eq(&p_wc.borrow().a[i_parent as usize], &p_term) {
                found = true;
                break;
            }
        }
        if !found {
            let p_w_info = p_wc.borrow().p_w_info.upgrade().expect("WhereInfo destruído");
            let p_parse = p_w_info.borrow().p_parse.clone();
            progress_check(&mut p_parse.borrow_mut());
            let (e_operator, truth_prob) = {
                let t = p_term.borrow();
                (t.e_operator, t.truth_prob)
            };
            if mask_self == prereq_all {
                // Se há termos extras na cláusula WHERE não usados por um índice, que
                // dependem só da tabela varrida e que tendem a omitir muitas linhas,
                // marca a tabela como "self-culling".
                //
                // 2022-03-24: o self-culling só vale se os termos extras são operadores
                // de comparação simples, não verdadeiros com operando NULL, ou se o loop
                // não é um OUTER JOIN.
                let jointype = p_w_info.borrow().p_tab_list.borrow().a[i_tab].fg.jointype;
                if (e_operator & 0x3f) != 0 || (jointype & (JT_LEFT | JT_LTORJ)) == 0 {
                    p_loop.borrow_mut().ws_flags |= WHERE_SELFCULL;
                }
            }
            if truth_prob <= 0 {
                // Se uma probabilidade de verdade é dada por dicas likelihood(), usa a
                // probabilidade fornecida pela aplicação.
                p_loop.borrow_mut().n_out += truth_prob;
            } else {
                // Sem probabilidades explícitas, usa heurísticas para adivinhar uma
                // probabilidade razoável.
                p_loop.borrow_mut().n_out -= 1;
                if (e_operator & (WO_EQ | WO_IS)) != 0
                    && (wt_flags & TERM_HIGHTRUTH) == 0 // tag-20200224-1
                {
                    let mut k: i32 = 0;
                    let is_small_int = {
                        let t = p_term.borrow();
                        let p_expr = t.p_expr.as_ref().expect("termo sem expressão").borrow();
                        match p_expr.p_right.as_deref() {
                            Some(p_right) => {
                                expr_is_integer(p_right, &mut k) != 0 && k >= -1 && k <= 1
                            }
                            None => false,
                        }
                    };
                    let k: LogEst = if is_small_int { 10 } else { 20 };
                    if i_reduce < k {
                        p_term.borrow_mut().wt_flags |= TERM_HEURTRUTH;
                        i_reduce = k;
                    }
                }
            }
        }
    }
    let mut l = p_loop.borrow_mut();
    if l.n_out > n_row - i_reduce {
        l.n_out = n_row - i_reduce;
    }
}

/// O termo `p_term` é uma comparação de intervalo vetorial. A primeira comparação do
/// vetor pode ser otimizada usando a coluna `n_eq` do índice. Esta função devolve o
/// número total de elementos do vetor que podem ser usados na comparação de intervalo.
///
/// Por exemplo, se a consulta é:
///
///   WHERE a = ? AND (b, c, d) > (?, ?, ?)
///
/// e o índice:
///
///   CREATE INDEX ... ON (a, b, c, d, e)
///
/// esta função seria chamada com n_eq=1 e o valor devolvido seria 3.
fn where_range_vector_len(
    p_parse: &ParseRef,
    i_cur: i32,
    p_idx: &Index,
    n_eq: i32,
    p_term: &WhereTerm,
) -> i32 {
    let p_expr_ref = p_term.p_expr.as_ref().expect("termo sem expressão");
    let p_expr = p_expr_ref.borrow();
    let p_left = p_expr.p_left.as_deref().expect("comparação vetorial sem lado esquerdo");
    let mut n_cmp = expr_vector_size(p_left);
    let mut i: i32 = 1;

    n_cmp = std::cmp::min(n_cmp, p_idx.n_column as i32 - n_eq);
    while i < n_cmp {
        // Testa se a comparação i de p_term é compatível com a coluna (i+n_eq) do
        // índice. Se não for, sai do laço.
        debug_assert!(expr_use_x_list(p_left));
        let p_lhs = p_left.x.p_list.as_ref().expect("lista do lado esquerdo").a[i as usize]
            .p_expr
            .as_deref()
            .expect("elemento do vetor esquerdo");
        let p_right = p_expr.p_right.as_deref().expect("comparação vetorial sem lado direito");
        let p_rhs = if expr_use_x_select(p_right) {
            p_right
                .x
                .p_select
                .as_ref()
                .expect("subconsulta do lado direito")
                .p_elist
                .as_ref()
                .expect("lista de resultado")
                .a[i as usize]
                .p_expr
                .as_deref()
                .expect("elemento do vetor direito")
        } else {
            p_right.x.p_list.as_ref().expect("lista do lado direito").a[i as usize]
                .p_expr
                .as_deref()
                .expect("elemento do vetor direito")
        };

        // Confere que o lado esquerdo da comparação é uma referência de coluna à coluna
        // certa da tabela de origem certa, e que a ordem de classificação da coluna do
        // índice é a mesma da coluna mais à esquerda do índice.
        let k = (i + n_eq) as usize;
        if p_lhs.op != TK_COLUMN
            || p_lhs.i_table != i_cur
            || p_lhs.i_column != p_idx.ai_column[k] as i32
            || p_idx.a_sort_order[k] != p_idx.a_sort_order[n_eq as usize]
        {
            break;
        }

        let aff = compare_affinity(p_rhs, expr_affinity(p_lhs));
        let idxaff = {
            let p_table = p_idx.p_table.upgrade().expect("índice sem tabela");
            let aff = table_column_affinity(&p_table.borrow(), p_lhs.i_column);
            aff
        };
        if aff != idxaff {
            break;
        }

        let p_coll = binary_compare_coll_seq(&mut p_parse.borrow_mut(), p_lhs, Some(p_rhs));
        let p_coll = match p_coll {
            None => break,
            Some(c) => c,
        };
        if str_i_cmp(&p_coll.borrow().z_name, &p_idx.az_coll[k]) != 0 {
            break;
        }
        i += 1;
    }
    i
}


// ---- part_008.rs ----

// Nota de integração: o trecho where_c.008.c começa no meio de whereLoopAddBtreeIndex
// (o cabeçalho e as variáveis locais estão no fim de where_c.007.c, linhas 327 a 351).
// Uma função Rust não pode ser cortada, então esta parte traz a função inteira, com o
// cabeçalho e os locais; a parte 007 não deve definir `where_loop_add_btree_index`.
//
// Assinaturas de outros trechos que esta parte assume (convenção de nomes):
//   where_loop_resize(&Sqlite3Ref, &mut WhereLoop, i32) -> i32   (deixa a_l_term com n_l_slot itens)
//   where_range_vector_len(&ParseRef, i32, &Index, i32, &WhereTerm) -> i32
//   where_range_scan_est(&ParseRef, &mut WhereLoopBuilder, Option<&WhereTermRef>, Option<&WhereTermRef>, &WhereLoopRef)
//   where_loop_output_adjust(&WhereClauseRef, &WhereLoopRef, LogEst)
//   where_loop_insert(&mut WhereLoopBuilder, &WhereLoopRef) -> i32
//   constraint_compatible_with_outer_join(&WhereTerm, &SrcItem) -> bool
//   progress_check(&ParseRef)
//   optimization_enabled(&Sqlite3, u32) -> bool, SQLITE_SEEK_SCAN, SQLITE_SKIP_SCAN
// SQLITE_ENABLE_STAT4, SQLITE_ENABLE_COSTMULT e WHERETRACE estão desligados no Debian 13:
// os blocos correspondentes somem.

/// Lê os contadores (n_eq, n_btm, n_top) da parte `u.btree` de um WhereLoop.
fn add_btree_idx_counts(p_loop: &WhereLoop) -> (u16, u16, u16) {
    match &p_loop.u {
        WhereLoopUnion::Btree { n_eq, n_btm, n_top, .. } => (*n_eq, *n_btm, *n_top),
        _ => unreachable!("o laço do btree nunca é de tabela virtual"),
    }
}

/// Acesso mutável aos contadores (n_eq, n_btm, n_top) da parte `u.btree` de um WhereLoop.
fn add_btree_idx_counts_mut(p_loop: &mut WhereLoop) -> (&mut u16, &mut u16, &mut u16) {
    match &mut p_loop.u {
        WhereLoopUnion::Btree { n_eq, n_btm, n_top, .. } => (n_eq, n_btm, n_top),
        _ => unreachable!("o laço do btree nunca é de tabela virtual"),
    }
}

/// Aumenta a_l_term de `p_new` para caber mais um termo. Verdadeiro se faltou memória.
fn add_btree_idx_grow(db: &Sqlite3Ref, p_new: &WhereLoopRef) -> bool {
    let n_wanted = p_new.borrow().n_l_term as i32 + 1;
    where_loop_resize(db, &mut p_new.borrow_mut(), n_wanted) != 0
}

/// Acrescenta `term` ao fim de a_l_term (equivale a `aLTerm[nLTerm++] = term`).
fn add_btree_idx_push(p_new: &WhereLoopRef, term: Option<WhereTermRef>) {
    let mut n = p_new.borrow_mut();
    let i = n.n_l_term as usize;
    n.a_l_term[i] = term;
    n.n_l_term += 1;
}

/// Já casamos `p_builder.p_new.u.btree.n_eq` termos do índice `p_probe`. Tenta casar mais um.
///
/// Quando a função é chamada, `p_new.n_out` tem o número de linhas esperado ao filtrar só
/// pelos n_eq termos. Se for modificado, o valor é restaurado antes do retorno.
///
/// Se `p_probe.idx_type == SQLITE_IDXTYPE_IPK`, o índice é falso, usado para a INTEGER
/// PRIMARY KEY.
pub fn where_loop_add_btree_index(
    p_builder: &mut WhereLoopBuilder,
    p_src: &SrcItem,
    p_probe: &Index,
    n_in_mul: LogEst,
) -> i32 {
    let p_w_info = p_builder.p_w_info.clone();
    let p_parse = p_w_info.borrow().p_parse.clone();
    let db = p_parse
        .borrow()
        .db
        .upgrade()
        .expect("a conexão de banco de dados vive mais que o Parse");
    let mut rc: i32 = SQLITE_OK;
    // Restrições de topo e de base do intervalo (declaradas fora do laço, como no C)
    let mut p_top: Option<WhereTermRef> = None;
    let mut p_btm: Option<WhereTermRef> = None;

    let p_new = p_builder.p_new.clone();
    if p_parse.borrow().n_err != 0 {
        return p_parse.borrow().rc;
    }

    let mut op_mask: u32 = if (p_new.borrow().ws_flags & WHERE_BTM_LIMIT) != 0 {
        (WO_LT | WO_LE) as u32
    } else {
        (WO_EQ | WO_IN | WO_GT | WO_GE | WO_LT | WO_LE | WO_ISNULL | WO_IS) as u32
    };
    if p_probe.b_unordered || p_probe.b_low_qual {
        if p_probe.b_unordered {
            op_mask &= !((WO_GT | WO_GE | WO_LT | WO_LE) as u32);
        }
        if p_probe.b_low_qual && p_src.fg.is_indexed_by == 0 {
            op_mask &= !((WO_EQ | WO_IN | WO_IS) as u32);
        }
    }

    let (
        saved_n_eq,
        saved_n_btm,
        saved_n_top,
        saved_n_skip,
        saved_n_l_term,
        saved_ws_flags,
        saved_prereq,
        saved_n_out,
        mask_self,
    ) = {
        let n = p_new.borrow();
        let (n_eq, n_btm, n_top) = add_btree_idx_counts(&n);
        (
            n_eq,
            n_btm,
            n_top,
            n.n_skip,
            n.n_l_term,
            n.ws_flags,
            n.prereq,
            n.n_out,
            n.mask_self,
        )
    };
    let mut scan = WhereScan::default();
    let mut p_term_opt = where_scan_init(
        &mut scan,
        Some(p_builder.p_wc.clone()),
        p_src.i_cursor,
        saved_n_eq as i16,
        op_mask,
        Some(p_probe),
    );
    p_new.borrow_mut().r_setup = 0;
    let r_size: LogEst = p_probe.ai_row_log_est[0];
    let r_log_size: LogEst = est_log(r_size);
    loop {
        if rc != SQLITE_OK {
            break;
        }
        let p_term = match p_term_opt.clone() {
            Some(t) => t,
            None => break,
        };
        let mut stop = false;
        // `break 'body` faz o papel do `continue` do C; `stop` faz o papel do `break`.
        'body: {
            let (e_op, wt_flags, prereq_right, truth_prob) = {
                let t = p_term.borrow();
                (t.e_operator, t.wt_flags, t.prereq_right, t.truth_prob)
            };
            // multiplicador do IN() (nIn do C)
            let mut n_in: i32 = 0;
            if (e_op == WO_ISNULL || (wt_flags & TERM_VNULL) != 0)
                && index_column_not_null(p_probe, saved_n_eq as usize)
            {
                // ignora restrições IS [NOT] NULL em colunas NOT NULL
                break 'body;
            }
            if (prereq_right & mask_self) != 0 {
                break 'body;
            }

            // Não permite que o limite superior de uma restrição de intervalo da otimização
            // LIKE se misture com um limite inferior de outra origem
            if (wt_flags & TERM_LIKEOPT) != 0 && e_op == WO_LT {
                break 'body;
            }

            if (p_src.fg.jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0
                && !constraint_compatible_with_outer_join(&p_term.borrow(), p_src)
            {
                break 'body;
            }
            if is_unique_index(p_probe) && saved_n_eq as i32 == p_probe.n_key_col as i32 - 1 {
                p_builder.bld_flags1 |= SQLITE_BLDF1_UNIQUE;
            } else {
                p_builder.bld_flags1 |= SQLITE_BLDF1_INDEXED;
            }
            {
                let mut n = p_new.borrow_mut();
                n.ws_flags = saved_ws_flags;
                {
                    let (c_eq, c_btm, c_top) = add_btree_idx_counts_mut(&mut n);
                    *c_eq = saved_n_eq;
                    *c_btm = saved_n_btm;
                    *c_top = saved_n_top;
                }
                n.n_l_term = saved_n_l_term;
            }
            let needs_grow = {
                let n = p_new.borrow();
                n.n_l_term >= n.n_l_slot
            };
            if needs_grow && add_btree_idx_grow(&db, &p_new) {
                // faltou memória ao tentar aumentar o array aLTerm
                stop = true;
                break 'body;
            }
            add_btree_idx_push(&p_new, Some(p_term.clone()));
            p_new.borrow_mut().prereq = (saved_prereq | prereq_right) & !mask_self;

            if (e_op & WO_IN) != 0 {
                let expr_ref = p_term
                    .borrow()
                    .p_expr
                    .clone()
                    .expect("termo IN sempre tem expressão");
                {
                    let expr = expr_ref.borrow();
                    if expr_use_x_select(&expr) {
                        // "x IN (SELECT ...)": AJUSTE: o SELECT devolve 25 linhas
                        n_in = 46;

                        // A expressão pode ser da forma (x, y) IN (SELECT...). Nesse caso há um
                        // termo separado para cada um de (x) e (y), mas o multiplicador nIn só
                        // deve ser aplicado uma vez, não uma por termo. O laço abaixo confere se
                        // pTerm é o primeiro termo em uso e volta nIn a 0 se não for.
                        let n = p_new.borrow();
                        let last = n.n_l_term as i32 - 1;
                        for i in 0..last {
                            if let Some(t) = &n.a_l_term[i as usize] {
                                if let Some(pe) = &t.borrow().p_expr {
                                    if Rc::ptr_eq(pe, &expr_ref) {
                                        n_in = 0;
                                    }
                                }
                            }
                        }
                    } else if let Some(list) = &expr.x.p_list {
                        // "x IN (valor, valor, ...)"
                        if list.n_expr != 0 {
                            n_in = log_est(list.n_expr as u64) as i32;
                        }
                    }
                }
                if p_probe.has_stat1 && r_log_size >= 10 {
                    // Sejam:
                    //   N = número total de linhas da tabela
                    //   K = número de entradas do lado direito do IN
                    //   M = número de linhas da tabela que casam com os termos à esquerda no
                    //       mesmo índice. Se o IN está na coluna mais à esquerda, M==N.
                    //
                    // Dadas as definições, é melhor omitir o IN da busca no índice e varrer os
                    // M elementos, testando cada linha contra o IN em separado, se:
                    //
                    //        M*log(K) < K*log(N)
                    //
                    // As estimativas de M, K e N podem ser imprecisas, então há uma margem de
                    // segurança de 2 (LogEst: 10) que favorece o IN com o índice, pois usar o
                    // índice tem melhor pior caso. Sem dados reais de sqlite_stat1, sempre se
                    // prefere o índice. Não vale a pena em tabelas muito pequenas (menos de 2
                    // linhas).
                    let m: LogEst = p_probe.ai_row_log_est[saved_n_eq as usize];
                    let log_k: LogEst = est_log(n_in as LogEst);
                    // AJUSTE      v-----  10 para favorecer o IN indexado
                    let x: LogEst = (m as i32 + log_k as i32 + 10 - (n_in + r_log_size as i32)) as LogEst;
                    if x >= 0 {
                        // prefere a busca indexada
                    } else if n_in_mul < 2 && optimization_enabled(&db.borrow(), SQLITE_SEEK_SCAN) {
                        p_new.borrow_mut().ws_flags |= WHERE_IN_SEEKSCAN;
                    } else {
                        // prefere a varredura normal
                        break 'body;
                    }
                }
                p_new.borrow_mut().ws_flags |= WHERE_COLUMN_IN;
            } else if (e_op & (WO_EQ | WO_IS)) != 0 {
                let i_col: i32 = p_probe.ai_column[saved_n_eq as usize] as i32;
                p_new.borrow_mut().ws_flags |= WHERE_COLUMN_EQ;
                if i_col == XN_ROWID as i32
                    || (i_col >= 0 && n_in_mul == 0 && saved_n_eq as i32 == p_probe.n_key_col as i32 - 1)
                {
                    if i_col == XN_ROWID as i32
                        || p_probe.uniq_not_null
                        || (p_probe.n_key_col == 1 && p_probe.on_error != 0 && e_op == WO_EQ)
                    {
                        p_new.borrow_mut().ws_flags |= WHERE_ONEROW;
                    } else {
                        p_new.borrow_mut().ws_flags |= WHERE_UNQ_WANTED;
                    }
                }
                if scan.i_equiv > 1 {
                    p_new.borrow_mut().ws_flags |= WHERE_TRANSCONS;
                }
            } else if (e_op & WO_ISNULL) != 0 {
                p_new.borrow_mut().ws_flags |= WHERE_COLUMN_NULL;
            } else {
                let n_vec_len = where_range_vector_len(
                    &p_parse,
                    p_src.i_cursor,
                    p_probe,
                    saved_n_eq as i32,
                    &p_term.borrow(),
                );
                if (e_op & (WO_GT | WO_GE)) != 0 {
                    {
                        let mut n = p_new.borrow_mut();
                        n.ws_flags |= WHERE_COLUMN_RANGE | WHERE_BTM_LIMIT;
                        *add_btree_idx_counts_mut(&mut n).1 = n_vec_len as u16;
                    }
                    p_btm = Some(p_term.clone());
                    p_top = None;
                    if (wt_flags & TERM_LIKEOPT) != 0 {
                        // Restrições de intervalo vindas da otimização LIKE são sempre usadas
                        // em pares: o termo seguinte da cláusula é o limite superior.
                        let p_wc = p_term
                            .borrow()
                            .p_wc
                            .upgrade()
                            .expect("a cláusula do termo está viva");
                        let pos = p_wc
                            .borrow()
                            .a
                            .iter()
                            .position(|t| Rc::ptr_eq(t, &p_term))
                            .expect("o termo pertence à própria cláusula");
                        let top = p_wc.borrow().a[pos + 1].clone();
                        p_top = Some(top.clone());
                        if add_btree_idx_grow(&db, &p_new) {
                            // falta de memória
                            stop = true;
                            break 'body;
                        }
                        add_btree_idx_push(&p_new, Some(top));
                        let mut n = p_new.borrow_mut();
                        n.ws_flags |= WHERE_TOP_LIMIT;
                        *add_btree_idx_counts_mut(&mut n).2 = 1;
                    }
                } else {
                    let mut n = p_new.borrow_mut();
                    n.ws_flags |= WHERE_COLUMN_RANGE | WHERE_TOP_LIMIT;
                    *add_btree_idx_counts_mut(&mut n).2 = n_vec_len as u16;
                    p_top = Some(p_term.clone());
                    p_btm = if (n.ws_flags & WHERE_BTM_LIMIT) != 0 {
                        n.a_l_term[n.n_l_term as usize - 2].clone()
                    } else {
                        None
                    };
                }
            }

            // Neste ponto p_new.n_out é o número de linhas que se espera visitar na varredura do
            // índice antes de considerar o termo pTerm, ou os valores de nIn e nInMul. Em outras
            // palavras, supondo que todo "x IN(...)" foi trocado por "x = ?". Este bloco atualiza
            // n_out para levar em conta pTerm (mas não nIn/nInMul).
            if (p_new.borrow().ws_flags & WHERE_COLUMN_RANGE) != 0 {
                // Ajusta n_out com dados stat4. Ou, sem dados stat4, com outra estimativa.
                where_range_scan_est(&p_parse, p_builder, p_btm.as_ref(), p_top.as_ref(), &p_new);
            } else {
                let n_eq: usize = {
                    let mut n = p_new.borrow_mut();
                    let c_eq = add_btree_idx_counts_mut(&mut n).0;
                    *c_eq += 1;
                    *c_eq as usize
                };
                let mut n = p_new.borrow_mut();
                if truth_prob <= 0 && p_probe.ai_column[saved_n_eq as usize] >= 0 {
                    n.n_out = (n.n_out as i32 + truth_prob as i32) as LogEst;
                    n.n_out = (n.n_out as i32 - n_in) as LogEst;
                } else {
                    n.n_out = (n.n_out as i32
                        + (p_probe.ai_row_log_est[n_eq] as i32 - p_probe.ai_row_log_est[n_eq - 1] as i32))
                        as LogEst;
                    if (e_op & WO_ISNULL) != 0 {
                        // AJUSTE: sem valor likelihood(), supõe que "col IS NULL" casa o dobro
                        // de linhas de (col=?).
                        n.n_out = (n.n_out as i32 + 10) as LogEst;
                    }
                }
            }

            // Põe em r_cost_idx o custo estimado de visitar as linhas selecionadas no índice.
            // A estimativa é a soma de dois valores:
            //   1.  O custo de uma busca por chave para achar a primeira entrada que casa
            //   2.  Avançar no índice n_out vezes para achar as demais entradas que casam.
            let n_out_now = p_new.borrow().n_out as i32;
            let mut r_cost_idx: LogEst;
            if p_probe.idx_type == SQLITE_IDXTYPE_IPK {
                // sz_idx_row é baixo numa tabela IPK porque as páginas internas são pequenas.
                // Então sz_idx_row estima bem o custo da busca. Mas as páginas folha têm
                // tamanho cheio, logo sz_idx_row subestimaria muito o custo da varredura.
                r_cost_idx = (n_out_now + 16) as LogEst;
            } else {
                let sz_tab_row = p_src
                    .p_tab
                    .as_ref()
                    .expect("item FROM com índice tem tabela")
                    .borrow()
                    .sz_tab_row as i32;
                r_cost_idx = (n_out_now + 1 + (15 * p_probe.sz_idx_row as i32) / sz_tab_row) as LogEst;
            }
            r_cost_idx = log_est_add(r_log_size, r_cost_idx);

            // Estima o custo de rodar o laço. Se todos os dados vêm do índice, é só o custo da
            // busca e da varredura no índice. Mas se parte vem da tabela principal, soma-se o
            // custo de fazer n_out buscas na tabela principal para achar a linha que corresponde
            // à entrada do índice.
            let n_out_unadjusted: LogEst;
            {
                let mut n = p_new.borrow_mut();
                n.r_run = r_cost_idx;
                if (n.ws_flags & (WHERE_IDX_ONLY | WHERE_IPK | WHERE_EXPRIDX)) == 0 {
                    n.r_run = log_est_add(n.r_run, (n.n_out as i32 + 16) as LogEst);
                }

                n_out_unadjusted = n.n_out;
                n.r_run = (n.r_run as i32 + n_in_mul as i32 + n_in) as LogEst;
                n.n_out = (n.n_out as i32 + n_in_mul as i32 + n_in) as LogEst;
            }
            where_loop_output_adjust(&p_builder.p_wc, &p_new, r_size);
            rc = where_loop_insert(p_builder, &p_new);

            {
                let mut n = p_new.borrow_mut();
                if (n.ws_flags & WHERE_COLUMN_RANGE) != 0 {
                    n.n_out = saved_n_out;
                } else {
                    n.n_out = n_out_unadjusted;
                }
            }

            let go_deeper = {
                let n = p_new.borrow();
                let n_eq_now = add_btree_idx_counts(&n).0;
                (n.ws_flags & WHERE_TOP_LIMIT) == 0
                    && n_eq_now < p_probe.n_column
                    && (n_eq_now < p_probe.n_key_col || p_probe.idx_type != SQLITE_IDXTYPE_PRIMARYKEY)
            };
            if go_deeper {
                let n_eq_now = add_btree_idx_counts(&p_new.borrow()).0;
                if n_eq_now > 3 {
                    progress_check(&p_parse);
                }
                let _ = where_loop_add_btree_index(
                    p_builder,
                    p_src,
                    p_probe,
                    (n_in_mul as i32 + n_in) as LogEst,
                );
            }
            p_new.borrow_mut().n_out = saved_n_out;
        }
        if stop {
            break;
        }
        p_term_opt = where_scan_next(&mut scan);
    }
    {
        let mut n = p_new.borrow_mut();
        n.prereq = saved_prereq;
        {
            let (c_eq, c_btm, c_top) = add_btree_idx_counts_mut(&mut n);
            *c_eq = saved_n_eq;
            *c_btm = saved_n_btm;
            *c_top = saved_n_top;
        }
        n.n_skip = saved_n_skip;
        n.ws_flags = saved_ws_flags;
        n.n_out = saved_n_out;
        n.n_l_term = saved_n_l_term;
    }

    // Considera usar um skip-scan se não há restrições da cláusula WHERE para os termos mais à
    // esquerda do índice, e se o número médio de repetições nos termos mais à esquerda é pelo
    // menos 18.
    //
    // O número mágico 18 foi escolhido porque varrer 17 linhas quase sempre é mais rápido que
    // uma busca no índice (embora, se o índice tem menos de 2^17 linhas, se suponha o contrário
    // em outras partes do código). E, mesmo que não seja, não deve ser muito mais lento. Por
    // outro lado, as buscas extras podem sair bem mais caras. (42 == log_est(18))
    let mut try_skip_scan = saved_n_eq == saved_n_skip
        && saved_n_eq as i32 + 1 < p_probe.n_key_col as i32
        && saved_n_eq == p_new.borrow().n_l_term
        && !p_probe.no_skip_scan
        && p_probe.has_stat1
        && optimization_enabled(&db.borrow(), SQLITE_SKIP_SCAN)
        && p_probe.ai_row_log_est[saved_n_eq as usize + 1] >= 42; // AJUSTE: mínimo para skip-scan
    if try_skip_scan {
        // a atribuição a rc faz parte da cadeia de condições do C
        let n_wanted = p_new.borrow().n_l_term as i32 + 1;
        rc = where_loop_resize(&db, &mut p_new.borrow_mut(), n_wanted);
        try_skip_scan = rc == SQLITE_OK;
    }
    if try_skip_scan {
        let mut n_iter: LogEst;
        {
            let mut n = p_new.borrow_mut();
            *add_btree_idx_counts_mut(&mut n).0 += 1;
            n.n_skip += 1;
        }
        add_btree_idx_push(&p_new, None);
        {
            let mut n = p_new.borrow_mut();
            n.ws_flags |= WHERE_SKIPSCAN;
            n_iter = (p_probe.ai_row_log_est[saved_n_eq as usize] as i32
                - p_probe.ai_row_log_est[saved_n_eq as usize + 1] as i32) as LogEst;
            n.n_out = (n.n_out as i32 - n_iter as i32) as LogEst;
        }
        // AJUSTE: por causa das incertezas nas estimativas de consultas skip-scan, soma um fator
        // de 1,375 para tornar o skip-scan um pouco menos provável.
        n_iter = (n_iter as i32 + 5) as LogEst;
        let _ = where_loop_add_btree_index(
            p_builder,
            p_src,
            p_probe,
            (n_iter as i32 + n_in_mul as i32) as LogEst,
        );
        let mut n = p_new.borrow_mut();
        n.n_out = saved_n_out;
        *add_btree_idx_counts_mut(&mut n).0 = saved_n_eq;
        n.n_skip = saved_n_skip;
        n.ws_flags = saved_ws_flags;
    }

    rc
}


// ---- part_009.rs ----

/// Retorna verdadeiro se é possível que `p_index` seja útil para implementar a cláusula
/// ORDER BY de `p_builder`.
///
/// Retorna falso se `p_builder` não tem ORDER BY ou se não há como `p_index` ser útil
/// para implementá-la.
fn index_might_help_with_order_by(
    p_builder: &WhereLoopBuilder,
    p_index: &Index,
    i_cursor: i32,
) -> i32 {
    if p_index.b_unordered {
        return 0;
    }
    let p_w_info = p_builder.p_w_info.borrow();
    let ob_ref = match &p_w_info.p_order_by {
        Some(ob) => ob.clone(),
        None => return 0,
    };
    let p_ob = ob_ref.borrow();
    let mut ii: i32 = 0;
    while ii < p_ob.n_expr {
        let p_expr = match p_ob.a[ii as usize].p_expr.as_deref() {
            Some(e) => expr_skip_collate_and_likely(e),
            None => None,
        };
        // NEVER(p_expr==0)
        let p_expr = match p_expr {
            Some(e) => e,
            None => {
                ii += 1;
                continue;
            }
        };
        if (p_expr.op == TK_COLUMN || p_expr.op == TK_AGG_COLUMN) && p_expr.i_table == i_cursor {
            if p_expr.i_column < 0 {
                return 1;
            }
            let mut jj: usize = 0;
            while jj < p_index.n_key_col as usize {
                if p_expr.i_column == p_index.ai_column[jj] as i32 {
                    return 1;
                }
                jj += 1;
            }
        } else if let Some(a_col_expr) = p_index.a_col_expr.as_deref() {
            let mut jj: usize = 0;
            while jj < p_index.n_key_col as usize {
                if p_index.ai_column[jj] != XN_EXPR {
                    jj += 1;
                    continue;
                }
                let p_other = a_col_expr.a[jj].p_expr.as_deref().unwrap();
                if expr_compare_skip(p_expr, p_other, i_cursor) == 0 {
                    return 1;
                }
                jj += 1;
            }
        }
        ii += 1;
    }
    0
}

/// Verifica se um índice parcial com `p_where` pode ser usado na consulta atual.
/// Retorna verdadeiro se pode e falso se não.
fn where_usable_partial_index(
    i_tab: i32,       // A tabela para a qual queremos um índice
    jointype: u8,     // Os flags JT_* do join
    p_wc: &WhereClause, // A cláusula WHERE da consulta
    p_where: &Expr,   // A cláusula WHERE do índice parcial
) -> i32 {
    if (jointype & JT_LTORJ) != 0 {
        return 0;
    }
    let p_w_info = p_wc.p_w_info.upgrade().unwrap();
    let mut p_parse: Option<ParseRef> = Some(p_w_info.borrow().p_parse.clone());
    let mut p_where = p_where;
    while p_where.op == TK_AND {
        if where_usable_partial_index(i_tab, jointype, p_wc, p_where.p_left.as_deref().unwrap())
            == 0
        {
            return 0;
        }
        p_where = p_where.p_right.as_deref().unwrap();
    }
    let enable_qpsg = {
        let parse_ref = p_parse.as_ref().unwrap().borrow();
        let db = parse_ref.db.upgrade().unwrap();
        let flags = db.borrow().flags;
        (flags & SQLITE_ENABLE_QPSG) != 0
    };
    if enable_qpsg {
        p_parse = None;
    }
    let mut i: i32 = 0;
    while i < p_wc.n_term {
        let p_term = p_wc.a[i as usize].borrow();
        let p_expr_ref = p_term.p_expr.as_ref().unwrap().clone();
        let p_expr = p_expr_ref.borrow();
        if (!expr_has_property(&p_expr, EP_OUTER_ON) || p_expr.w.i_join == i_tab)
            && ((jointype & JT_OUTER) == 0 || expr_has_property(&p_expr, EP_OUTER_ON))
            && expr_implies_expr(p_parse.as_ref(), &p_expr, p_where, i_tab) != 0
            && (p_term.wt_flags & TERM_VNULL) == 0
        {
            return 1;
        }
        i += 1;
    }
    0
}

/// `p_idx` é um índice que contém expressões. Verifica se alguma das expressões do
/// índice casa com a expressão `p_expr`.
fn expr_is_covered_by_index(p_expr: &Expr, p_idx: &Index, i_tab_cur: i32) -> i32 {
    let mut i: usize = 0;
    while i < p_idx.n_column as usize {
        if p_idx.ai_column[i] == XN_EXPR {
            let p_other = p_idx.a_col_expr.as_deref().unwrap().a[i]
                .p_expr
                .as_deref()
                .unwrap();
            if expr_compare(None, p_expr, p_other, i_tab_cur) == 0 {
                return 1;
            }
        }
        i += 1;
    }
    0
}

/// Estrutura passada ao callback do Walker de `where_is_covering_index`.
pub struct CoveringIndexCheck {
    /// O índice
    pub p_idx: IndexRef,
    /// Número do cursor da tabela correspondente
    pub i_tab_cur: i32,
    /// Usa uma expressão indexada
    pub b_expr: u8,
    /// Usa uma coluna não indexada fora de uma expressão indexada
    pub b_unidx: u8,
}

/// A informação recebida está em `p_walk.u` (variante `CovIdxCk`). Chamada de `p_ck`.
///
/// Se o nó Expr referencia a tabela com cursor `p_ck.i_tab_cur`, garante que a coluna
/// seja coberta pelo índice `p_ck.p_idx`. Sabemos que todas as colunas menores que 63
/// (na verdade BMS-1) são cobertas, então não precisam ser verificadas, mas as de 63 em
/// diante precisam.
///
/// Se o índice não cobre a coluna, liga `b_unidx` e devolve WRC_ABORT para parar a busca.
///
/// Se este nó não refuta que o índice possa ser cobridor, devolve WRC_CONTINUE.
///
/// Se `p_ck.p_idx` contém expressões indexadas e uma delas casa com `p_expr`, poda a busca.
fn where_is_covering_index_walk_callback(p_walk: &mut Walker, p_expr: &mut Expr) -> i32 {
    let (p_idx_ref, i_tab_cur) = match &p_walk.u {
        WalkerU::CovIdxCk(ck) => (ck.p_idx.clone(), ck.i_tab_cur),
        _ => return WRC_CONTINUE,
    };
    let p_idx = p_idx_ref.borrow();
    if p_expr.op == TK_COLUMN || p_expr.op == TK_AGG_COLUMN {
        // if( pExpr->iColumn<(BMS-1) && pIdx->bHasExpr==0 ) return WRC_Continue;
        if p_expr.i_table != i_tab_cur {
            return WRC_CONTINUE;
        }
        let n_column = p_idx.n_column as usize;
        let mut i: usize = 0;
        while i < n_column {
            if p_idx.ai_column[i] as i32 == p_expr.i_column {
                return WRC_CONTINUE;
            }
            i += 1;
        }
        if let WalkerU::CovIdxCk(ck) = &mut p_walk.u {
            ck.b_unidx = 1;
        }
        return WRC_ABORT;
    } else if p_idx.b_has_expr && expr_is_covered_by_index(p_expr, &p_idx, i_tab_cur) != 0 {
        if let WalkerU::CovIdxCk(ck) = &mut p_walk.u {
            ck.b_expr = 1;
        }
        return WRC_PRUNE;
    }
    WRC_CONTINUE
}

/// `p_idx` é um índice que cobre todas as colunas de número baixo usadas por
/// `p_w_info.p_select` (colunas de 0 a 62) ou um índice com termos de expressão. Logo, não
/// dá para saber se é cobridor pelas máscaras `colUsed`: é preciso fazer uma busca para ver
/// se o índice é cobridor. Esta rotina faz essa busca.
///
/// O valor devolvido é um destes:
///
///      0                O índice definitivamente não é cobridor
///
///      WHERE_IDX_ONLY   O índice definitivamente é cobridor
///
///      WHERE_EXPRIDX    O índice provavelmente é cobridor, mas é difícil determinar com
///                       precisão por causa das expressões indexadas. Pontua como
///                       cobridor, mas mantém a tabela principal aberta por garantia.
///
/// Esta rotina é uma otimização. É sempre seguro devolver zero. Mas devolver um dos outros
/// dois valores quando zero seria o correto pode levar a bytecode incorreto e falhas de
/// asserção.
#[inline(never)]
fn where_is_covering_index(
    p_w_info: &WhereInfo, // O contexto da cláusula WHERE
    p_idx: &IndexRef,     // Índice que está sendo testado
    i_tab_cur: i32,       // Cursor da tabela sendo indexada
) -> u32 {
    let p_select = match &p_w_info.p_select {
        Some(s) => s.clone(),
        None => {
            // Não temos acesso à consulta inteira, então não dá para verificar se p_idx é
            // cobridor. Assume que não é.
            return 0;
        }
    };
    {
        let idx = p_idx.borrow();
        if !idx.b_has_expr {
            let mut i: usize = 0;
            while i < idx.n_column as usize {
                if (idx.ai_column[i] as i64) >= (BMS as i64) - 1 {
                    break;
                }
                i += 1;
            }
            if i >= idx.n_column as usize {
                // p_idx não indexa nenhuma coluna maior que 62, mas sabemos pelo colMask
                // que colunas maiores que 62 são usadas, então não é um índice cobridor
                return 0;
            }
        }
    }
    let ck = CoveringIndexCheck {
        p_idx: p_idx.clone(),
        i_tab_cur,
        b_expr: 0,
        b_unidx: 0,
    };
    let mut w = Walker {
        p_parse: None,
        x_expr_callback: Some(where_is_covering_index_walk_callback),
        x_select_callback: Some(select_walk_noop),
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WalkerU::CovIdxCk(Box::new(ck)),
    };
    walk_select(&mut w, &mut p_select.borrow_mut());
    let (b_unidx, b_expr) = match &w.u {
        WalkerU::CovIdxCk(ck) => (ck.b_unidx, ck.b_expr),
        _ => (0, 0),
    };
    let rc: u32;
    if b_unidx != 0 {
        rc = 0;
    } else if b_expr != 0 {
        rc = WHERE_EXPRIDX;
    } else {
        rc = WHERE_IDX_ONLY;
    }
    rc
}

/// Esta é uma rotina de callback de `parser_add_cleanup()` chamada para liberar a lista
/// `Parse.p_idx_epr` quando o objeto Parse é destruído. `pp` é a cabeça da lista.
fn where_indexed_expr_cleanup(db: &Sqlite3Ref, pp: &mut Option<Box<IndexedExpr>>) {
    while pp.is_some() {
        let mut p = pp.take().unwrap();
        *pp = p.p_ie_next.take();
        let p_expr = std::mem::replace(&mut p.p_expr, Box::new(Expr::default()));
        expr_delete(db, Some(p_expr));
        // db_free_nn(db, p): o Box é liberado ao sair do escopo
        drop(p);
    }
}

/// Esta função é chamada para um índice parcial (com cláusula WHERE) em dois cenários.
/// Nos dois casos determina se a cláusula WHERE do índice implica que uma coluna da tabela
/// pode ser substituída com segurança por uma expressão constante. Por exemplo, neste
/// SELECT:
///
///   CREATE INDEX i1 ON t1(b, c) WHERE a=<expr>;
///   SELECT a, b, c FROM t1 WHERE a=<expr> AND b=?;
///
/// O "a" da lista de seleção pode ser substituído por <expr> se:
///
///    (a) <expr> é uma expressão constante, e
///    (b) A comparação (a=<expr>) usa a sequência de colação BINARY, e
///    (c) A coluna "a" tem afinidade diferente de NONE ou BLOB.
///
/// Se o argumento `p_item` é None, então `p_mask` não pode ser None. Neste caso a função
/// está sendo chamada para determinar se `p_idx` é um índice cobridor. Ela zera os bits de
/// `p_mask` correspondentes a colunas que podem ser substituídas por constantes como
/// descrito acima.
///
/// Caso contrário, se `p_item` não é None, a função está sendo chamada para gerar código de
/// um loop que usa o índice `p_idx`. Neste caso adiciona entradas à lista
/// `Parse.p_idx_part_expr` para cada coluna que pode ser substituída por uma constante.
fn where_part_idx_expr(
    p_parse: &ParseRef,          // Contexto de análise
    p_idx: &Index,               // Índice parcial em processamento
    p_part: &Expr,               // Cláusula WHERE em processamento
    mut p_mask: Option<&mut Bitmask>, // Máscara onde zerar bits
    i_idx_cur: i32,              // Número do cursor do índice
    p_item: Option<&SrcItem>,    // A entrada da cláusula FROM da tabela
) {
    debug_assert!(p_item.is_none() || (p_item.unwrap().fg.jointype & JT_RIGHT) == 0);
    debug_assert!(
        (p_item.is_none() || p_mask.is_none()) && (p_mask.is_some() || p_item.is_some())
    );

    let mut p_part = p_part;
    if p_part.op == TK_AND {
        where_part_idx_expr(
            p_parse,
            p_idx,
            p_part.p_right.as_deref().unwrap(),
            p_mask.as_deref_mut(),
            i_idx_cur,
            p_item,
        );
        p_part = p_part.p_left.as_deref().unwrap();
    }

    if p_part.op == TK_EQ || p_part.op == TK_IS {
        let p_left = p_part.p_left.as_deref().unwrap();
        let p_right = p_part.p_right.as_deref().unwrap();
        let aff: u8;

        if p_left.op != TK_COLUMN {
            return;
        }
        if expr_is_constant(None, p_right) == 0 {
            return;
        }
        if is_binary(expr_compare_coll_seq(p_parse, p_part).as_ref()) == 0 {
            return;
        }
        if p_left.i_column < 0 {
            return;
        }
        aff = p_idx.p_table.upgrade().unwrap().borrow().a_col[p_left.i_column as usize].affinity;
        if aff >= SQLITE_AFF_TEXT {
            if let Some(p_item) = p_item {
                let db = p_parse.borrow().db.upgrade().unwrap();
                let b_null_row = (p_item.fg.jointype & (JT_LEFT | JT_LTORJ)) != 0;
                let p_ie_next = p_parse.borrow_mut().p_idx_part_expr.take();
                let first = p_ie_next.is_none();
                let p = IndexedExpr {
                    p_expr: expr_dup(&db, p_right, 0).unwrap(),
                    i_data_cur: p_item.i_cursor,
                    i_idx_cur,
                    i_idx_col: p_left.i_column,
                    b_maybe_null_row: b_null_row as u8,
                    aff,
                    p_ie_next,
                };
                p_parse.borrow_mut().p_idx_part_expr = Some(Box::new(p));
                if first {
                    // O objeto do C (ponteiro para Parse.pIdxPartExpr) é capturado como
                    // referência fraca ao Parse, para a closure não formar ciclo
                    let p_arg = Rc::downgrade(p_parse);
                    parser_add_cleanup(
                        p_parse,
                        Box::new(move |db: &Sqlite3Ref| {
                            if let Some(p_parse) = p_arg.upgrade() {
                                let mut head = p_parse.borrow_mut().p_idx_part_expr.take();
                                where_indexed_expr_cleanup(db, &mut head);
                            }
                        }),
                    );
                }
            } else if p_left.i_column < (BMS as i32) - 1 {
                if let Some(m) = p_mask {
                    *m &= !((1 as Bitmask) << p_left.i_column);
                }
            }
        }
    }
}

/// Acesso aos campos `btree` da união `u` de um WhereLoop: devolve `(n_eq, n_btm, n_top,
/// p_index)`. Só vale para loops de tabela btree (o C acessa `u.btree.*` direto).
fn where_loop_btree_mut(
    u: &mut WhereLoopUnion,
) -> (&mut u16, &mut u16, &mut u16, &mut Option<IndexRef>) {
    match u {
        WhereLoopUnion::Btree { n_eq, n_btm, n_top, p_index, .. } => (n_eq, n_btm, n_top, p_index),
        _ => panic!("WhereLoop sem a variante btree"),
    }
}

/// Adiciona todos os objetos WhereLoop de uma única tabela do join, onde a tabela é
/// identificada por `p_builder.p_new.i_tab`. Essa tabela é garantidamente uma tabela
/// b-tree, não uma tabela virtual.
///
/// Os custos (WhereLoop.r_run) dos loops b-tree adicionados por esta função são
/// calculados assim:
///
/// Para uma varredura completa, supondo que a tabela (ou índice) tem n_row linhas:
///
///     custo = n_row * 3.0                    // varredura da tabela inteira
///     custo = n_row * K                      // varredura de índice cobridor
///     custo = n_row * (K+3.0)                // varredura de índice não cobridor
///
/// onde K é um valor entre 1.1 e 3.0 definido pelo tamanho médio estimado relativo dos
/// registros do índice e da tabela.
///
/// Para uma varredura de índice, onde n_visit é o número de linhas do índice visitadas e
/// n_seek o número de operações de busca necessárias no b-tree do índice:
///
///     custo = n_seek * (log(n_row) + K * n_visit)          // índice cobridor
///     custo = n_seek * (log(n_row) + (K+3.0) * n_visit)    // índice não cobridor
///
/// Normalmente n_seek é 1. Valores maiores vêm de termos "x IN (....)" usados no lugar de
/// "x=?", ou de termos implícitos "x IN (SELECT x FROM tbl)" adicionados em skip-scans.
///
/// Os valores estimados (n_row, n_visit, n_seek) costumam ter muita incerteza. Por isso a
/// pontuação é pensada para escolher planos que "façam o menor mal" se as estimativas
/// forem imprecisas. Por exemplo, o fator log(n_row) é omitido da varredura de índice não
/// cobridor para inclinar a pontuação a favor de usar um índice, já que o pior caso de
/// usar um índice é muito melhor que o pior caso de uma varredura completa da tabela.
fn where_loop_add_btree(
    p_builder: &mut WhereLoopBuilder, // Informação da cláusula WHERE
    m_prereq: Bitmask,                // Pré-requisitos extras para usar esta tabela
) -> i32 {
    let p_probe_start: Option<IndexRef>; // Um índice que estamos avaliando
    let mut rc: i32 = SQLITE_OK; // Código de retorno
    let mut i_sort_idx: i32 = 1; // Número do índice
    let mut r_size: LogEst; // número de linhas da tabela

    let p_new: WhereLoopRef = p_builder.p_new.clone(); // WhereLoop modelo
    let p_w_info: WhereInfoRef = p_builder.p_w_info.clone(); // Contexto de análise do WHERE
    let p_tab_list: SrcListRef = p_w_info.borrow().p_tab_list.clone(); // A cláusula FROM
    let p_tab_list_b = p_tab_list.borrow();
    let i_tab_new = p_new.borrow().i_tab as usize;
    let p_src: &SrcItem = &p_tab_list_b.a[i_tab_new]; // O termo b-tree da cláusula FROM
    let p_tab: TableRef = p_src.p_tab.as_ref().unwrap().clone(); // Tabela consultada
    let p_wc: WhereClauseRef = p_builder.p_wc.clone(); // A cláusula WHERE analisada
    let p_parse: ParseRef = p_w_info.borrow().p_parse.clone();
    let wctrl_flags = p_w_info.borrow().wctrl_flags;
    debug_assert!(!is_virtual(&p_tab.borrow()));

    if p_src.fg.is_indexed_by != 0 {
        debug_assert!(p_src.fg.is_cte == 0);
        // Uma cláusula INDEXED BY especifica um índice particular a usar
        p_probe_start = match &p_src.u2 {
            SrcItemU2::IBIndex(ix) => Some(ix.clone()),
            _ => None,
        };
    } else if !has_rowid(&p_tab.borrow()) {
        p_probe_start = p_tab.borrow().p_index.clone();
    } else {
        // Não há cláusula INDEXED BY. Cria um objeto Index falso, `s_pk`, para representar
        // o índice da chave primária rowid. Faz desse índice falso o primeiro de uma cadeia
        // de objetos Index com todos os índices reais a seguir.
        let p_first = p_tab.borrow().p_index.clone(); // Primeiro dos índices reais da tabela
        let n_row_log_est = p_tab.borrow().n_row_log_est;
        let s_pk = Index {
            z_name: Vec::new(),
            ai_column: vec![-1],
            ai_row_log_est: vec![n_row_log_est, 0],
            p_table: Rc::downgrade(&p_tab),
            z_col_aff: Vec::new(),
            // Os índices reais só são considerados se o qualificador NOT INDEXED é omitido
            // da cláusula FROM
            p_next: if p_src.fg.not_indexed == 0 { p_first } else { None },
            p_schema: None,
            a_sort_order: Vec::new(),
            az_coll: Vec::new(),
            p_part_idx_where: None,
            a_col_expr: None,
            tnum: 0,
            sz_idx_row: 3, // TUNING: linhas internas de tabela IPK são muito pequenas
            n_key_col: 1,
            n_column: 1,
            on_error: OE_REPLACE,
            idx_type: SQLITE_IDXTYPE_IPK,
            b_unordered: false,
            uniq_not_null: false,
            is_resized: false,
            is_covering: false,
            no_skip_scan: false,
            has_stat1: false,
            b_low_qual: false,
            b_no_query: false,
            b_asc_key_bug: false,
            b_has_vcol: false,
            b_has_expr: false,
            n_sample: 0,
            mx_sample: 0,
            n_sample_col: 0,
            a_avg_eq: Vec::new(),
            a_sample: Vec::new(),
            ai_row_est: Vec::new(),
            n_row_est0: 0,
            col_not_idxed: 0,
        };
        p_probe_start = Some(Rc::new(RefCell::new(s_pk)));
    }
    r_size = p_tab.borrow().n_row_log_est;

    // Índices automáticos
    let auto_db_flags = p_parse.borrow().db.upgrade().unwrap().borrow().flags;
    if p_builder.p_or_set.is_none() // Não faz parte de uma otimização de OR
        && (wctrl_flags & (WHERE_RIGHT_JOIN | WHERE_OR_SUBCLAUSE)) == 0
        && (auto_db_flags & SQLITE_AUTO_INDEX) != 0
        && p_src.fg.is_indexed_by == 0 // Sem cláusula INDEXED BY
        && p_src.fg.not_indexed == 0 // Sem cláusula NOT INDEXED
        && has_rowid(&p_tab.borrow()) // Não é tabela WITHOUT ROWID. (FIXME: por quê não?)
        && p_src.fg.is_correlated == 0 // Não é subconsulta correlacionada
        && p_src.fg.is_recursive == 0 // Não é CTE recursiva
        && (p_src.fg.jointype & JT_RIGHT) == 0 // Não é a tabela direita de um RIGHT JOIN
    {
        // Gera WhereLoops de índice automático
        let n_term_end = p_wc.borrow().n_term;
        let r_log_size: LogEst = est_log(r_size); // Logaritmo do número de linhas da tabela
        let mut i_term: i32 = 0;
        while rc == SQLITE_OK && i_term < n_term_end {
            let p_term: WhereTermRef = p_wc.borrow().a[i_term as usize].clone();
            i_term += 1;
            if (p_term.borrow().prereq_right & p_new.borrow().mask_self) != 0 {
                continue;
            }
            if term_can_drive_index(&p_term.borrow(), p_src, 0) != 0 {
                let mut n = p_new.borrow_mut();
                {
                    let (n_eq, _, _, p_index) = where_loop_btree_mut(&mut n.u);
                    *n_eq = 1;
                    *p_index = None;
                }
                n.n_skip = 0;
                n.n_l_term = 1;
                n.a_l_term[0] = Some(p_term.clone());
                // TUNING: O custo único de calcular o índice automático é estimado em
                // X*N*log2(N), onde N é o número de linhas da tabela indexada e X é 7
                // (LogEst=28) para tabelas normais ou 0.5 (LogEst=-10) para views e
                // subconsultas. X é menor para views e subconsultas para que o planejador
                // seja mais agressivo ao gerar índices automáticos para esses objetos,
                // já que não há como adicionar índices do schema em subconsultas e views.
                n.r_setup = r_log_size + r_size;
                if !is_view(&p_tab.borrow()) && (p_tab.borrow().tab_flags & TF_EPHEMERAL) == 0 {
                    n.r_setup += 28;
                } else {
                    n.r_setup -= 25; // Custo de preparação muito reduzido para índices
                                     // automáticos em materializações efêmeras de views
                }
                // ApplyCostMultiplier(n.r_setup, costMult): SQLITE_ENABLE_COSTMULT não
                // está ligado no Debian 13, é um nada
                if n.r_setup < 0 {
                    n.r_setup = 0;
                }
                // TUNING: Cada busca no índice rende 20 linhas da tabela. É mais que o
                // palpite usual de 10 linhas, já que não há como saber o quão seletivo o
                // índice será. Não seria irracional tornar este valor bem maior.
                n.n_out = 43; // 43==log_est(20)
                n.r_run = log_est_add(r_log_size, n.n_out);
                n.ws_flags = WHERE_AUTO_INDEX;
                n.prereq = m_prereq | p_term.borrow().prereq_right;
                drop(n);
                rc = where_loop_insert(p_builder, &p_new);
            }
        }
    }

    // Percorre todos os índices. Se houve cláusula INDEXED BY, só considera o índice
    // `p_probe`. O `continue` do C vira `break 'body` (o avanço do `for` roda depois).
    let mut p_probe: Option<IndexRef> = p_probe_start;
    'probe: while rc == SQLITE_OK && p_probe.is_some() {
        let p_probe_ref: IndexRef = p_probe.clone().unwrap();
        'body: {
            let probe = p_probe_ref.borrow();
            if let Some(p_part_where) = probe.p_part_idx_where.as_deref() {
                if where_usable_partial_index(
                    p_src.i_cursor,
                    p_src.fg.jointype,
                    &p_wc.borrow(),
                    p_part_where,
                ) == 0
                {
                    // Índice parcial inadequado para esta consulta (ticket 98d973b8f5)
                    break 'body;
                }
            }
            if probe.b_no_query {
                break 'body;
            }
            r_size = probe.ai_row_log_est[0];
            {
                let mut n = p_new.borrow_mut();
                {
                    let (n_eq, n_btm, n_top, p_index) = where_loop_btree_mut(&mut n.u);
                    *n_eq = 0;
                    *n_btm = 0;
                    *n_top = 0;
                    *p_index = Some(p_probe_ref.clone());
                }
                n.n_skip = 0;
                n.n_l_term = 0;
                n.i_sort_idx = 0;
                n.r_setup = 0;
                n.prereq = m_prereq;
                n.n_out = r_size;
            }
            let b: i32 = index_might_help_with_order_by(&*p_builder, &probe, p_src.i_cursor);

            // As flags ONEPASS_DESIRED nunca ocorrem junto com ORDER BY
            debug_assert!((wctrl_flags & WHERE_ONEPASS_DESIRED) == 0 || b == 0);
            if probe.idx_type == SQLITE_IDXTYPE_IPK {
                // Índice de chave primária inteira
                {
                    let mut n = p_new.borrow_mut();
                    n.ws_flags = WHERE_IPK;

                    // Varredura completa da tabela
                    n.i_sort_idx = (if b != 0 { i_sort_idx } else { 0 }) as u8;
                    // TUNING: O custo da varredura completa da tabela é 3.0*N. O fator 3.0
                    // é um custo extra para desencorajar varreduras completas, já que
                    // buscas por índice têm melhor desempenho no pior caso se nossos
                    // palpites de estatísticas estiverem errados. (O ajuste para 2.75 com
                    // STAT4 não existe: SQLITE_ENABLE_STAT4 está desligado no Debian 13.)
                    n.r_run = r_size + 16;
                    // ApplyCostMultiplier(n.r_run, costMult): nada, sem COSTMULT
                }
                where_loop_output_adjust(&p_wc, &p_new, r_size);
                rc = where_loop_insert(p_builder, &p_new);
                p_new.borrow_mut().n_out = r_size;
                if rc != 0 {
                    break 'probe;
                }
            } else {
                let mut m: Bitmask;
                if probe.is_covering {
                    m = 0;
                    p_new.borrow_mut().ws_flags = WHERE_IDX_ONLY | WHERE_INDEXED;
                } else {
                    m = p_src.col_used & probe.col_not_idxed;
                    if let Some(p_part_where) = probe.p_part_idx_where.as_deref() {
                        where_part_idx_expr(&p_parse, &probe, p_part_where, Some(&mut m), 0, None);
                    }
                    p_new.borrow_mut().ws_flags = WHERE_INDEXED;
                    if m == TOPBIT || (probe.b_has_expr && !probe.b_has_vcol && m != 0) {
                        let is_cov: u32 =
                            where_is_covering_index(&p_w_info.borrow(), &p_probe_ref, p_src.i_cursor);
                        if is_cov == 0 {
                            // -> o índice não é cobridor segundo where_is_covering_index()
                            debug_assert!(m != 0);
                        } else {
                            m = 0;
                            p_new.borrow_mut().ws_flags |= is_cov;
                            // Se is_cov & WHERE_IDX_ONLY: é um índice de expressão cobridor;
                            // senão (is_cov==WHERE_EXPRIDX): talvez seja um índice de
                            // expressão cobridor, segundo where_is_covering_index()
                        }
                    } else if m == 0
                        && (has_rowid(&p_tab.borrow())
                            || p_w_info.borrow().p_select.is_some()
                            || fault_sim(700) != 0)
                    {
                        // -> é um índice cobridor segundo as máscaras de bits
                        p_new.borrow_mut().ws_flags = WHERE_IDX_ONLY | WHERE_INDEXED;
                    }
                }

                // Varredura completa via índice
                let sz_tab_row = p_tab.borrow().sz_tab_row;
                let cis_enabled = {
                    let db = p_parse.borrow().db.upgrade().unwrap();
                    let enabled = optimization_enabled(&db.borrow(), SQLITE_COVER_IDX_SCAN);
                    enabled
                };
                if b != 0
                    || !has_rowid(&p_tab.borrow())
                    || probe.p_part_idx_where.is_some()
                    || p_src.fg.is_indexed_by != 0
                    || (m == 0
                        && !probe.b_unordered
                        && probe.sz_idx_row < sz_tab_row
                        && (wctrl_flags & WHERE_ONEPASS_DESIRED) == 0
                        && global_config().b_use_cis != 0
                        && cis_enabled)
                {
                    let mut r_run: LogEst = (r_size as i32
                        + 1
                        + (15 * probe.sz_idx_row as i32) / sz_tab_row as i32)
                        as LogEst;
                    {
                        let mut n = p_new.borrow_mut();
                        n.i_sort_idx = (if b != 0 { i_sort_idx } else { 0 }) as u8;

                        // O custo de visitar as linhas do índice é N*K, onde K está entre
                        // 1.1 e 3.0, conforme os tamanhos relativos das linhas do índice e
                        // da tabela.
                        n.r_run = r_run;
                    }
                    if m != 0 {
                        // Se é uma varredura de índice não cobridor, soma o custo das
                        // buscas na tabela. O custo será 3x o número de buscas. Leva em
                        // conta termos da cláusula WHERE que podem ser satisfeitos só com
                        // o índice, sem busca na tabela.
                        let mut n_lookup: LogEst = r_size + 16; // Custo base: N*3
                        let i_cur = p_src.i_cursor;
                        let w_info_b = p_w_info.borrow();
                        let p_wc2 = &w_info_b.s_wc;
                        let mut ii: i32 = 0;
                        while ii < p_wc2.n_term {
                            let p_term = p_wc2.a[ii as usize].borrow();
                            let p_term_expr = p_term.p_expr.as_ref().unwrap().borrow();
                            if expr_covered_by_index(&p_term_expr, i_cur, &probe) == 0 {
                                break;
                            }
                            // p_term pode ser avaliado só com o índice. Então reduz o
                            // número esperado de buscas na tabela de acordo
                            if p_term.truth_prob <= 0 {
                                n_lookup += p_term.truth_prob as LogEst;
                            } else {
                                n_lookup -= 1;
                                if (p_term.e_operator & (WO_EQ | WO_IS)) != 0 {
                                    n_lookup -= 19;
                                }
                            }
                            ii += 1;
                        }
                        drop(w_info_b);

                        r_run = log_est_add(r_run, n_lookup);
                        p_new.borrow_mut().r_run = r_run;
                    }
                    // ApplyCostMultiplier(r_run, costMult): nada, sem COSTMULT
                    where_loop_output_adjust(&p_wc, &p_new, r_size);
                    if (p_src.fg.jointype & JT_RIGHT) != 0 && probe.a_col_expr.is_some() {
                        // Não faz SCAN de um índice sobre expressão num RIGHT JOIN, porque
                        // o cursor usado para acessar o índice pode não estar posicionado
                        // na linha certa durante o laço sem casamento do right-join.
                    } else {
                        rc = where_loop_insert(p_builder, &p_new);
                    }
                    p_new.borrow_mut().n_out = r_size;
                    if rc != 0 {
                        break 'probe;
                    }
                }
            }

            p_builder.bld_flags1 = 0;
            rc = where_loop_add_btree_index(p_builder, p_src, &probe, 0);
            if p_builder.bld_flags1 == SQLITE_BLDF1_INDEXED {
                // Se um índice não único é usado, ou se um prefixo da chave de um índice
                // único é usado (tornando o índice funcionalmente não único), então os
                // dados de sqlite_stat1 passam a ser importantes para pontuar o plano
                p_tab.borrow_mut().tab_flags |= TF_MAYBE_REANALYZE;
            }
            // sqlite3Stat4ProbeFree(pRec): STAT4 desligado no Debian 13
        }
        p_probe = if p_src.fg.is_indexed_by != 0 {
            None
        } else {
            p_probe_ref.borrow().p_next.clone()
        };
        i_sort_idx += 1;
    }
    rc
}


// ---- part_010.rs ----

// Nota de tradução: no C o HiddenIndexInfo mora na memória logo depois do
// sqlite3_index_info (`&pIdxInfo[1]`). Sem aritmética de ponteiro, as rotinas
// deste trecho recebem o HiddenIndexInfo como parâmetro explícito, ao lado do
// Sqlite3IndexInfo. allocate_index_info() devolve o par.

/// Retorna verdadeiro se p_term é um termo LIMIT ou OFFSET de tabela virtual.
fn is_limit_term(p_term: &WhereTerm) -> bool {
    debug_assert!(p_term.e_operator == WO_AUX || p_term.e_match_op == 0);
    (p_term.e_match_op as i32) >= SQLITE_INDEX_CONSTRAINT_LIMIT
        && (p_term.e_match_op as i32) <= SQLITE_INDEX_CONSTRAINT_OFFSET
}

/// Retorna verdadeiro se as primeiras n_cons restrições do vetor a_usage estão
/// marcadas como em uso (têm argv_index>0). Falso caso contrário.
fn all_constraints_used(a_usage: &[Sqlite3IndexConstraintUsage], n_cons: i32) -> bool {
    for ii in 0..n_cons as usize {
        if a_usage[ii].argv_index <= 0 {
            return false;
        }
    }
    true
}

/// O argumento p_idx_info já está preenchido com todas as restrições que podem
/// ser usadas pela tabela virtual identificada por p_builder.p_new.i_tab. Esta
/// função marca um subconjunto delas como utilizável, chama o método xBestIndex
/// e acrescenta o plano devolvido ao p_builder.
///
/// Uma restrição é marcada como utilizável se:
///
///   * o argumento m_usable indica que seus pré-requisitos estão disponíveis, e
///
///   * ela não é um dos operadores da máscara m_exclude passada como quarto
///     argumento (que na prática é WO_IN ou 0).
///
/// O argumento m_prereq é uma máscara de tabelas que precisam ser varridas antes
/// da tabela virtual em questão. Elas entram nos pré-requisitos do plano antes de
/// ele ser adicionado ao p_builder.
///
/// O parâmetro de saída *pb_in vira verdadeiro se o plano acrescentado ao
/// p_builder usa um ou mais termos WO_IN, ou falso caso contrário.
fn where_loop_add_virtual_one(
    p_builder: &mut WhereLoopBuilder,
    m_prereq: Bitmask,               // Máscara de tabelas que precisam ser usadas
    m_usable: Bitmask,               // Máscara de tabelas utilizáveis
    m_exclude: u16,                  // Exclui termos que usam estes operadores
    p_idx_info: &mut Sqlite3IndexInfo, // Objeto preenchido para o xBestIndex
    p_hidden: &mut HiddenIndexInfo,  // Parte oculta que acompanha p_idx_info
    m_no_omit: u16,                  // Não omitir estas restrições
    pb_in: &mut i32,                 // SAÍDA: verdadeiro se o plano usa um IN(...)
    mut pb_retry_limit: Option<&mut i32>, // SAÍDA: tentar de novo sem LIMIT/OFFSET
) -> i32 {
    let p_wc = p_builder.p_wc.clone();
    let p_new = p_builder.p_new.clone();
    let p_parse = p_builder.p_w_info.borrow().p_parse.clone();
    let i_tab = p_new.borrow().i_tab as usize;
    let (src_col_used, p_src_tab) = {
        let p_w_info = p_builder.p_w_info.borrow();
        let p_tab_list = p_w_info.p_tab_list.borrow();
        let p_src = &p_tab_list.a[i_tab];
        (p_src.col_used, p_src.p_tab.clone().expect("tabela virtual do item FROM"))
    };
    let n_constraint = p_idx_info.n_constraint;
    let n_cons = n_constraint as usize;

    debug_assert!((m_usable & m_prereq) == m_prereq);
    *pb_in = 0;
    p_new.borrow_mut().prereq = m_prereq;

    // Mensagem de erro de um xBestIndex que se comporta mal.
    let xbest_index_malfunction = || -> i32 {
        let mut z_msg = p_src_tab.borrow().z_name.clone();
        z_msg.extend_from_slice(b".xBestIndex malfunction");
        error_msg(&p_parse, &z_msg);
        SQLITE_ERROR
    };

    // Marca a flag usable no subconjunto de restrições identificado pelos
    // argumentos m_usable e m_exclude.
    {
        let wc = p_wc.borrow();
        let a_constraint = p_idx_info
            .a_constraint
            .as_mut()
            .expect("a_constraint preenchido por allocate_index_info");
        for i in 0..n_cons {
            let p_idx_cons = &mut a_constraint[i];
            let p_term = wc.a[p_idx_cons.i_term_offset as usize].borrow();
            p_idx_cons.usable = 0;
            if (p_term.prereq_right & m_usable) == p_term.prereq_right
                && (p_term.e_operator & m_exclude) == 0
                && (pb_retry_limit.is_some() || !is_limit_term(&p_term))
            {
                p_idx_cons.usable = 1;
            }
        }
    }

    // Inicializa os campos de saída da estrutura sqlite3_index_info
    {
        let a_usage = p_idx_info
            .a_constraint_usage
            .as_mut()
            .expect("a_constraint_usage preenchido por allocate_index_info");
        for p_usage in &mut a_usage[..n_cons] {
            p_usage.argv_index = 0;
            p_usage.omit = 0;
        }
    }
    debug_assert!(p_idx_info.need_to_free_idx_str == 0);
    p_idx_info.idx_str = None;
    p_idx_info.idx_num = 0;
    p_idx_info.order_by_consumed = 0;
    p_idx_info.estimated_cost = SQLITE_BIG_DBL / 2.0;
    p_idx_info.estimated_rows = 25;
    p_idx_info.idx_flags = 0;
    p_idx_info.col_used = src_col_used;
    p_hidden.m_handle_in = 0;

    // Chama o método xBestIndex() da tabela virtual
    let rc = vtab_best_index(&p_parse, &p_src_tab, p_idx_info);
    if rc != 0 {
        if rc == SQLITE_CONSTRAINT {
            // Se o xBestIndex devolve SQLITE_CONSTRAINT, a combinação de
            // parâmetros fornecida é inutilizável. Não faz nenhuma entrada na
            // tabela de loops.
            return SQLITE_OK;
        }
        return rc;
    }

    let mut mx_term: i32 = -1;
    {
        let mut new = p_new.borrow_mut();
        debug_assert!(new.n_l_slot as i32 >= n_constraint);
        for slot in &mut new.a_l_term[..n_cons] {
            *slot = None;
        }
        new.u = WhereLoopUnion::Vtab {
            idx_num: 0,
            need_free: false,
            b_omit_offset: false,
            is_ordered: 0,
            omit_mask: 0,
            idx_str: Vec::new(),
            m_handle_in: 0,
        };
    }
    for i in 0..n_cons {
        let i_term = p_idx_info
            .a_constraint_usage
            .as_ref()
            .expect("a_constraint_usage")[i]
            .argv_index
            - 1;
        if i_term >= 0 {
            let j = p_idx_info.a_constraint.as_ref().expect("a_constraint")[i].i_term_offset;
            let usable = p_idx_info.a_constraint.as_ref().expect("a_constraint")[i].usable;
            if i_term >= n_constraint
                || j < 0
                || j >= p_wc.borrow().n_term
                || p_new.borrow().a_l_term[i_term as usize].is_some()
                || usable == 0
            {
                return xbest_index_malfunction();
            }
            let p_term_ref = p_wc.borrow().a[j as usize].clone();
            let p_term = p_term_ref.borrow();
            {
                let mut new = p_new.borrow_mut();
                new.prereq |= p_term.prereq_right;
                debug_assert!(i_term < new.n_l_slot as i32);
                new.a_l_term[i_term as usize] = Some(p_term_ref.clone());
            }
            if i_term > mx_term {
                mx_term = i_term;
            }
            let a_usage = p_idx_info
                .a_constraint_usage
                .as_ref()
                .expect("a_constraint_usage");
            {
                let mut new = p_new.borrow_mut();
                let WhereLoopUnion::Vtab { omit_mask, b_omit_offset, m_handle_in, .. } = &mut new.u
                else {
                    unreachable!("p_new.u foi definido como Vtab acima");
                };
                if a_usage[i].omit != 0 {
                    if i < 16 && ((1u32 << i) & (m_no_omit as u32)) == 0 {
                        *omit_mask |= 1u32.wrapping_shl(i_term as u32) as u16;
                    }
                    if (p_term.e_match_op as i32) == SQLITE_INDEX_CONSTRAINT_OFFSET {
                        *b_omit_offset = true;
                    }
                }
                if (smaskbit32(i as u32) & p_hidden.m_handle_in) != 0 {
                    *m_handle_in |= maskbit32(i_term as u32);
                } else if (p_term.e_operator & WO_IN) != 0 {
                    // Uma tabela virtual restringida por uma cláusula IN não pode
                    // consumir a cláusula ORDER BY porque (1) a ordem dos termos IN
                    // não tem relação necessária com a ordem dos termos de saída e
                    // (2) várias saídas de um único valor IN não se intercalam.
                    p_idx_info.order_by_consumed = 0;
                    p_idx_info.idx_flags &= !SQLITE_INDEX_SCAN_UNIQUE;
                    *pb_in = 1;
                    debug_assert!((m_exclude & WO_IN) == 0);
                }
            }

            // A menos que pb_retry_limit seja não nulo, não deve haver termos
            // LIMIT/OFFSET. E se houver, devem vir depois de todos os outros termos.
            debug_assert!(pb_retry_limit.is_some() || !is_limit_term(&p_term));
            debug_assert!(!is_limit_term(&p_term) || (i as i32) >= n_constraint - 2);
            debug_assert!(
                !is_limit_term(&p_term)
                    || (i as i32) == n_constraint - 1
                    || is_limit_term(&p_wc.borrow().a[j as usize + 1].borrow())
            );

            if is_limit_term(&p_term) && (*pb_in != 0 || !all_constraints_used(a_usage, i as i32)) {
                // Se há um termo IN(...) tratado como == (uma chamada separada ao
                // xFilter para cada valor do lado direito do IN) e também um termo
                // LIMIT ou OFFSET tratado, o plano é inutilizável. Do mesmo modo, se
                // há um LIMIT/OFFSET e outros termos não usados, o plano não pode ser
                // usado. Nesses casos *pb_retry_limit vira verdadeiro para avisar o
                // chamador a tentar de novo com LIMIT e OFFSET desabilitados.
                if p_idx_info.need_to_free_idx_str != 0 {
                    p_idx_info.idx_str = None;
                    p_idx_info.need_to_free_idx_str = 0;
                }
                if let Some(p_retry) = pb_retry_limit.as_deref_mut() {
                    *p_retry = 1;
                }
                return SQLITE_OK;
            }
        }
    }

    p_new.borrow_mut().n_l_term = (mx_term + 1) as u16;
    for i in 0..=mx_term {
        if p_new.borrow().a_l_term[i as usize].is_none() {
            // Os valores argvIdx não nulos devem ser contíguos. Levanta um erro
            // se não forem.
            return xbest_index_malfunction();
        }
    }
    {
        let mut new = p_new.borrow_mut();
        debug_assert!(new.n_l_term <= new.n_l_slot);
        let is_ordered_new: i8 = if p_idx_info.order_by_consumed != 0 {
            p_idx_info.n_order_by as i8
        } else {
            0
        };
        let need_free_new = p_idx_info.need_to_free_idx_str != 0;
        let idx_str_new = p_idx_info.idx_str.clone().unwrap_or_default();
        let WhereLoopUnion::Vtab { idx_num, need_free, is_ordered, idx_str, .. } = &mut new.u
        else {
            unreachable!("p_new.u foi definido como Vtab acima");
        };
        *idx_num = p_idx_info.idx_num;
        *need_free = need_free_new;
        p_idx_info.need_to_free_idx_str = 0;
        *idx_str = idx_str_new;
        *is_ordered = is_ordered_new;
        new.r_setup = 0;
        new.r_run = log_est_from_double(p_idx_info.estimated_cost);
        new.n_out = log_est(p_idx_info.estimated_rows as u64);

        // Liga a flag WHERE_ONEROW se o xBestIndex() indicou que a varredura
        // visita no máximo uma linha. Desliga caso contrário.
        if (p_idx_info.idx_flags & SQLITE_INDEX_SCAN_UNIQUE) != 0 {
            new.ws_flags |= WHERE_ONEROW;
        } else {
            new.ws_flags &= !WHERE_ONEROW;
        }
    }
    let rc = where_loop_insert(p_builder, &p_new);
    {
        let mut new = p_new.borrow_mut();
        if let WhereLoopUnion::Vtab { need_free, idx_str, .. } = &mut new.u {
            if *need_free {
                *idx_str = Vec::new();
                *need_free = false;
            }
        }
    }

    rc
}

/// Retorna a sequência de colação de uma restrição passada ao xBestIndex.
///
/// p_idx_info deve ser uma estrutura sqlite3_index_info passada ao xBestIndex.
/// Esta rotina depende do HiddenIndexInfo que acompanha o sqlite3_index_info.
///
/// Retorna o nome da colação:
///
///    1. Se há um operador COLLATE explícito na restrição, retorna ele.
///
///    2. Senão, se a coluna tem uma colação alternativa, retorna ela.
///
///    3. Caso contrário, retorna "BINARY".
///
/// Retorna None quando i_cons está fora do intervalo.
pub fn vtab_collation(
    p_idx_info: &Sqlite3IndexInfo,
    p_hidden: &HiddenIndexInfo,
    i_cons: i32,
) -> Option<Vec<u8>> {
    let mut z_ret: Option<Vec<u8>> = None;
    if i_cons >= 0 && i_cons < p_idx_info.n_constraint {
        let mut p_c: Option<CollSeqRef> = None;
        let i_term = p_idx_info.a_constraint.as_ref().expect("a_constraint")[i_cons as usize]
            .i_term_offset;
        let p_x = p_hidden
            .p_wc
            .as_ref()
            .expect("p_wc do HiddenIndexInfo")
            .borrow()
            .a[i_term as usize]
            .borrow()
            .p_expr
            .clone()
            .expect("p_expr do termo");
        if p_x.borrow().p_left.is_some() {
            let p_parse = p_hidden.p_parse.as_ref().expect("p_parse do HiddenIndexInfo");
            p_c = expr_compare_coll_seq(p_parse, &p_x);
        }
        z_ret = Some(match p_c {
            Some(p_coll) => p_coll.borrow().z_name.clone(),
            None => b"BINARY".to_vec(),
        });
    }
    z_ret
}

/// Retorna verdadeiro se a restrição i_cons é realmente uma restrição IN(...), ou
/// falso caso contrário. Se i_cons é uma restrição IN(...), liga (se b_handle!=0)
/// ou desliga (se b_handle==0) a flag de tratá-la com um iterador.
pub fn vtab_in(p_hidden: &mut HiddenIndexInfo, i_cons: i32, b_handle: i32) -> i32 {
    let m: u32 = smaskbit32(i_cons as u32);
    if (m & p_hidden.m_in) != 0 {
        if b_handle == 0 {
            p_hidden.m_handle_in &= !m;
        } else if b_handle > 0 {
            p_hidden.m_handle_in |= m;
        }
        return 1;
    }
    0
}

/// Esta interface só pode ser chamada de dentro do callback xBestIndex.
///
/// Se possível, faz *pp_val apontar para um objeto com o valor do lado direito da
/// restrição i_cons.
pub fn vtab_rhs_value(
    p_idx_info: &Sqlite3IndexInfo,   // Cópia do primeiro argumento do xBestIndex
    p_h: &mut HiddenIndexInfo,       // Parte oculta que acompanha p_idx_info
    i_cons: i32,                     // Restrição cujo lado direito se quer
    pp_val: &mut Option<ValueRef>,   // Escreve aqui o valor extraído
) -> i32 {
    let mut p_val: Option<ValueRef> = None;
    let mut rc = SQLITE_OK;
    if i_cons < 0 || i_cons >= p_idx_info.n_constraint {
        rc = sqlite_misuse_bkpt(line!() as i32); // EV: R-30545-25046
    } else {
        if p_h.a_rhs[i_cons as usize].is_none() {
            let i_term = p_idx_info.a_constraint.as_ref().expect("a_constraint")[i_cons as usize]
                .i_term_offset;
            let p_term_ref = p_h.p_wc.as_ref().expect("p_wc do HiddenIndexInfo").borrow().a
                [i_term as usize]
                .clone();
            let p_right = p_term_ref
                .borrow()
                .p_expr
                .as_ref()
                .expect("p_expr do termo")
                .borrow()
                .p_right
                .clone();
            let p_parse = p_h.p_parse.as_ref().expect("p_parse do HiddenIndexInfo").clone();
            let db = p_parse.borrow().db.upgrade().expect("conexão viva");
            let enc = db.borrow().enc;
            rc = value_from_expr(
                &db,
                p_right.as_ref(),
                enc,
                SQLITE_AFF_BLOB,
                &mut p_h.a_rhs[i_cons as usize],
            );
        }
        p_val = p_h.a_rhs[i_cons as usize].clone();
    }
    *pp_val = p_val.clone();

    if rc == SQLITE_OK && p_val.is_none() {
        // IMP: R-19933-32160
        rc = SQLITE_NOTFOUND; // IMP: R-36424-56542
    }

    rc
}

/// Retorna verdadeiro se a cláusula ORDER BY pode ser tratada como DISTINCT.
pub fn vtab_distinct(p_hidden: &HiddenIndexInfo) -> i32 {
    debug_assert!(p_hidden.e_distinct >= 0 && p_hidden.e_distinct <= 3);
    p_hidden.e_distinct
}

/// Faz o statement preparado associado a uma chamada ao xBestIndex usar
/// potencialmente todos os esquemas. Se o statement sendo preparado é somente de
/// leitura, apenas inicia transações de leitura em todos os esquemas. Mas se é uma
/// operação de escrita, inicia escritas em todos os esquemas.
///
/// É usada pela tabela virtual embutida sqlite_dbpage.
pub fn vtab_uses_all_schemas(p_parse: &ParseRef) {
    let db = p_parse.borrow().db.upgrade().expect("conexão viva");
    let n_db = db.borrow().n_db;
    for i in 0..n_db {
        code_verify_schema(p_parse, i);
    }
    let write_mask = p_parse.borrow().write_mask;
    if db_mask_non_zero(write_mask) {
        for i in 0..n_db {
            begin_write_operation(p_parse, 0, i);
        }
    }
}

/// Adiciona todos os objetos WhereLoop de uma tabela do join identificada por
/// p_builder.p_new.i_tab. Essa tabela é garantidamente uma tabela virtual.
///
/// Se não há LEFT nem CROSS JOIN na consulta, m_prereq e m_unusable valem 0. Caso
/// contrário, m_prereq é uma máscara de todas as entradas da cláusula FROM que
/// ocorrem antes da tabela virtual e são separadas dela por pelo menos um LEFT ou
/// CROSS JOIN. Do mesmo modo, a máscara m_unusable contém todas as entradas da
/// cláusula FROM que ocorrem depois da tabela virtual e são separadas dela por
/// pelo menos um LEFT ou CROSS JOIN.
///
/// Por exemplo, se a consulta fosse:
///
///   ... FROM t1, t2 LEFT JOIN t3, t4, vt CROSS JOIN t5, t6;
///
/// então m_prereq corresponde a (t1, t2) e m_unusable a (t5, t6).
///
/// Todas as tabelas em m_prereq precisam ser varridas antes da tabela virtual
/// atual. Portanto qualquer termo cujos pré-requisitos sejam satisfeitos por
/// m_prereq pode ser especificado como "usable" em todas as chamadas ao
/// xBestIndex. Inversamente, todas as tabelas em m_unusable precisam ser varridas
/// depois da tabela virtual atual, então qualquer termo cujos pré-requisitos se
/// sobreponham a m_unusable deve sempre ser configurado como "not-usable" para o
/// xBestIndex.
fn where_loop_add_virtual(
    p_builder: &mut WhereLoopBuilder, // Informação da cláusula WHERE
    m_prereq: Bitmask,                // Tabelas que precisam ser varridas antes desta
    m_unusable: Bitmask,              // Tabelas que precisam ser varridas depois desta
) -> i32 {
    let mut rc: i32; // Código de retorno
    let mut b_in: i32 = 0; // Verdadeiro se o plano usa o operador IN(...)
    let mut m_no_omit: u16 = 0;
    let mut b_retry: i32 = 0; // Verdadeiro para tentar de novo com LIMIT/OFFSET desabilitados

    debug_assert!((m_prereq & m_unusable) == 0);
    let p_w_info = p_builder.p_w_info.clone(); // Contexto de análise do WHERE
    let p_parse = p_w_info.borrow().p_parse.clone(); // O contexto de análise
    let p_wc = p_builder.p_wc.clone(); // A cláusula WHERE
    let p_new = p_builder.p_new.clone();
    let i_tab = p_new.borrow().i_tab as usize;
    let p_src_tab = p_w_info.borrow().p_tab_list.borrow().a[i_tab]
        .p_tab
        .clone()
        .expect("tabela do item FROM"); // O termo FROM a pesquisar
    debug_assert!(is_virtual(&p_src_tab.borrow()));
    let db = p_parse.borrow().db.upgrade().expect("conexão viva");
    let allocated = {
        let p_info = p_w_info.borrow();
        let p_tab_list = p_info.p_tab_list.borrow();
        allocate_index_info(&p_w_info, &p_wc, m_unusable, &p_tab_list.a[i_tab], &mut m_no_omit)
    };
    let (mut p_idx_info, mut p_hidden) = match allocated {
        Some(pair) => pair, // Objeto a passar ao xBestIndex()
        None => return SQLITE_NOMEM_BKPT,
    };
    {
        let mut new = p_new.borrow_mut();
        new.r_setup = 0;
        new.ws_flags = WHERE_VIRTUALTABLE;
        new.n_l_term = 0;
        new.u = WhereLoopUnion::Vtab {
            idx_num: 0,
            need_free: false,
            b_omit_offset: false,
            is_ordered: 0,
            omit_mask: 0,
            idx_str: Vec::new(),
            m_handle_in: 0,
        };
    }
    let n_constraint = p_idx_info.n_constraint; // Número de restrições em p_idx_info
    if where_loop_resize(&db, &p_new, n_constraint) != 0 {
        free_index_info(&db, p_idx_info, p_hidden);
        return SQLITE_NOMEM_BKPT;
    }

    // Primeiro chama o xBestIndex() com todas as restrições utilizáveis.
    rc = where_loop_add_virtual_one(
        p_builder,
        m_prereq,
        ALLBITS,
        0,
        &mut p_idx_info,
        &mut p_hidden,
        m_no_omit,
        &mut b_in,
        Some(&mut b_retry),
    );
    if b_retry != 0 {
        debug_assert!(rc == SQLITE_OK);
        rc = where_loop_add_virtual_one(
            p_builder,
            m_prereq,
            ALLBITS,
            0,
            &mut p_idx_info,
            &mut p_hidden,
            m_no_omit,
            &mut b_in,
            None,
        );
    }

    // Se a chamada ao xBestIndex() com todos os termos habilitados produziu um
    // plano que não exige nenhuma tabela de origem (IOW: um plano com m_best==0) e
    // não usa um operador IN(...), não adianta fazer mais chamadas ao xBestIndex(),
    // pois todas devolverão o mesmo resultado (se a implementação do xBestIndex()
    // for sã).
    let mut m_best: Bitmask = 0; // Tabelas usadas pelo melhor plano possível
    if rc == SQLITE_OK {
        m_best = p_new.borrow().prereq & !m_prereq;
    }
    if rc == SQLITE_OK && (m_best != 0 || b_in != 0) {
        let mut seen_zero = 0; // Verdadeiro se um plano sem pré-requisitos foi visto
        let mut seen_zero_no_in = 0; // Plano sem pré-requisitos e sem IN(...) visto
        let mut m_prev: Bitmask = 0;
        let mut m_best_no_in: Bitmask = 0;

        // Se o plano produzido pela chamada anterior usa um termo IN(...), chama o
        // xBestIndex de novo, desta vez com os termos IN(...) desabilitados.
        if b_in != 0 {
            rc = where_loop_add_virtual_one(
                p_builder,
                m_prereq,
                ALLBITS,
                WO_IN,
                &mut p_idx_info,
                &mut p_hidden,
                m_no_omit,
                &mut b_in,
                None,
            );
            debug_assert!(b_in == 0);
            m_best_no_in = p_new.borrow().prereq & !m_prereq;
            if m_best_no_in == 0 {
                seen_zero = 1;
                seen_zero_no_in = 1;
            }
        }

        // Chama o xBestIndex uma vez para cada valor distinto de
        // (prereq_right & ~m_prereq) no conjunto de termos que se aplicam à
        // tabela virtual atual.
        while rc == SQLITE_OK {
            let mut m_next: Bitmask = ALLBITS;
            debug_assert!(m_next > 0);
            for i in 0..n_constraint as usize {
                let i_term_offset =
                    p_idx_info.a_constraint.as_ref().expect("a_constraint")[i].i_term_offset;
                let m_this: Bitmask =
                    p_wc.borrow().a[i_term_offset as usize].borrow().prereq_right & !m_prereq;
                if m_this > m_prev && m_this < m_next {
                    m_next = m_this;
                }
            }
            m_prev = m_next;
            if m_next == ALLBITS {
                break;
            }
            if m_next == m_best || m_next == m_best_no_in {
                continue;
            }
            rc = where_loop_add_virtual_one(
                p_builder,
                m_prereq,
                m_next | m_prereq,
                0,
                &mut p_idx_info,
                &mut p_hidden,
                m_no_omit,
                &mut b_in,
                None,
            );
            if p_new.borrow().prereq == m_prereq {
                seen_zero = 1;
                if b_in == 0 {
                    seen_zero_no_in = 1;
                }
            }
        }

        // Se as chamadas ao xBestIndex() do laço acima não acharam um plano que
        // não exija nenhuma tabela de origem (isto é, um plano garantidamente
        // utilizável), faz aqui uma chamada com todas as tabelas de origem
        // desabilitadas.
        if rc == SQLITE_OK && seen_zero == 0 {
            rc = where_loop_add_virtual_one(
                p_builder,
                m_prereq,
                m_prereq,
                0,
                &mut p_idx_info,
                &mut p_hidden,
                m_no_omit,
                &mut b_in,
                None,
            );
            if b_in == 0 {
                seen_zero_no_in = 1;
            }
        }

        // Se as chamadas ao xBestIndex() até aqui não acharam um plano que não
        // exija nenhuma tabela de origem e não use um operador IN(...), faz uma
        // chamada final para obter um.
        if rc == SQLITE_OK && seen_zero_no_in == 0 {
            rc = where_loop_add_virtual_one(
                p_builder,
                m_prereq,
                m_prereq,
                WO_IN,
                &mut p_idx_info,
                &mut p_hidden,
                m_no_omit,
                &mut b_in,
                None,
            );
        }
    }

    if p_idx_info.need_to_free_idx_str != 0 {
        p_idx_info.idx_str = None;
    }
    free_index_info(&db, p_idx_info, p_hidden);
    rc
}


// ---- part_011.rs ----

/// Adiciona entradas WhereLoop para tratar termos OR. Funciona tanto para btrees
/// quanto para tabelas virtuais.
fn where_loop_add_or(
    p_builder: &mut WhereLoopBuilder,
    m_prereq: Bitmask,
    m_unusable: Bitmask,
) -> i32 {
    let p_w_info = p_builder.p_w_info.clone();
    let mut rc = SQLITE_OK;
    let mut s_sum = WhereOrSet::default();

    let p_wc = p_builder.p_wc.clone();
    // pWCEnd: o fim da lista de termos é fixado antes do laço, como no C.
    let n_wc_term = p_wc.borrow().n_term as usize;
    let p_new = p_builder.p_new.clone();
    let i_tab = p_new.borrow().i_tab as usize;
    let (i_cur, join_type, is_virt) = {
        let info = p_w_info.borrow();
        let tab_list = info.p_tab_list.borrow();
        let p_item = &tab_list.a[i_tab];
        (
            p_item.i_cursor,
            p_item.fg.jointype,
            p_item
                .p_tab
                .as_ref()
                .map_or(false, |t| is_virtual(&t.borrow())),
        )
    };

    // A otimização OR de vários índices não funciona para RIGHT e FULL JOIN
    if (join_type & JT_RIGHT) != 0 {
        return SQLITE_OK;
    }

    for k in 0..n_wc_term {
        if rc != SQLITE_OK {
            break;
        }
        let p_term = p_wc.borrow().a[k].clone();
        let mask_self = p_new.borrow().mask_self;

        // Só interessam termos WO_OR cujas tabelas indexáveis incluem a deste loop
        let p_or_wc: WhereClauseRef = {
            let t = p_term.borrow();
            if (t.e_operator & WO_OR) == 0 {
                continue;
            }
            match &t.u {
                WhereTermUnion::OrInfo(Some(p_or_info)) => {
                    if (p_or_info.indexable & mask_self) == 0 {
                        continue;
                    }
                    p_or_info.wc.clone()
                }
                _ => continue,
            }
        };
        let p_or_terms: Vec<WhereTermRef> = {
            let or_wc = p_or_wc.borrow();
            or_wc.a.iter().take(or_wc.n_term as usize).cloned().collect()
        };
        let mut once = true;

        // sSubBuild = *pBuilder
        let s_cur: WhereOrSetRef = Rc::new(RefCell::new(WhereOrSet::default()));
        let mut s_sub_build = WhereLoopBuilder {
            p_w_info: p_builder.p_w_info.clone(),
            p_wc: p_builder.p_wc.clone(),
            p_new: p_builder.p_new.clone(),
            p_or_set: Some(s_cur.clone()),
            bld_flags1: p_builder.bld_flags1,
            bld_flags2: p_builder.bld_flags2,
            i_plan_limit: p_builder.i_plan_limit,
        };

        for p_or_term in p_or_terms.iter() {
            let (e_operator, left_cursor) = {
                let t = p_or_term.borrow();
                (t.e_operator, t.left_cursor)
            };
            if (e_operator & WO_AND) != 0 {
                let p_and_wc: WhereClauseRef = match &p_or_term.borrow().u {
                    WhereTermUnion::AndInfo(Some(p_and_info)) => p_and_info.wc.clone(),
                    _ => continue,
                };
                s_sub_build.p_wc = p_and_wc;
            } else if left_cursor == i_cur {
                // tempWC: cláusula de um único termo, aninhada em pWC
                let temp_wc = WhereClause {
                    p_w_info: p_wc.borrow().p_w_info.clone(),
                    p_outer: Some(Rc::downgrade(&p_wc)),
                    op: TK_AND as u8,
                    has_or: 0,
                    n_term: 1,
                    n_slot: 1,
                    n_base: 1,
                    a: vec![p_or_term.clone()],
                };
                s_sub_build.p_wc = Rc::new(RefCell::new(temp_wc));
            } else {
                continue;
            }
            s_cur.borrow_mut().n = 0;
            if is_virt {
                rc = where_loop_add_virtual(&mut s_sub_build, m_prereq, m_unusable);
            } else {
                rc = where_loop_add_btree(&mut s_sub_build, m_prereq);
            }
            if rc == SQLITE_OK {
                rc = where_loop_add_or(&mut s_sub_build, m_prereq, m_unusable);
            }
            let s_cur_val: WhereOrSet = s_cur.borrow().clone();
            if s_cur_val.n == 0 {
                s_sum.n = 0;
                break;
            } else if once {
                where_or_move(&mut s_sum, &s_cur_val);
                once = false;
            } else {
                let mut s_prev = WhereOrSet::default();
                where_or_move(&mut s_prev, &s_sum);
                s_sum.n = 0;
                for i in 0..s_prev.n as usize {
                    for j in 0..s_cur_val.n as usize {
                        where_or_insert(
                            &mut s_sum,
                            s_prev.a[i].prereq | s_cur_val.a[j].prereq,
                            log_est_add(s_prev.a[i].r_run, s_cur_val.a[j].r_run),
                            log_est_add(s_prev.a[i].n_out, s_cur_val.a[j].n_out),
                        );
                    }
                }
            }
        }
        {
            let mut new_loop = p_new.borrow_mut();
            new_loop.n_l_term = 1;
            new_loop.a_l_term[0] = Some(p_term.clone());
            new_loop.ws_flags = WHERE_MULTI_OR;
            new_loop.r_setup = 0;
            new_loop.i_sort_idx = 0;
            // memset(&pNew->u, 0, sizeof(pNew->u))
            new_loop.u = WhereLoopUnion::Btree {
                n_eq: 0,
                n_btm: 0,
                n_top: 0,
                n_distinct_col: 0,
                p_index: None,
            };
        }
        let mut i = 0usize;
        while rc == SQLITE_OK && i < s_sum.n as usize {
            // AJUSTE: hoje sSum.a[i].rRun é a soma dos custos de todas as varreduras
            // internas exigidas pela varredura OR. Por erros de arredondamento, o custo
            // da varredura OR pode ser igual ao da sua subvarredura mais cara. Soma-se a
            // menor penalidade possível (equivalente a multiplicar o custo por 1,07) para
            // que isso não aconteça. Do contrário, para cláusulas WHERE como
            // "WHERE likelihood(x=?, 0.99) OR y=?" com índice em "y", o planejador
            // poderia fazer o OR de uma varredura completa com uma busca por índice, e
            // outros resultados igualmente estranhos.
            {
                let mut new_loop = p_new.borrow_mut();
                new_loop.r_run = s_sum.a[i].r_run.wrapping_add(1);
                new_loop.n_out = s_sum.a[i].n_out;
                new_loop.prereq = s_sum.a[i].prereq;
            }
            rc = where_loop_insert(p_builder, &p_new);
            i += 1;
        }
    }
    rc
}

/// Adiciona todos os objetos WhereLoop de todas as tabelas.
fn where_loop_add_all(p_builder: &mut WhereLoopBuilder) -> i32 {
    let p_w_info = p_builder.p_w_info.clone();
    let mut m_prereq: Bitmask = 0;
    let mut m_prior: Bitmask = 0;
    let p_tab_list = p_w_info.borrow().p_tab_list.clone();
    // pEnd = &pTabList->a[pWInfo->nLevel]
    let n_level = p_w_info.borrow().n_level as usize;
    let db = p_w_info
        .borrow()
        .p_parse
        .borrow()
        .db
        .upgrade()
        .expect("a conexão de banco de dados vive mais que o Parse");
    let mut rc = SQLITE_OK;
    let mut b_first_past_rj = false;
    let mut has_right_join = false;

    // Percorre as tabelas do join, da esquerda para a direita
    let p_new = p_builder.p_new.clone();

    // O pNew já deve ter sido inicializado
    debug_assert!(p_new.borrow().n_l_term == 0);
    debug_assert!(p_new.borrow().ws_flags == 0);
    debug_assert!(p_new.borrow().n_l_slot as usize >= p_new.borrow().a_l_term_space.len());

    p_builder.i_plan_limit = SQLITE_QUERY_PLANNER_LIMIT;
    for i_tab in 0..n_level {
        let mut m_unusable: Bitmask = 0;
        p_new.borrow_mut().i_tab = i_tab as u8;
        p_builder.i_plan_limit += SQLITE_QUERY_PLANNER_LIMIT_INCR;
        let (i_cursor, join_type, is_virt) = {
            let tab_list = p_tab_list.borrow();
            let p_item = &tab_list.a[i_tab];
            (
                p_item.i_cursor,
                p_item.fg.jointype,
                p_item
                    .p_tab
                    .as_ref()
                    .map_or(false, |t| is_virtual(&t.borrow())),
            )
        };
        let mask_self = where_get_mask(&p_w_info.borrow().s_mask_set, i_cursor);
        p_new.borrow_mut().mask_self = mask_self;
        if b_first_past_rj || (join_type & (JT_OUTER | JT_CROSS | JT_LTORJ)) != 0 {
            // Soma pré-requisitos para impedir a reordenação dos termos da cláusula FROM
            // através de CROSS joins e outer joins. O booleano b_first_past_rj impede que
            // o operando direito de um RIGHT JOIN seja trocado com outros elementos ainda
            // mais à direita.
            //
            // O caso JT_LTORJ e a flag has_right_join trabalham juntos para impedir que
            // termos da cláusula FROM passem do lado direito de um LEFT JOIN para o lado
            // esquerdo desse join, se o LEFT JOIN estiver, ele mesmo, à esquerda de um
            // RIGHT JOIN.
            if (join_type & JT_LTORJ) != 0 {
                has_right_join = true;
            }
            m_prereq |= m_prior;
            b_first_past_rj = (join_type & JT_RIGHT) != 0;
        } else if !has_right_join {
            m_prereq = 0;
        }
        if is_virt {
            for i_next in (i_tab + 1)..n_level {
                let (next_join_type, next_cursor) = {
                    let tab_list = p_tab_list.borrow();
                    let p = &tab_list.a[i_next];
                    (p.fg.jointype, p.i_cursor)
                };
                if m_unusable != 0 || (next_join_type & (JT_OUTER | JT_CROSS)) != 0 {
                    m_unusable |= where_get_mask(&p_w_info.borrow().s_mask_set, next_cursor);
                }
            }
            rc = where_loop_add_virtual(p_builder, m_prereq, m_unusable);
        } else {
            rc = where_loop_add_btree(p_builder, m_prereq);
        }
        if rc == SQLITE_OK && p_builder.p_wc.borrow().has_or != 0 {
            rc = where_loop_add_or(p_builder, m_prereq, m_unusable);
        }
        m_prior |= p_new.borrow().mask_self;
        if rc != 0 || db.borrow().malloc_failed {
            if rc == SQLITE_DONE {
                // Atingiu o limite de busca do planejador fixado por i_plan_limit
                api::log(SQLITE_WARNING, b"abbreviated query algorithm search");
                rc = SQLITE_OK;
            } else {
                break;
            }
        }
    }

    where_loop_clear(&db, &p_new);
    rc
}

/// Examina um WherePath (com o WhereLoop extra do sexto parâmetro) para ver se ele
/// produz linhas no ORDER BY (ou GROUP BY) pedido sem exigir uma ordenação separada.
/// Retorna N:
///
///   N>0:   N termos do ORDER BY são satisfeitos
///   N==0:  nenhum termo do ORDER BY é satisfeito
///   N<0:   ainda não se sabe quantos termos do ORDER BY podem ser satisfeitos
///
/// O processamento de WHERE_GROUPBY e WHERE_DISTINCTBY é menos estrito. Com GROUP BY e
/// DISTINCT só se exige que linhas equivalentes apareçam adjacentes, em qualquer ordem,
/// logo os termos de p_order_by podem casar em qualquer ordem. Com ORDER BY, os termos
/// precisam casar estritamente da esquerda para a direita.
fn where_path_satisfies_order_by(
    p_w_info: &WhereInfo,        // A cláusula WHERE
    p_order_by: &ExprList,       // ORDER BY, GROUP BY ou DISTINCT a verificar
    p_path: &WherePath,          // O WherePath a verificar
    wctrl_flags: u16,            // WHERE_GROUPBY, _DISTINCTBY ou _ORDERBY_LIMIT
    n_loop: u16,                 // Número de entradas em p_path.a_loop
    p_last: &WhereLoopRef,       // Acrescenta este WhereLoop ao fim de p_path.a_loop
    p_rev_mask: &mut Bitmask,    // SAÍDA: máscara dos WhereLoop a rodar em ordem inversa
) -> i8 {
    let mut rev_set: bool; // Verdadeiro se rev é conhecido
    let mut rev: u8; // Ordem de classificação composta
    let mut rev_idx: u8; // Ordem de classificação do índice
    let mut is_order_distinct: bool; // Todos os WhereLoop anteriores são order-distinct
    let mut distinct_columns: bool; // Verdadeiro se o loop tem colunas UNIQUE NOT NULL
    let mut is_match: bool; // i_column casa com um termo do ORDER BY
    let mut eq_op_mask: u16; // Operadores de igualdade permitidos
    let n_order_by: u16; // Número de termos do ORDER BY
    let mut p_loop: Option<WhereLoopRef> = None; // WhereLoop em processamento
    let p_parse: ParseRef = p_w_info.p_parse.clone();
    let db = p_parse
        .borrow()
        .db
        .upgrade()
        .expect("a conexão de banco de dados vive mais que o Parse"); // Conexão de banco de dados
    let mut ob_sat: Bitmask = 0; // Máscara dos termos do ORDER BY satisfeitos até agora
    let ob_done: Bitmask; // Máscara de todos os termos do ORDER BY
    let mut order_distinct_mask: Bitmask; // Máscara de todos os loops bem ordenados
    let mut ready: Bitmask; // Máscara dos loops internos
    let wctrl = wctrl_flags as u32;

    // Dizemos que o WhereLoop é "one-row" se gera no máximo uma linha de saída. Um
    // WhereLoop é one-row se: (a) todas as colunas do índice casam com WHERE_COLUMN_EQ e
    // (b) o índice é único. Qualquer WhereLoop com restrição WHERE_COLUMN_EQ no rowid é
    // one-row. Todo WhereLoop one-row tem o bit WHERE_ONEROW em ws_flags.
    //
    // Dizemos que o WhereLoop é "order-distinct" se o conjunto de colunas dele que estão
    // no ORDER BY é diferente para toda linha do WhereLoop. Todo WhereLoop one-row é
    // automaticamente order-distinct. Um WhereLoop sem colunas no ORDER BY não é
    // order-distinct. Ser order-distinct não é bem o mesmo que ser UNIQUE, pois uma coluna
    // ou índice UNIQUE pode ter várias linhas NULL, e os NULL são equivalentes para fins de
    // order-distinct. Para ser order-distinct, as colunas precisam ser UNIQUE e NOT NULL.
    //
    // O rowid de uma tabela é sempre UNIQUE e NOT NULL, então sempre que o rowid aparece
    // no ORDER BY o WhereLoop correspondente é automaticamente order-distinct.

    if n_loop != 0 && optimization_disabled(&db.borrow(), SQLITE_ORDERBYIDXJOIN) {
        return 0;
    }

    n_order_by = p_order_by.n_expr as u16;
    if (n_order_by as usize) > (BMS as usize) - 1 {
        return 0; // Não otimiza ORDER BY grande demais
    }
    is_order_distinct = true;
    ob_done = maskbit(n_order_by as u32).wrapping_sub(1);
    order_distinct_mask = 0;
    ready = 0;
    eq_op_mask = WO_EQ | WO_IS | WO_ISNULL;
    if (wctrl & (WHERE_ORDERBY_LIMIT | WHERE_ORDERBY_MAX | WHERE_ORDERBY_MIN)) != 0 {
        eq_op_mask |= WO_IN;
    }
    for i_loop in 0..=(n_loop as i32) {
        if !(is_order_distinct && ob_sat < ob_done) {
            break;
        }
        if i_loop > 0 {
            if let Some(prev) = &p_loop {
                ready |= prev.borrow().mask_self;
            }
        }
        if i_loop < n_loop as i32 {
            p_loop = Some(p_path.a_loop[i_loop as usize].clone());
            if (wctrl & WHERE_ORDERBY_LIMIT) != 0 {
                continue;
            }
        } else {
            p_loop = Some(p_last.clone());
        }
        let loop_ref: WhereLoopRef = match &p_loop {
            Some(l) => l.clone(),
            None => return 0,
        };
        let loop_ws_flags = loop_ref.borrow().ws_flags;
        if (loop_ws_flags & WHERE_VIRTUALTABLE) != 0 {
            let vtab_is_ordered = match &loop_ref.borrow().u {
                WhereLoopUnion::Vtab { is_ordered, .. } => *is_ordered,
                _ => 0,
            };
            if vtab_is_ordered != 0
                && (wctrl & (WHERE_DISTINCTBY | WHERE_SORTBYGROUP)) != WHERE_DISTINCTBY
            {
                ob_sat = ob_done;
            }
            break;
        } else if (wctrl & WHERE_DISTINCTBY) != 0 {
            if let WhereLoopUnion::Btree { n_distinct_col, .. } = &mut loop_ref.borrow_mut().u {
                *n_distinct_col = 0;
            }
        }
        let i_cur = {
            let tab_list = p_w_info.p_tab_list.borrow();
            tab_list.a[loop_ref.borrow().i_tab as usize].i_cursor
        };

        // Marca todo termo X do ORDER BY que é coluna da tabela do loop atual e para o
        // qual há na cláusula WHERE um termo da forma X IS NULL ou X=? que referencia
        // apenas loops externos.
        for i in 0..n_order_by as usize {
            if (maskbit(i as u32) & ob_sat) != 0 {
                continue;
            }
            let p_ob_expr = match p_order_by.a[i]
                .p_expr
                .as_deref()
                .and_then(expr_skip_collate_and_likely)
            {
                Some(e) => e,
                None => continue,
            };
            if p_ob_expr.op != TK_COLUMN && p_ob_expr.op != TK_AGG_COLUMN {
                continue;
            }
            if p_ob_expr.i_table != i_cur {
                continue;
            }
            let p_term = match where_find_term(
                Some(p_w_info.s_wc.clone()),
                i_cur,
                p_ob_expr.i_column as i16,
                !ready,
                eq_op_mask as u32,
                None,
            ) {
                Some(t) => t,
                None => continue,
            };
            let term = p_term.borrow();
            if term.e_operator == WO_IN {
                // Termos IN só valem para ordenar na otimização ORDER BY LIMIT, e só se
                // forem de fato usados pelo plano de consulta
                let loop_b = loop_ref.borrow();
                let mut j = 0usize;
                while j < loop_b.n_l_term as usize
                    && !loop_b.a_l_term[j]
                        .as_ref()
                        .map_or(false, |t| Rc::ptr_eq(t, &p_term))
                {
                    j += 1;
                }
                if j >= loop_b.n_l_term as usize {
                    continue;
                }
            }
            if (term.e_operator & (WO_EQ | WO_IS)) != 0 && p_ob_expr.i_column >= 0 {
                let p_coll1 = match p_order_by.a[i].p_expr.as_deref() {
                    Some(e) => expr_nn_coll_seq(&p_parse, e),
                    None => continue,
                };
                let p_coll2 = match &term.p_expr {
                    Some(e) => expr_compare_coll_seq(&p_parse, &e.borrow()),
                    None => None,
                };
                match p_coll2 {
                    None => continue,
                    Some(c2) => {
                        if str_i_cmp(&p_coll1.borrow().z_name, &c2.borrow().z_name) != 0 {
                            continue;
                        }
                    }
                }
            }
            ob_sat |= maskbit(i as u32);
        }

        if (loop_ref.borrow().ws_flags & WHERE_ONEROW) == 0 {
            let p_index: Option<IndexRef>;
            let n_key_col: u16; // Número de colunas-chave de p_index
            let n_column: u16; // Total de colunas ordenadas do índice
            if (loop_ref.borrow().ws_flags & WHERE_IPK) != 0 {
                p_index = None;
                n_key_col = 0;
                n_column = 1;
            } else {
                let cur_index = match &loop_ref.borrow().u {
                    WhereLoopUnion::Btree { p_index, .. } => p_index.clone(),
                    _ => None,
                };
                match cur_index {
                    None => return 0,
                    Some(ix) => {
                        if ix.borrow().b_unordered {
                            return 0;
                        }
                        {
                            let ixb = ix.borrow();
                            n_key_col = ixb.n_key_col;
                            n_column = ixb.n_column;
                            // Todos os termos relevantes do índice também precisam ser
                            // não NULL para is_order_distinct ser verdadeiro. O valor
                            // calculado aqui pode ser um falso positivo. As correções
                            // vêm em tag-20210426-1 mais abaixo
                            is_order_distinct = is_unique_index(&ixb)
                                && (loop_ref.borrow().ws_flags & WHERE_SKIPSCAN) == 0;
                        }
                        p_index = Some(ix);
                    }
                }
            }
            let p_index_b = p_index.as_ref().map(|r| r.borrow());
            let (n_eq, n_skip) = {
                let lb = loop_ref.borrow();
                let n_eq = match &lb.u {
                    WhereLoopUnion::Btree { n_eq, .. } => *n_eq as usize,
                    _ => 0,
                };
                (n_eq, lb.n_skip as usize)
            };

            // Percorre todas as colunas do índice e trata as que não são restringidas
            // por == nem IN.
            rev = 0;
            rev_set = false;
            distinct_columns = false;
            for j in 0..n_column as usize {
                let mut b_once = true; // Verdadeiro para rodar o laço de busca do ORDER BY

                if j < n_eq && j >= n_skip {
                    let (e_op, p_x) = {
                        let lb = loop_ref.borrow();
                        let t = lb.a_l_term[j].as_ref().map(|t| t.borrow());
                        match t {
                            Some(t) => (t.e_operator, t.p_expr.clone()),
                            None => (0u16, None),
                        }
                    };

                    // Pula termos == e IS e ISNULL (e também termos IN no processamento
                    // WHERE_ORDERBY_LIMIT). Termos IS e ISNULL implicam que o índice não
                    // é UNIQUE NOT NULL, e então o loop precisa ser marcado como não
                    // order-distinct, pois pode ter linhas NULL repetidas.
                    //
                    // Se o termo atual é uma coluna de uma expressão ((?,?) IN (SELECT...))
                    // cujo SELECT devolve mais de uma coluna, verifica-se que ela é a
                    // única coluna usada por este loop. Do contrário, se for uma entre
                    // duas ou mais, nenhuma das colunas pode ser considerada casando com
                    // um termo do ORDER BY.
                    if (e_op & eq_op_mask) != 0 {
                        if (e_op & (WO_ISNULL | WO_IS)) != 0 {
                            is_order_distinct = false;
                        }
                        continue;
                    } else if (e_op & WO_IN) != 0 {
                        // e_op é um operador de igualdade pela restrição j<n_eq acima.
                        // Qualquer igualdade que não seja WO_IN é capturada pelo "if"
                        // anterior, então aqui só pode ser WO_IN.
                        let lb = loop_ref.borrow();
                        for i in (j + 1)..n_eq {
                            let other = lb.a_l_term[i].as_ref().and_then(|t| t.borrow().p_expr.clone());
                            let same = match (&other, &p_x) {
                                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                                (None, None) => true,
                                _ => false,
                            };
                            if same {
                                b_once = false;
                                break;
                            }
                        }
                    }
                }

                // Obtém o número da coluna na tabela (i_column) e a ordem de
                // classificação (rev_idx) da j-ésima coluna do índice.
                let mut i_column: i32;
                if let Some(ixb) = &p_index_b {
                    i_column = ixb.ai_column[j] as i32;
                    rev_idx = ixb.a_sort_order[j] & KEYINFO_ORDER_DESC;
                    let i_p_key = match ixb.p_table.upgrade() {
                        Some(t) => t.borrow().i_p_key as i32,
                        None => -1,
                    };
                    if i_column == i_p_key {
                        i_column = XN_ROWID as i32;
                    }
                } else {
                    i_column = XN_ROWID as i32;
                    rev_idx = 0;
                }

                // Uma coluna sem restrição que pode ser NULL significa que este
                // WhereLoop não é bem ordenado. tag-20210426-1
                if is_order_distinct {
                    if i_column >= 0 && j >= n_eq {
                        let not_null = match &p_index_b {
                            Some(ixb) => match ixb.p_table.upgrade() {
                                Some(t) => t.borrow().a_col[i_column as usize].not_null,
                                None => 1,
                            },
                            None => 1,
                        };
                        if not_null == 0 {
                            is_order_distinct = false;
                        }
                    }
                    if i_column == XN_EXPR as i32 {
                        is_order_distinct = false;
                    }
                }

                // Acha o termo do ORDER BY que corresponde à j-ésima coluna do índice e
                // o marca como satisfeito
                is_match = false;
                let mut matched_i = 0usize;
                for ii in 0..n_order_by as usize {
                    if !b_once {
                        break;
                    }
                    if (maskbit(ii as u32) & ob_sat) != 0 {
                        continue;
                    }
                    let p_ob_expr = match p_order_by.a[ii]
                        .p_expr
                        .as_deref()
                        .and_then(expr_skip_collate_and_likely)
                    {
                        Some(e) => e,
                        None => continue,
                    };
                    if (wctrl & (WHERE_GROUPBY | WHERE_DISTINCTBY)) == 0 {
                        b_once = false;
                    }
                    if i_column >= XN_ROWID as i32 {
                        if p_ob_expr.op != TK_COLUMN && p_ob_expr.op != TK_AGG_COLUMN {
                            continue;
                        }
                        if p_ob_expr.i_table != i_cur {
                            continue;
                        }
                        if p_ob_expr.i_column != i_column {
                            continue;
                        }
                    } else {
                        let p_ix_expr = match &p_index_b {
                            Some(ixb) => match &ixb.a_col_expr {
                                Some(l) => l.a[j].p_expr.as_deref(),
                                None => None,
                            },
                            None => None,
                        };
                        match p_ix_expr {
                            Some(e) => {
                                if expr_compare_skip(p_ob_expr, e, i_cur) != 0 {
                                    continue;
                                }
                            }
                            None => continue,
                        }
                    }
                    if i_column != XN_ROWID as i32 {
                        let p_coll = match p_order_by.a[ii].p_expr.as_deref() {
                            Some(e) => expr_nn_coll_seq(&p_parse, e),
                            None => continue,
                        };
                        let coll_matches = match &p_index_b {
                            Some(ixb) => {
                                str_i_cmp(&p_coll.borrow().z_name, &ixb.az_coll[j]) == 0
                            }
                            None => false,
                        };
                        if !coll_matches {
                            continue;
                        }
                    }
                    if (wctrl & WHERE_DISTINCTBY) != 0 {
                        if let WhereLoopUnion::Btree { n_distinct_col, .. } =
                            &mut loop_ref.borrow_mut().u
                        {
                            *n_distinct_col = (j + 1) as u16;
                        }
                    }
                    is_match = true;
                    matched_i = ii;
                    break;
                }
                if is_match && (wctrl & WHERE_GROUPBY) == 0 {
                    // Garante que a ordem de classificação é compatível numa cláusula
                    // ORDER BY. A ordem é irrelevante para GROUP BY.
                    let sort_flags = p_order_by.a[matched_i].fg.sort_flags;
                    if rev_set {
                        if (rev ^ rev_idx) != (sort_flags & KEYINFO_ORDER_DESC) {
                            is_match = false;
                        }
                    } else {
                        rev = rev_idx ^ (sort_flags & KEYINFO_ORDER_DESC);
                        if rev != 0 {
                            *p_rev_mask |= maskbit(i_loop as u32);
                        }
                        rev_set = true;
                    }
                }
                if is_match && (p_order_by.a[matched_i].fg.sort_flags & KEYINFO_ORDER_BIGNULL) != 0 {
                    if j == n_eq {
                        loop_ref.borrow_mut().ws_flags |= WHERE_BIGNULL_SORT;
                    } else {
                        is_match = false;
                    }
                }
                if is_match {
                    if i_column == XN_ROWID as i32 {
                        distinct_columns = true;
                    }
                    ob_sat |= maskbit(matched_i as u32);
                } else {
                    // Nenhum casamento encontrado
                    if j == 0 || j < n_key_col as usize {
                        is_order_distinct = false;
                    }
                    break;
                }
            } // fim do laço sobre todas as colunas do índice
            if distinct_columns {
                is_order_distinct = true;
            }
        } // fim do if não one-row

        // Marca os demais termos do ORDER BY que referenciam p_loop
        if is_order_distinct {
            order_distinct_mask |= loop_ref.borrow().mask_self;
            for i in 0..n_order_by as usize {
                if (maskbit(i as u32) & ob_sat) != 0 {
                    continue;
                }
                let p = match p_order_by.a[i].p_expr.as_deref() {
                    Some(e) => e,
                    None => continue,
                };
                let m_term = where_expr_usage(&p_w_info.s_mask_set, p);
                if m_term == 0 && expr_is_constant(None, p) == 0 {
                    continue;
                }
                if (m_term & !order_distinct_mask) == 0 {
                    ob_sat |= maskbit(i as u32);
                }
            }
        }
    } // Fim do laço sobre todos os WhereLoop, do mais externo ao mais interno
    if ob_sat == ob_done {
        return n_order_by as i8;
    }
    if !is_order_distinct {
        let mut i = n_order_by as i32 - 1;
        while i > 0 {
            let m: Bitmask = if (i as usize) < (BMS as usize) {
                maskbit(i as u32).wrapping_sub(1)
            } else {
                0
            };
            if (ob_sat & m) == m {
                return i as i8;
            }
            i -= 1;
        }
        return 0;
    }
    -1
}


// ---- part_012.rs ----

/// Se o flag WHERE_GROUPBY está na máscara passada a where_begin(), o planejador
/// assume que a lista p_order_by é na verdade uma cláusula GROUP BY, e qualquer ordem
/// que agrupe as linhas como exigido satisfaz o pedido.
///
/// Normalmente, nesse caso o chamador não sabe se as linhas realmente saem ordenadas
/// ou só numa ordem que dá o agrupamento necessário. Porém, se WHERE_SORTBYGROUP
/// também foi passado a where_begin(), esta função pode ser chamada no WhereInfo
/// devolvido: retorna verdadeiro se as linhas realmente saem ordenadas como pedido.
///
/// Por exemplo, com `CREATE INDEX i1 ON t1(x, Y);`:
///
///   SELECT * FROM t1 GROUP BY x,y ORDER BY x,y;   -- is_sorted()==1
///   SELECT * FROM t1 GROUP BY y,x ORDER BY y,x;   -- is_sorted()==0
pub fn where_is_sorted(p_w_info: &WhereInfoRef) -> i32 {
    let p_w_info = p_w_info.borrow();
    debug_assert!((p_w_info.wctrl_flags & ((WHERE_GROUPBY | WHERE_DISTINCTBY) as u16)) != 0);
    debug_assert!((p_w_info.wctrl_flags & (WHERE_SORTBYGROUP as u16)) != 0);
    p_w_info.sorted as i32
}

// where_path_name() só existe sob WHERETRACE_ENABLED (depuração) e não faz parte do
// build do Debian 13, então não é traduzida. O mesmo vale para os blocos de
// WHERETRACE e sqlite3WhereTrace do solver.

/// Devolve o custo de ordenar n_row linhas, supondo que as chaves têm n_order_by
/// colunas e que as primeiras n_sorted colunas já estão em ordem.
pub fn where_sorting_cost(
    p_w_info: &WhereInfoRef, // Contexto de planejamento da consulta
    mut n_row: LogEst,       // Número estimado de linhas a ordenar
    n_order_by: i32,         // Número de termos da cláusula ORDER BY
    n_sorted: i32,           // Número de termos iniciais do ORDER BY já em ordem natural
) -> LogEst {
    // O custo estimado de uma ordenação externa completa, sendo N o número de linhas
    // a ordenar, é:
    //
    //   custo = (K * N * log(N)).
    //
    // Ou, se a cláusula order-by tem X termos mas só os últimos Y estão fora de ordem,
    // a ordenação por blocos reduz o custo a:
    //
    //   custo = (K * N * log(N)) * (Y/X)
    //
    // A constante K é no mínimo 2.0, mas será maior se há muitas colunas a ordenar,
    // pois o tempo de ordenação é proporcional à quantidade de conteúdo. O algoritmo
    // não distingue colunas gordas (BLOBs e TEXTs) de magras (INTs): usa o número de
    // colunas como aproximação da largura da linha.
    //
    // Um fator extra de 2.0 ou 3.0 entra no custo se a ordenação usa OP_IdxInsert e
    // OP_Sort em vez de OP_SorterInsert.
    let (wctrl_flags, i_limit, n_expr) = {
        let w = p_w_info.borrow();
        let p_select = w.p_select.as_ref().unwrap().borrow();
        let n_expr = p_select.p_e_list.as_ref().unwrap().borrow().n_expr;
        (w.wctrl_flags, w.i_limit, n_expr)
    };
    // AJUSTE: custo de ordenação proporcional ao número de colunas de saída
    let n_col: LogEst = log_est(((n_expr + 59) / 30) as u64);
    let mut r_sort_cost: LogEst = n_row.wrapping_add(n_col);
    if n_sorted > 0 {
        // Escala o resultado por (Y/X)
        r_sort_cost = r_sort_cost.wrapping_add(
            log_est(((n_order_by - n_sorted) * 100 / n_order_by) as u64).wrapping_sub(66),
        );
    }

    // Multiplica por log(M), sendo M o número de linhas de saída. Usa o LIMIT como M
    // se for menor. Ou, se esta ordenação é de um DISTINCT, M é o número de linhas
    // distintas de saída, então reduz um pouco o valor.
    if (wctrl_flags & (WHERE_USE_LIMIT as u16)) != 0 {
        r_sort_cost = r_sort_cost.wrapping_add(10); // AJUSTE: 2.0x extra se usa LIMIT
        if n_sorted != 0 {
            r_sort_cost = r_sort_cost.wrapping_add(6); // AJUSTE: 1.5x extra se usa também ordenação parcial
        }
        if i_limit < n_row {
            n_row = i_limit;
        }
    } else if (wctrl_flags & (WHERE_WANT_DISTINCT as u16)) != 0 {
        // AJUSTE: na ordenação de um DISTINCT, supõe que o DISTINCT reduz o número de
        // linhas de saída por um fator de 2
        if n_row > 10 {
            n_row = n_row.wrapping_sub(10);
        }
    }
    r_sort_cost = r_sort_cost.wrapping_add(est_log(n_row));
    r_sort_cost
}

/// Dada a lista de objetos WhereLoop em p_w_info.p_loops, esta rotina tenta achar o
/// caminho de menor custo que visita cada WhereLoop uma vez. O caminho é então
/// carregado nos campos p_w_info.a[].p_w_loop.
///
/// Supõe que o número total de linhas de saída a ordenar será n_row_est (na
/// representação 10*log2). Ou ignora os custos de ordenação se n_row_est==0.
///
/// Devolve SQLITE_OK em sucesso, ou SQLITE_ERROR se não há solução. A falha de
/// alocação (SQLITE_NOMEM) do espaço temporário do C não existe aqui: os vetores
/// crescem pelo alocador do Rust.
pub fn where_path_solver(p_w_info: &WhereInfoRef, n_row_est: LogEst) -> i32 {
    let mut mx_i: usize = 0; // Índice da próxima entrada a substituir
    let mut mx_cost: LogEst = 0; // Custo máximo de um conjunto de caminhos
    let mut mx_unsorted: LogEst = 0; // Custo máximo sem ordenação de um conjunto de caminhos

    let p_parse = p_w_info.borrow().p_parse.clone();
    let n_loop: usize = p_w_info.borrow().n_level as usize;
    // AJUSTE: para consultas simples, só o melhor caminho é seguido. Para joins de 2
    // vias, os 5 melhores caminhos. Para joins de 3 ou mais tabelas, os 10 melhores.
    let mx_choice: usize = if n_loop <= 1 {
        1
    } else if n_loop == 2 {
        5
    } else {
        10
    };

    // Se n_row_est é zero e há cláusula ORDER BY, ela é ignorada. Nesse caso o
    // propósito da chamada é estimar o número de linhas devolvidas pela consulta
    // inteira. Obtida a estimativa, o chamador invoca esta função uma segunda vez,
    // passando a estimativa como n_row_est.
    let (wctrl_flags, p_order_by, p_result_set) = {
        let w = p_w_info.borrow();
        (w.wctrl_flags, w.p_order_by.clone(), w.p_result_set.clone())
    };
    let n_order_by: i32 = if p_order_by.is_none() || n_row_est == 0 {
        0
    } else {
        p_order_by.as_ref().unwrap().borrow().n_expr
    };

    // Aloca e inicializa aTo, aFrom e aSortCost[]. As duas gerações de caminhos são
    // dois vetores de mx_choice entradas cuja função (aFrom e aTo) se troca a cada
    // rodada.
    let mut a_to: Vec<WherePath> = (0..mx_choice).map(|_| WherePath::default()).collect();
    let mut a_from: Vec<WherePath> = (0..mx_choice).map(|_| WherePath::default()).collect();
    // Se há ORDER BY e ele não está sendo ignorado, reserva o vetor aSortCost[]. Cada
    // elemento é zero (ainda não inicializado) ou o custo de ordenar n_row_est linhas
    // de dados com os primeiros X termos do ORDER BY já em ordem, sendo X o índice.
    let mut a_sort_cost: Vec<LogEst> = vec![0; n_order_by as usize];

    // Semeia a busca com um único WherePath sem nenhum WhereLoop.
    //
    // AJUSTE: o número de iterações não passa de 28. Se o custo de computar um índice
    // automático não é pago nas primeiras 28 linhas, o índice automático não é usado.
    a_from[0].n_row = std::cmp::min(p_parse.borrow().n_query_loop, 48);
    let mut n_from: usize = 1;
    if n_order_by != 0 {
        // Se n_loop é zero não há termos FROM na consulta. Como nesse caso a consulta
        // devolve no máximo uma linha, o resultado já está na ordem pedida: is_ordered
        // recebe n_order_by para indicar isso. Se n_loop é maior que zero, is_ordered
        // recebe -1, indicando que o conjunto resultado pode ou não estar ordenado,
        // dependendo dos loops adicionados ao plano atual.
        a_from[0].is_ordered = if n_loop > 0 { -1 } else { n_order_by as i8 };
    }

    // Computa WherePaths sucessivamente mais longos usando a geração anterior como
    // base da seguinte. Guarda os mx_choice melhores caminhos de cada geração.
    for i_loop in 0..n_loop {
        let mut n_to: usize = 0;
        for ii in 0..n_from {
            let mut p_w_loop_cur: Option<WhereLoopRef> = p_w_info.borrow().p_loops.clone();
            while let Some(p_w_loop) = p_w_loop_cur {
                p_w_loop_cur = p_w_loop.borrow().p_next_loop.clone();

                let n_out: LogEst; // Linhas visitadas por (p_from+p_w_loop)
                let r_cost: LogEst; // Custo do caminho (p_from+p_w_loop)
                let mut r_unsorted: LogEst; // Custo sem ordenação de (p_from+p_w_loop)
                let is_ordered: i8; // is_ordered de (p_from+p_w_loop)
                let mask_new: Bitmask; // Máscara das tabelas visitadas por (..)
                let rev_mask: Bitmask; // Máscara dos loops em ordem reversa de (..)

                let (l_prereq, l_mask_self, l_ws_flags, l_r_setup, l_r_run, l_n_out) = {
                    let l = p_w_loop.borrow();
                    (l.prereq, l.mask_self, l.ws_flags, l.r_setup, l.r_run, l.n_out)
                };

                if (l_prereq & !a_from[ii].mask_loop) != 0 {
                    continue;
                }
                if (l_mask_self & a_from[ii].mask_loop) != 0 {
                    continue;
                }
                if (l_ws_flags & WHERE_AUTO_INDEX) != 0 && a_from[ii].n_row < 3 {
                    // Não usa índice automático se este loop deve rodar menos de 1.25
                    // vezes. É tentador excluir também o uso de índice automático num
                    // loop externo, mas às vezes ele é útil no loop externo de uma
                    // subconsulta correlacionada.
                    continue;
                }

                // Neste ponto p_w_loop é candidato a próximo loop. Computa seu custo.
                r_unsorted = log_est_add(l_r_setup, l_r_run.wrapping_add(a_from[ii].n_row));
                r_unsorted = log_est_add(r_unsorted, a_from[ii].r_unsorted);
                n_out = a_from[ii].n_row.wrapping_add(l_n_out);
                mask_new = a_from[ii].mask_loop | l_mask_self;
                let from_is_ordered = a_from[ii].is_ordered;
                if from_is_ordered < 0 {
                    let mut rev = 0;
                    is_ordered = where_path_satisfies_order_by(
                        p_w_info,
                        p_order_by.as_ref().unwrap(),
                        &a_from[ii],
                        wctrl_flags,
                        i_loop as i32,
                        &p_w_loop,
                        &mut rev,
                    ) as i8;
                    rev_mask = rev;
                } else {
                    is_ordered = from_is_ordered;
                    rev_mask = a_from[ii].rev_loop;
                }
                if is_ordered >= 0 && (is_ordered as i32) < n_order_by {
                    if a_sort_cost[is_ordered as usize] == 0 {
                        a_sort_cost[is_ordered as usize] =
                            where_sorting_cost(p_w_info, n_row_est, n_order_by, is_ordered as i32);
                    }
                    // AJUSTE: soma uma pequena penalidade extra (3) à ordenação, como
                    // incentivo adicional para o planejador escolher um plano em que as
                    // linhas saem na ordem certa sem precisar ordenar.
                    r_cost = log_est_add(r_unsorted, a_sort_cost[is_ordered as usize]).wrapping_add(3);
                } else {
                    r_cost = r_unsorted;
                    r_unsorted = r_unsorted.wrapping_sub(2); // AJUSTE: leve viés a favor de planos sem ordenação
                }

                // Verifica se p_w_loop deve entrar no conjunto dos mx_choice melhores
                // caminhos até agora.
                //
                // Primeiro procura, entre os melhores até agora, um caminho que cubra
                // o mesmo conjunto de loops e tenha o mesmo is_ordered do candidato.
                //
                // O termo "((p_to.is_ordered^is_ordered)&0x80)==0" equivale a
                // "(p_to.is_ordered==(-1))==(is_ordered==(-1))" para a faixa de
                // valores legais de is_ordered, -1..64.
                let mut jj: usize = 0;
                while jj < n_to {
                    if a_to[jj].mask_loop == mask_new
                        && (((a_to[jj].is_ordered ^ is_ordered) as i32) & 0x80) == 0
                    {
                        break;
                    }
                    jj += 1;
                }
                if jj >= n_to {
                    // Nenhum dos melhores até agora casa com o candidato.
                    if n_to >= mx_choice
                        && (r_cost > mx_cost || (r_cost == mx_cost && r_unsorted >= mx_unsorted))
                    {
                        // O candidato atual não é melhor que nenhum dos mx_choice
                        // caminhos do buffer dos melhores até agora. Descarta-o como
                        // inviável.
                        continue;
                    }
                    // Chegando aqui, o novo candidato precisa entrar no conjunto dos
                    // melhores até agora.
                    if n_to < mx_choice {
                        // Aumenta o conjunto aTo em um
                        jj = n_to;
                        n_to += 1;
                    } else {
                        // O novo caminho substitui o pior anterior para manter a
                        // contagem abaixo de mx_choice
                        jj = mx_i;
                    }
                } else {
                    // Chegando aqui, o melhor até agora p_to=aTo[jj] cobre o mesmo
                    // conjunto de loops e tem o mesmo is_ordered do candidato.
                    // Verifica se o candidato deve substituir p_to ou ser ignorado.
                    //
                    // A condição é uma comparação vetorial expandida equivalente a:
                    //   (p_to.r_cost,p_to.n_row,p_to.r_unsorted) <= (r_cost,n_out,r_unsorted)
                    let p_to = &a_to[jj];
                    if p_to.r_cost < r_cost
                        || (p_to.r_cost == r_cost
                            && (p_to.n_row < n_out
                                || (p_to.n_row == n_out && p_to.r_unsorted <= r_unsorted)))
                    {
                        // Descarta o caminho candidato de qualquer consideração futura
                        continue;
                    }
                    // Chegando aqui, o caminho candidato é melhor que o caminho p_to.
                    // Substitui p_to pelo candidato.
                }
                // p_w_loop é vencedor. Adiciona-o ao conjunto dos melhores até agora.
                let from_mask_loop = a_from[ii].mask_loop;
                {
                    let p_from = &a_from[ii];
                    let p_to = &mut a_to[jj];
                    p_to.mask_loop = from_mask_loop | l_mask_self;
                    p_to.rev_loop = rev_mask;
                    p_to.n_row = n_out;
                    p_to.r_cost = r_cost;
                    p_to.r_unsorted = r_unsorted;
                    p_to.is_ordered = is_ordered;
                    p_to.a_loop.clear();
                    p_to.a_loop.extend_from_slice(&p_from.a_loop[..i_loop]);
                    p_to.a_loop.push(p_w_loop.clone());
                }
                if n_to >= mx_choice {
                    mx_i = 0;
                    mx_cost = a_to[0].r_cost;
                    mx_unsorted = a_to[0].n_row;
                    for jj in 1..mx_choice {
                        let p_to = &a_to[jj];
                        if p_to.r_cost > mx_cost
                            || (p_to.r_cost == mx_cost && p_to.r_unsorted > mx_unsorted)
                        {
                            mx_cost = p_to.r_cost;
                            mx_unsorted = p_to.r_unsorted;
                            mx_i = jj;
                        }
                    }
                }
            }
        }

        // Troca os papéis de aFrom e aTo para a próxima geração
        std::mem::swap(&mut a_from, &mut a_to);
        n_from = n_to;
    }

    if n_from == 0 {
        error_msg(&mut p_parse.borrow_mut(), b"no query solution", &[]);
        return SQLITE_ERROR;
    }

    // Acha o caminho de menor custo. p_from fica apontando para ele.
    let mut i_best: usize = 0;
    for ii in 1..n_from {
        if a_from[i_best].r_cost > a_from[ii].r_cost {
            i_best = ii;
        }
    }
    // Carrega o caminho de menor custo em p_w_info
    for i_loop in 0..n_loop {
        let p_w_loop = a_from[i_best].a_loop[i_loop].clone();
        let i_tab = p_w_loop.borrow().i_tab;
        let i_cursor = {
            let w = p_w_info.borrow();
            let tab_list = w.p_tab_list.borrow();
            tab_list.a[i_tab as usize].i_cursor
        };
        let mut w = p_w_info.borrow_mut();
        let p_level = &mut w.a[i_loop];
        p_level.p_w_loop = Some(p_w_loop);
        p_level.i_from = i_tab;
        p_level.i_tab_cur = i_cursor;
    }
    if (wctrl_flags & (WHERE_WANT_DISTINCT as u16)) != 0
        && (wctrl_flags & (WHERE_DISTINCTBY as u16)) == 0
        && p_w_info.borrow().e_distinct == (WHERE_DISTINCT_NOOP as u8)
        && n_row_est != 0
        && n_loop > 0
    {
        // O guarda n_loop>0 não existe no C: com n_loop==0 o C lê aLoop[-1] fora do
        // vetor e a rotina de ordem não percorre nenhum loop (iLoop<=nLoop com
        // nLoop==-1), então nada seria satisfeito e o ramo não mudaria e_distinct.
        let result_set = p_result_set.as_ref().unwrap();
        let mut not_used: Bitmask = 0;
        let rc = where_path_satisfies_order_by(
            p_w_info,
            result_set,
            &a_from[i_best],
            WHERE_DISTINCTBY as u16,
            (n_loop - 1) as i32,
            &a_from[i_best].a_loop[n_loop - 1],
            &mut not_used,
        ) as i32;
        if rc == result_set.borrow().n_expr {
            p_w_info.borrow_mut().e_distinct = WHERE_DISTINCT_ORDERED as u8;
        }
    }
    p_w_info.borrow_mut().b_ordered_inner_loop = false;
    if let Some(p_order_by) = p_order_by.as_ref() {
        let ob_n_expr: i32 = p_order_by.borrow().n_expr;
        p_w_info.borrow_mut().n_ob_sat = a_from[i_best].is_ordered;
        if (wctrl_flags & (WHERE_DISTINCTBY as u16)) != 0 {
            if (a_from[i_best].is_ordered as i32) == ob_n_expr {
                p_w_info.borrow_mut().e_distinct = WHERE_DISTINCT_ORDERED as u8;
            }
        } else {
            p_w_info.borrow_mut().rev_mask = a_from[i_best].rev_loop;
            let n_ob_sat = p_w_info.borrow().n_ob_sat;
            if n_ob_sat <= 0 {
                p_w_info.borrow_mut().n_ob_sat = 0;
                if n_loop > 0 {
                    let p_last = a_from[i_best].a_loop[n_loop - 1].clone();
                    let ws_flags: u32 = p_last.borrow().ws_flags;
                    if (ws_flags & WHERE_ONEROW) == 0
                        && (ws_flags & (WHERE_IPK | WHERE_COLUMN_IN)) != (WHERE_IPK | WHERE_COLUMN_IN)
                    {
                        let mut m: Bitmask = 0;
                        let rc = where_path_satisfies_order_by(
                            p_w_info,
                            p_order_by,
                            &a_from[i_best],
                            WHERE_ORDERBY_LIMIT as u16,
                            (n_loop - 1) as i32,
                            &p_last,
                            &mut m,
                        ) as i32;
                        if rc == ob_n_expr {
                            let mut w = p_w_info.borrow_mut();
                            w.b_ordered_inner_loop = true;
                            w.rev_mask = m;
                        }
                    }
                }
            } else if n_loop != 0
                && n_ob_sat == 1
                && (wctrl_flags & ((WHERE_ORDERBY_MIN | WHERE_ORDERBY_MAX) as u16)) != 0
            {
                p_w_info.borrow_mut().b_ordered_inner_loop = true;
            }
        }
        if (wctrl_flags & (WHERE_SORTBYGROUP as u16)) != 0
            && (p_w_info.borrow().n_ob_sat as i32) == ob_n_expr
            && n_loop > 0
        {
            let mut rev_mask: Bitmask = 0;
            let n_order = where_path_satisfies_order_by(
                p_w_info,
                p_order_by,
                &a_from[i_best],
                0,
                (n_loop - 1) as i32,
                &a_from[i_best].a_loop[n_loop - 1],
                &mut rev_mask,
            ) as i32;
            if n_order == ob_n_expr {
                let mut w = p_w_info.borrow_mut();
                w.sorted = true;
                w.rev_mask = rev_mask;
            }
        }
    }

    p_w_info.borrow_mut().n_row_out = a_from[i_best].n_row;

    // Libera a memória temporária (o Rust faz isso ao sair do escopo) e devolve sucesso
    SQLITE_OK
}


// ---- part_013.rs ----

// Nota sobre as rotinas de depuração: `showAllWhereLoops` e o macro `WHERETRACE_ALL_LOOPS`
// existem só com WHERETRACE_ENABLED (SQLITE_DEBUG/SQLITE_TEST), que o Debian 13 não define.
// Por isso os blocos de rastreamento (`sqlite3WhereTrace`, `WHERETRACE`, `cId`) somem deste trecho.

/// Esta rotina implementa uma heurística para melhorar o planejamento de consultas.
/// É chamada entre a primeira e a segunda chamada de where_path_solver(), daí o nome
/// "Interstage" "Heuristic".
///
/// A primeira chamada do solver calcula o melhor caminho sem considerar a ordem das
/// saídas. A segunda parte dela para tentar achar um caminho alternativo que satisfaça o
/// ORDER BY.
///
/// Para todo termo da cláusula FROM que, no plano resultante, usa uma restrição de
/// igualdade contra um índice, desabilita os outros WhereLoop do mesmo termo que fariam
/// uma varredura completa da tabela. Isso impede que uma busca por índice seja convertida
/// em varredura completa só para satisfazer o ORDER BY: mesmo que a varredura sem ordenação
/// possa ser um pouco melhor quando as estimativas são precisas, ela pode degradar muito o
/// desempenho se a estimativa de saída for grande demais. É melhor errar para o lado da
/// cautela.
///
/// Exceção: se a primeira chamada do solver gerou uma varredura completa num loop externo,
/// a análise para na primeira varredura completa, porque a segunda chamada pode trocá-la
/// por outra para colocar a saída na ordem certa. Ou seja, é permitida a reescrita:
///
///     Primeiro solver()                   Segundo solver()
///       |-- SCAN t1                         |-- SCAN t2
///       |-- SEARCH t2                       `-- SEARCH t1
///       `-- SORT USING B-TREE
///
/// O objetivo é proibir reescritas como esta:
///
///     Primeiro solver()                   Segundo solver()
///       |-- SEARCH t1                       |-- SCAN t2     <--- ruim!
///       |-- SEARCH t2                       `-- SEARCH t1
///       `-- SORT USING B-TREE
///
/// Ver os casos de teste em test/whereN.test para a consulta real que provocou a heurística.
pub(super) fn where_interstage_heuristic(p_w_info: &WhereInfo) {
    for i in 0..(p_w_info.n_level as usize) {
        let p = match &p_w_info.a[i].p_w_loop {
            Some(p) => p.clone(),
            None => break,
        };
        let (p_ws_flags, i_tab) = {
            let p_borrow = p.borrow();
            (p_borrow.ws_flags, p_borrow.i_tab)
        };
        if (p_ws_flags & WHERE_VIRTUALTABLE) != 0 {
            continue;
        }
        if (p_ws_flags & (WHERE_COLUMN_EQ | WHERE_COLUMN_NULL | WHERE_COLUMN_IN)) != 0 {
            let mut p_loop = p_w_info.p_loops.clone();
            while let Some(p_loop_ref) = p_loop {
                let p_next_loop = p_loop_ref.borrow().p_next_loop.clone();
                let (loop_i_tab, loop_ws_flags) = {
                    let loop_borrow = p_loop_ref.borrow();
                    (loop_borrow.i_tab, loop_borrow.ws_flags)
                };
                if loop_i_tab != i_tab {
                    p_loop = p_next_loop;
                    continue;
                }
                if (loop_ws_flags & (WHERE_CONSTRAINT | WHERE_AUTO_INDEX)) != 0 {
                    // Auto-index e loops restritos por índice podem continuar
                    p_loop = p_next_loop;
                    continue;
                }
                // Impede que o segundo solver() use este loop
                p_loop_ref.borrow_mut().prereq = ALLBITS;
                p_loop = p_next_loop;
            }
        } else {
            break;
        }
    }
}

/// Auxiliar de where_short_cut(): avança o scanner enquanto o termo achado depender de
/// outras tabelas (`prereq_right != 0`), como o laço
/// `while( pTerm && pTerm->prereqRight ) pTerm = whereScanNext(&scan);` do C.
fn where_scan_next_independent(
    p_scan: &mut WhereScan,
    mut p_term: Option<WhereTermRef>,
) -> Option<WhereTermRef> {
    loop {
        let depends = match &p_term {
            Some(t) => t.borrow().prereq_right != 0,
            None => false,
        };
        if !depends {
            return p_term;
        }
        p_term = where_scan_next(p_scan);
    }
}

/// A maioria das consultas usa uma única tabela (não são joins) e tem restrições simples
/// `==` contra campos indexados. Esta rotina tenta planejar esses casos simples com muito
/// menos cerimônia que o planejador geral, e assim acelera o sqlite3_prepare() no caso
/// comum.
///
/// Devolve diferente de zero em caso de sucesso, se a consulta pode ser tratada por este
/// planejador simplificado. Devolve zero se a consulta precisa do planejador geral.
///
/// No C, `pWC` é `&pWInfo->sWC`, e o construtor guarda esse mesmo objeto em `p_wc`.
pub(super) fn where_short_cut(p_builder: &mut WhereLoopBuilder) -> i32 {
    let p_w_info_ref = p_builder.p_w_info.clone();
    let p_wc = p_builder.p_wc.clone();
    let mut scan = WhereScan::default();

    let (wctrl_flags, p_tab_list_ref) = {
        let w = p_w_info_ref.borrow();
        (w.wctrl_flags as u32, w.p_tab_list.clone())
    };
    if (wctrl_flags & WHERE_OR_SUBCLAUSE) != 0 {
        return 0;
    }
    let tab_list = p_tab_list_ref.borrow();
    debug_assert!(tab_list.n_src >= 1);
    let p_item = &tab_list.a[0];
    let p_tab_ref = match &p_item.p_tab {
        Some(t) => t.clone(),
        None => return 0,
    };
    let p_tab = p_tab_ref.borrow();
    if is_virtual(&p_tab) {
        return 0;
    }
    if p_item.fg.is_indexed_by != 0 || p_item.fg.not_indexed != 0 {
        return 0;
    }
    let i_cur = p_item.i_cursor;
    let p_loop_ref = p_builder.p_new.clone();
    let mut p_loop = p_loop_ref.borrow_mut();
    // O laço modelo é usado como estrutura btree: garante que a união `u` é a variante Btree
    if !matches!(p_loop.u, WhereLoopUnion::Btree { .. }) {
        p_loop.u = WhereLoopUnion::Btree {
            n_eq: 0,
            n_btm: 0,
            n_top: 0,
            n_distinct_col: 0,
            p_index: None,
        };
    }
    // aLTerm aponta para aLTermSpace, que tem espaço para 3 termos
    if p_loop.a_l_term.len() < p_loop.a_l_term_space.len() {
        let n_space = p_loop.a_l_term_space.len();
        p_loop.a_l_term.resize(n_space, None);
    }
    p_loop.ws_flags = 0;
    p_loop.n_skip = 0;
    let p_term = where_scan_init(
        &mut scan,
        Some(p_wc.clone()),
        i_cur,
        -1,
        (WO_EQ | WO_IS) as u32,
        None,
    );
    let p_term = where_scan_next_independent(&mut scan, p_term);
    if p_term.is_some() {
        p_loop.ws_flags = WHERE_COLUMN_EQ | WHERE_IPK | WHERE_ONEROW;
        p_loop.a_l_term[0] = p_term;
        p_loop.n_l_term = 1;
        if let WhereLoopUnion::Btree { n_eq, .. } = &mut p_loop.u {
            *n_eq = 1;
        }
        // TUNING: o custo de uma busca por rowid é 10
        p_loop.r_run = 33; // 33==sqlite3LogEst(10)
    } else {
        let mut p_idx_cur = p_tab.p_index.clone();
        while let Some(p_idx_ref) = p_idx_cur {
            p_idx_cur = p_idx_ref.borrow().p_next.clone();
            let p_idx = p_idx_ref.borrow();
            if !is_unique_index(&p_idx)
                || p_idx.p_part_idx_where.is_some()
                || (p_idx.n_key_col as usize) > p_loop.a_l_term_space.len()
            {
                continue;
            }
            let op_mask: u32 = if p_idx.uniq_not_null {
                (WO_EQ | WO_IS) as u32
            } else {
                WO_EQ as u32
            };
            let mut j: usize = 0;
            while j < (p_idx.n_key_col as usize) {
                let p_term = where_scan_init(
                    &mut scan,
                    Some(p_wc.clone()),
                    i_cur,
                    j as i16,
                    op_mask,
                    Some(&p_idx),
                );
                let p_term = where_scan_next_independent(&mut scan, p_term);
                if p_term.is_none() {
                    break;
                }
                p_loop.a_l_term[j] = p_term;
                j += 1;
            }
            if j != (p_idx.n_key_col as usize) {
                continue;
            }
            p_loop.ws_flags = WHERE_COLUMN_EQ | WHERE_ONEROW | WHERE_INDEXED;
            if p_idx.is_covering || (p_item.col_used & p_idx.col_not_idxed) == 0 {
                p_loop.ws_flags |= WHERE_IDX_ONLY;
            }
            p_loop.n_l_term = j as u16;
            if let WhereLoopUnion::Btree { n_eq, p_index, .. } = &mut p_loop.u {
                *n_eq = j as u16;
                *p_index = Some(p_idx_ref.clone());
            }
            // TUNING: o custo de uma busca por índice único é 15
            p_loop.r_run = 39; // 39==sqlite3LogEst(15)
            break;
        }
    }
    if p_loop.ws_flags != 0 {
        p_loop.n_out = 1 as LogEst;
        // sqlite3WhereGetMask(&pWInfo->sMaskSet, iCur) vale 1: só há uma tabela
        p_loop.mask_self = 1;
        if scan.i_equiv > 1 {
            p_loop.ws_flags |= WHERE_TRANSCONS;
        }
        let n_ob_sat = {
            let w = p_w_info_ref.borrow();
            w.p_order_by.as_ref().map(|ob| ob.borrow().n_expr as i8)
        };
        let mut w = p_w_info_ref.borrow_mut();
        debug_assert!(w.s_mask_set.n == 1 && i_cur == w.s_mask_set.ix[0]);
        w.a[0].p_w_loop = Some(p_loop_ref.clone());
        w.a[0].i_tab_cur = i_cur;
        w.n_row_out = 1;
        if let Some(n) = n_ob_sat {
            w.n_ob_sat = n;
        }
        if (wctrl_flags & WHERE_WANT_DISTINCT) != 0 {
            w.e_distinct = WHERE_DISTINCT_UNIQUE as u8;
        }
        return 1;
    }
    0
}

/// Função auxiliar de expr_is_deterministic().
pub(super) fn expr_node_is_deterministic(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_FUNCTION && !expr_has_property(p_expr, EP_CONST_FUNC) {
        p_walker.e_code = 0;
        return WRC_ABORT;
    }
    WRC_CONTINUE
}

/// Devolve verdadeiro se a expressão não contém funções SQL não determinísticas. Não
/// considera as funções não determinísticas que fazem parte de sub-selects.
pub(super) fn expr_is_deterministic(p: &ExprRef) -> i32 {
    let mut w = Walker {
        p_parse: None,
        x_expr_callback: Some(expr_node_is_deterministic),
        x_select_callback: Some(select_walk_fail),
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 1,
        m_w_flags: 0,
        u: WalkerU::None,
    };
    walk_expr(&mut w, Some(p));
    w.e_code as i32
}

/// Tenta omitir de um join as tabelas que não afetam o resultado. Para uma tabela não
/// afetar o resultado, é preciso que:
///
///   1) A consulta não seja um agregado.
///   2) A tabela seja o lado direito de um LEFT JOIN.
///   3) A consulta seja DISTINCT, ou então o ON ou USING contenha uma restrição que limite
///      a varredura da tabela a no máximo uma linha.
///   4) A tabela não seja referenciada por nenhuma parte da consulta além do próprio USING
///      ou ON.
///   5) A tabela não tenha um ON ou USING de inner join se houver um RIGHT JOIN em qualquer
///      lugar da consulta. Senão o ON/USING poderia passar do lado direito para o lado
///      esquerdo do RIGHT JOIN. Nota: por causa de (2), essa condição só surge se a tabela
///      for a mais à direita de uma subconsulta que foi achatada na consulta principal e
///      que era o operando direito de um inner join com ON ou USING.
///   6) O ORDER BY tenha 63 termos ou menos.
///   7) A otimização omit-noop-join esteja habilitada.
///
/// Os itens (1), (6) e (7) são conferidos pelo chamador.
///
/// Por exemplo, dadas
///
///     CREATE TABLE t1(ipk INTEGER PRIMARY KEY, v1);
///     CREATE TABLE t2(ipk INTEGER PRIMARY KEY, v2);
///     CREATE TABLE t3(ipk INTEGER PRIMARY KEY, v3);
///
/// a tabela t2 pode ser omitida de:
///
///     SELECT v1, v3 FROM t1
///       LEFT JOIN t2 ON (t1.ipk=t2.ipk)
///       LEFT JOIN t3 ON (t1.ipk=t3.ipk)
///
/// ou de:
///
///     SELECT DISTINCT v1, v3 FROM t1
///       LEFT JOIN t2
///       LEFT JOIN t3 ON (t1.ipk=t3.ipk)
pub(super) fn where_omit_noop_join(p_w_info: &mut WhereInfo, mut not_ready: Bitmask) -> Bitmask {
    // Pré-condições conferidas pelo chamador
    debug_assert!(p_w_info.n_level >= 2);
    // Estas duas pré-condições, conferidas pelo chamador, garantem a condição (1) do
    // comentário de cabeçalho
    debug_assert!(p_w_info.p_result_set.is_some());
    debug_assert!(0 == ((p_w_info.wctrl_flags as u32) & WHERE_AGG_DISTINCT));

    let mut tab_used: Bitmask = match &p_w_info.p_result_set {
        Some(rs) => where_expr_list_usage(&p_w_info.s_mask_set, &rs.borrow()),
        None => 0,
    };
    if let Some(ob) = &p_w_info.p_order_by {
        tab_used |= where_expr_list_usage(&p_w_info.s_mask_set, &ob.borrow());
    }
    let has_right_join = {
        let tab_list = p_w_info.p_tab_list.borrow();
        (tab_list.a[0].fg.jointype & JT_LTORJ) != 0
    };
    let n_level_start = p_w_info.n_level as usize;
    for i in (1..n_level_start).rev() {
        let p_loop_ref = p_w_info.a[i]
            .p_w_loop
            .clone()
            .expect("where_omit_noop_join: p_w_loop");
        let (loop_i_tab, loop_ws_flags, loop_mask_self) = {
            let p_loop = p_loop_ref.borrow();
            (p_loop.i_tab as usize, p_loop.ws_flags, p_loop.mask_self)
        };
        let (item_jointype, item_i_cursor) = {
            let tab_list = p_w_info.p_tab_list.borrow();
            let p_item = &tab_list.a[loop_i_tab];
            (p_item.fg.jointype, p_item.i_cursor)
        };
        if (item_jointype & (JT_LEFT | JT_RIGHT)) != JT_LEFT {
            continue;
        }
        if ((p_w_info.wctrl_flags as u32) & WHERE_WANT_DISTINCT) == 0
            && (loop_ws_flags & WHERE_ONEROW) == 0
        {
            continue;
        }
        if (tab_used & loop_mask_self) != 0 {
            continue;
        }
        let p_end = p_w_info.s_wc.n_term as usize;
        let mut k: usize = 0;
        while k < p_end {
            let p_term = p_w_info.s_wc.a[k].borrow();
            let p_expr_ref = p_term.p_expr.as_ref().expect("where_omit_noop_join: p_expr");
            let p_expr = p_expr_ref.borrow();
            if (p_term.prereq_all & loop_mask_self) != 0 {
                if !expr_has_property(&p_expr, EP_OUTER_ON) || p_expr.w.i_join != item_i_cursor {
                    break;
                }
            }
            if has_right_join
                && expr_has_property(&p_expr, EP_INNER_ON)
                && p_expr.w.i_join == item_i_cursor
            {
                break; // restrição (5)
            }
            k += 1;
        }
        if k < p_end {
            continue;
        }
        not_ready &= !loop_mask_self;
        for k in 0..p_end {
            let mut p_term = p_w_info.s_wc.a[k].borrow_mut();
            if (p_term.prereq_all & loop_mask_self) != 0 {
                p_term.wt_flags |= TERM_CODED;
            }
        }
        if i != (p_w_info.n_level as usize) - 1 {
            // memmove(&a[i], &a[i+1], (nLevel-1-i)*sizeof(WhereLevel)): o nível removido vai
            // para a posição nLevel-1, que deixa de ser ativa depois do decremento abaixo
            let n_level = p_w_info.n_level as usize;
            p_w_info.a[i..n_level].rotate_left(1);
        }
        p_w_info.n_level -= 1;
        debug_assert!(p_w_info.n_level > 0);
    }
    not_ready
}

/// Confere se há loops SEARCH que se beneficiariam de um filtro de Bloom. Considera um
/// filtro de Bloom se:
///
///   (1)  A SEARCH acontece mais de N vezes, onde N é o número de linhas da tabela
///        considerada para o filtro.
///   (2)  Espera-se que algumas buscas não achem nenhuma linha. (Determinado pelo flag
///        WHERE_SELFCULL no termo.)
///   (3)  O processamento de filtro de Bloom não está desabilitado. (Conferido pelo
///        chamador.)
///   (4)  O tamanho da tabela pesquisada é conhecido pelo ANALYZE.
///
/// Este bloco só confere se um filtro de Bloom seria apropriado e, se for, liga o flag
/// WHERE_BLOOMFILTER no WhereLoop. A implementação do filtro de Bloom fica mais adiante,
/// onde se gera o código de cada WhereLoop.
pub(super) fn where_check_if_bloom_filter_is_useful(p_w_info: &WhereInfo) {
    let mut n_search: LogEst = 0;

    debug_assert!(p_w_info.n_level >= 2);
    for i in 0..(p_w_info.n_level as usize) {
        let p_loop_ref = p_w_info.a[i]
            .p_w_loop
            .clone()
            .expect("where_check_if_bloom_filter_is_useful: p_w_loop");
        let req_flags: u32 = WHERE_SELFCULL | WHERE_COLUMN_EQ;
        let loop_i_tab = p_loop_ref.borrow().i_tab as usize;
        let p_tab_ref = {
            let tab_list = p_w_info.p_tab_list.borrow();
            tab_list.a[loop_i_tab]
                .p_tab
                .clone()
                .expect("where_check_if_bloom_filter_is_useful: p_tab")
        };
        if (p_tab_ref.borrow().tab_flags & TF_HAS_STAT1) == 0 {
            break;
        }
        p_tab_ref.borrow_mut().tab_flags |= TF_MAYBE_REANALYZE;
        let mut p_loop = p_loop_ref.borrow_mut();
        if i >= 1
            && (p_loop.ws_flags & req_flags) == req_flags
            // Sempre verdadeiro se WHERE_COLUMN_EQ está definido
            && (p_loop.ws_flags & (WHERE_IPK | WHERE_INDEXED)) != 0
        {
            if n_search > p_tab_ref.borrow().n_row_log_est {
                p_loop.ws_flags |= WHERE_BLOOMFILTER;
                p_loop.ws_flags &= !WHERE_IDX_ONLY;
            }
        }
        n_search = n_search.wrapping_add(p_loop.n_out);
    }
}


// ---- part_014.rs ----

/// Callback de nó de expressão para `expr_can_return_subtype()`.
///
/// Só uma chamada de função é capaz de devolver um subtipo. Se o nó não é uma chamada de
/// função, devolve WRC_PRUNE imediatamente.
///
/// Uma chamada de função pode devolver um subtipo se tem a propriedade
/// SQLITE_RESULT_SUBTYPE.
///
/// Supõe-se que toda função é capaz de repassar o subtipo de um dos seus argumentos (usando
/// sqlite3_result_value()). A maioria das funções não é assim, mas não há mecanismo para
/// distinguir as que são das que não são, então se supõe que todas funcionam desse jeito.
/// Isso significa que, se um dos argumentos é outra função capaz de devolver um subtipo,
/// esta função também é capaz de devolver um subtipo.
pub(super) fn expr_node_can_return_subtype(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    if p_expr.op != TK_FUNCTION {
        return WRC_PRUNE;
    }
    debug_assert!(expr_use_x_list(p_expr));
    let p_parse = p_walker.p_parse.as_ref().unwrap().clone();
    let db = p_parse.borrow().db.upgrade().unwrap();
    let n = match &p_expr.x.p_list {
        Some(p_list) => p_list.n_expr,
        None => 0,
    };
    let z_name: Vec<u8> = p_expr.u.z_token.as_deref().unwrap_or(&[]).to_vec();
    let enc_value = enc(&db.borrow());
    let p_def = find_function(&db, &z_name, n, enc_value, false);
    let can_return = match &p_def {
        None => true,
        Some(def) => (def.borrow().func_flags & (SQLITE_RESULT_SUBTYPE as u32)) != 0,
    };
    if can_return {
        p_walker.e_code = 1;
        return WRC_PRUNE;
    }
    WRC_CONTINUE
}

/// Devolve verdadeiro se a expressão `p_expr` é capaz de devolver um subtipo.
///
/// Um retorno verdadeiro não garante que um subtipo será devolvido, só indica que é possível.
/// Falsos positivos são aceitáveis porque apenas desligam uma otimização. Falsos negativos,
/// por outro lado, podem levar a respostas incorretas.
pub(super) fn expr_can_return_subtype(p_parse: &ParseRef, p_expr: &Expr) -> i32 {
    let mut w = Walker {
        p_parse: Some(p_parse.clone()),
        x_expr_callback: Some(expr_node_can_return_subtype),
        x_select_callback: None,
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WalkerU::None,
    };
    walk_expr(&mut w, p_expr);
    w.e_code as i32
}

/// O índice `p_idx` é usado por uma consulta e contém uma ou mais expressões. Em outras
/// palavras, `p_idx` é um índice sobre expressão. `i_idx_cur` é o número do cursor do índice
/// e `i_data_cur` (via `p_tab_item`) o da tabela correspondente.
///
/// Esta rotina acrescenta entradas IndexedExpr em `Parse.p_idx_epr` para cada uma das
/// expressões do índice, de modo que o gerador de código de expressões saiba substituir as
/// ocorrências da expressão indexada por referências à coluna correspondente do índice.
#[inline(never)]
pub(super) fn where_add_indexed_expr(
    p_parse: &ParseRef,      // Acrescenta entradas IndexedExpr em p_parse.p_idx_epr
    p_idx: &IndexRef,        // O índice sobre expressão que contém as expressões
    i_idx_cur: i32,          // Número do cursor de p_idx
    p_tab_item: &SrcItem,    // A entrada da cláusula FROM para a tabela
) {
    debug_assert!(p_idx.borrow().b_has_expr);
    let p_tab: TableRef = p_idx.borrow().p_table.upgrade().unwrap();
    let n_column = p_idx.borrow().n_column as usize;
    let db = p_parse.borrow().db.upgrade().unwrap();
    for i in 0..n_column {
        // A cópia da expressão é feita dentro do bloco, onde as referências à expressão
        // (que mora no índice ou na tabela) ainda estão emprestadas.
        let p_expr_copy: Box<Expr> = {
            let idx_guard = p_idx.borrow();
            let tab_guard = p_tab.borrow();
            let j = idx_guard.ai_column[i];
            let p_expr: &Expr = if j == XN_EXPR {
                idx_guard.a_col_expr.as_ref().unwrap().a[i].p_expr.as_deref().unwrap()
            } else if j >= 0 && (tab_guard.a_col[j as usize].col_flags & COLFLAG_VIRTUAL) != 0 {
                match column_expr(&tab_guard, &tab_guard.a_col[j as usize]) {
                    Some(e) => e,
                    // No C a expressão nula é constante para expr_is_constant(): segue o continue.
                    None => continue,
                }
            } else {
                continue;
            };
            if expr_is_constant(None, p_expr) != 0 {
                continue;
            }
            if p_expr.op == TK_FUNCTION && expr_can_return_subtype(p_parse, p_expr) != 0 {
                // Funções que podem definir um subtipo não devem ser substituídas pelo
                // valor tirado de um índice sobre expressão, já que o índice omite o
                // subtipo. https://sqlite.org/forum/forumpost/68d284c86b082c3e
                continue;
            }
            // No C a alocação do IndexedExpr vem antes da cópia e o `if( p==0 ) break;` cobre a
            // falta de memória. Aqui o Box não falha; a falta de memória da cópia faz o mesmo break.
            match expr_dup(&db.borrow(), p_expr, 0) {
                Some(copy) => copy,
                None => break,
            }
        };
        let b_maybe_null_row = ((p_tab_item.fg.jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0) as u8;
        let has_aff_str = index_affinity_str(&db, p_idx).is_some();
        let mut aff: u8 = 0;
        if has_aff_str {
            aff = p_idx.borrow().z_col_aff[i];
        }
        let mut parse = p_parse.borrow_mut();
        let p_ie_next = parse.p_idx_epr.take();
        let was_empty = p_ie_next.is_none();
        parse.p_idx_epr = Some(Box::new(IndexedExpr {
            p_expr: p_expr_copy,
            i_data_cur: p_tab_item.i_cursor,
            i_idx_cur,
            i_idx_col: i as i32,
            b_maybe_null_row,
            aff,
            p_ie_next,
        }));
        if was_empty {
            // O `pArg = &pParse->pIdxEpr` do C vira a captura do próprio Parse: a limpeza
            // libera a cadeia que começa em Parse.p_idx_epr.
            let p_parse_weak = Rc::downgrade(p_parse);
            let _ = parser_add_cleanup(
                &mut *parse,
                Box::new(move |db_ref: &Sqlite3Ref| {
                    if let Some(p_parse_rc) = p_parse_weak.upgrade() {
                        let mut parse_guard = p_parse_rc.borrow_mut();
                        where_indexed_expr_cleanup(db_ref, &mut parse_guard.p_idx_epr);
                    }
                }),
            );
        }
    }
}

/// Liga a máscara de varredura reversa para todas as tabelas da consulta, com exceção das
/// expressões de tabela comum MATERIALIZED que têm seu próprio ORDER BY interno.
///
/// Implementa o PRAGMA reverse_unordered_selects=ON (e também
/// SQLITE_DBCONFIG_REVERSE_SCANORDER).
#[inline(never)]
pub(super) fn where_reverse_scan_order(p_w_info: &mut WhereInfo) {
    let n_src = p_w_info.p_tab_list.borrow().n_src;
    for ii in 0..n_src {
        let reverse = {
            let tab_list = p_w_info.p_tab_list.borrow();
            let p_item = &tab_list.a[ii as usize];
            // Com is_cte ligado, u2 guarda sempre o CteUse; o outro braço não ocorre.
            let m10d_is_not_yes = match &p_item.u2 {
                SrcItemU2::CteUse(p_cte_use) => p_cte_use.borrow().e_m10d != M10D_YES,
                _ => true,
            };
            p_item.fg.is_cte == 0
                || m10d_is_not_yes
                || p_item.p_select.is_none()
                || p_item.p_select.as_ref().unwrap().p_order_by.is_none()
        };
        if reverse {
            p_w_info.rev_mask |= mask_bit(ii);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// FRAGMENTO: início de sqlite3WhereBegin() (where_begin). A função do C começa neste trecho
// (chunks/where_c.014.c) e continua em where_c.015.c e seguintes: o corte do fatiador caiu no
// meio dela, dentro do `if( nTabList==0 ){`. Uma fn Rust não pode atravessar arquivos, então o
// começo fica aqui, desligado por `cfg(any())` (o parser aceita, o compilador descarta), para o
// tech lead costurar com as partes seguintes numa única fn `where_begin`. O corpo inteiro vai
// dentro de um bloco rotulado `'where_begin_error: { ... }`; o `goto whereBeginError` do C vira
// `break 'where_begin_error`. Esta parte termina exatamente antes de `if( nTabList==0 ){`.
// ---------------------------------------------------------------------------------------------

/// Gera o início do laço usado no processamento da cláusula WHERE (ver a documentação completa
/// em sqlite3WhereBegin). Devolve None em caso de erro.
#[cfg(any())]
fn where_begin_head(
    p_parse: &ParseRef,                   // O contexto do analisador
    p_tab_list: &SrcListRef,              // Cláusula FROM: todas as tabelas a varrer
    p_where: Option<&mut Expr>,           // A cláusula WHERE
    mut p_order_by: Option<ExprListRef>,  // ORDER BY (ou GROUP BY), ou None
    p_result_set: Option<ExprListRef>,    // Conjunto de resultados da consulta. Exigido por DISTINCT
    p_select: Option<SelectRef>,          // O SELECT inteiro
    mut wctrl_flags: u16,                 // Flags WHERE_* definidas em sqliteInt.h
    i_aux_arg: i32,                       // Com WHERE_OR_SUBCLAUSE, o cursor do índice; com
                                          // WHERE_USE_LIMIT, o valor do limite
) -> Option<WhereInfoRef> {
    debug_assert!(
        (wctrl_flags & WHERE_ONEPASS_MULTIROW) == 0
            || ((wctrl_flags & WHERE_ONEPASS_DESIRED) != 0 && (wctrl_flags & WHERE_OR_SUBCLAUSE) == 0)
    );

    // Só um entre WHERE_OR_SUBCLAUSE e WHERE_USE_LIMIT
    debug_assert!((wctrl_flags & WHERE_OR_SUBCLAUSE) == 0 || (wctrl_flags & WHERE_USE_LIMIT) == 0);

    // Inicialização de variáveis
    let db = p_parse.borrow().db.upgrade().unwrap();
    let v = p_parse.borrow().p_vdbe.clone();
    let b_fordelete: u8 = 0; // OPFLAG_FORDELETE ou zero, conforme o caso

    // Uma cláusula ORDER/GROUP BY com mais de 63 termos não pode ser otimizada
    if p_order_by.as_ref().map_or(false, |o| o.borrow().n_expr >= BMS) {
        p_order_by = None;
        wctrl_flags &= !WHERE_WANT_DISTINCT;
        wctrl_flags |= WHERE_KEEP_ALL_JOINS; // Desliga a otimização de omitir join sem efeito
    }

    // O número de tabelas na cláusula FROM é limitado pelo número de bits de um Bitmask
    if p_tab_list.borrow().n_src > BMS {
        error_msg(
            &mut p_parse.borrow_mut(),
            format!("at most {} tables in a join", BMS).as_bytes(),
            &[],
        );
        return None;
    }

    // Esta função normalmente gera um laço aninhado para todas as tabelas de p_tab_list. Mas
    // se WHERE_OR_SUBCLAUSE está ligado, só se gera código para a primeira tabela e se supõe
    // que os cursores das tabelas seguintes não estão inicializados.
    let n_tab_list: i32 = if (wctrl_flags & WHERE_OR_SUBCLAUSE) != 0 { 1 } else { p_tab_list.borrow().n_src };

    // Aloca e inicializa a estrutura WhereInfo que será o valor de retorno. No C uma única
    // alocação guarda o WhereInfo, o conteúdo de a[], o WhereClause e o WhereMaskSet (com o
    // ROUND8P para o alinhamento do Bitmask); aqui cada parte é um valor próprio e o cálculo
    // de nByteWInfo não existe. O memset do C vira a inicialização com valores zerados.
    if db.borrow().malloc_failed != 0 {
        // pWInfo = 0; goto whereBeginError;
        break 'where_begin_error;
    }
    let p_w_info: WhereInfoRef = Rc::new(RefCell::new(WhereInfo {
        p_parse: p_parse.clone(),
        p_tab_list: p_tab_list.clone(),
        p_order_by: p_order_by.clone(),
        p_result_set,
        p_select,
        ai_cur_one_pass: [-1, -1],
        i_continue: 0,
        i_break: 0,
        saved_n_query_loop: p_parse.borrow().n_query_loop as i32,
        wctrl_flags,
        i_limit: i_aux_arg as LogEst,
        n_level: n_tab_list as u8,
        n_ob_sat: 0,
        e_one_pass: ONEPASS_OFF,
        e_distinct: 0,
        b_deferred_seek: false,
        untested_terms: false,
        b_ordered_inner_loop: false,
        sorted: false,
        n_row_out: 0,
        i_top: 0,
        i_end_where: 0,
        p_loops: None,
        p_mem_to_free: None,
        rev_mask: 0,
        s_wc: WhereClause::default(),
        s_mask_set: WhereMaskSet::default(),
        a: (0..n_tab_list).map(|_| WhereLevel::default()).collect(),
    }));
    {
        let mut w = p_w_info.borrow_mut();
        let label = vdbe_make_label(&mut p_parse.borrow_mut());
        w.i_break = label;
        w.i_continue = label;
        let p_mask_set = &mut w.s_mask_set;
        p_mask_set.n = 0;
        p_mask_set.ix[0] = -99; // Valor que nunca é um cursor válido, evitando o teste
                                // de p_mask_set.n==0 em where_get_mask()
    }

    // sWLB: o construtor de WhereLoop. O WhereLoop modelo (p_new) é uma alocação própria.
    // Decisão do tech lead: WhereInfo.s_wc é por valor e WhereLoopBuilder.p_wc é uma Ref, e o C
    // faz `sWLB.pWC = &pWInfo->sWC`. O alias precisa de uma forma única nos dois tipos; aqui
    // `p_wc` recebe a Ref que o lead definir para o s_wc de p_w_info (marcador `S_WC_REF`).
    let mut s_wlb = WhereLoopBuilder {
        p_w_info: p_w_info.clone(),
        p_wc: S_WC_REF,
        p_new: Rc::new(RefCell::new(WhereLoop::default())),
        p_or_set: None,
        bld_flags1: 0,
        bld_flags2: 0,
        i_plan_limit: 0,
    };
    where_loop_init(&mut s_wlb.p_new.borrow_mut());

    // Divide a cláusula WHERE em subexpressões separadas, cada uma delimitada por um AND.
    where_clause_init(&mut p_w_info.borrow_mut().s_wc, Rc::downgrade(&p_w_info));
    where_split(&mut p_w_info.borrow_mut().s_wc, p_where, TK_AND);

    // Caso especial: sem cláusula FROM. A tradução continua em where_c.015.c, com o
    // `if( nTabList==0 ){ ... }else{ ... }` que o corte do fatiador deixou no meio.
}


// ---- part_015.rs ----

// Este trecho (where_c.015.c) é a SEGUNDA METADE do corpo de `sqlite3WhereBegin`: o corte do
// fatiador caiu dentro do ramo `if( nTabList==0 ){` da função. Como uma fn de Rust não pode ser
// partida entre arquivos, a metade final vira `where_begin_tail`, e o rótulo `whereBeginError`
// vira `where_begin_error` (o rótulo do C com o mesmo nome, como manda a convenção).
//
// Contrato com a primeira metade (where_c.014, `where_begin`): logo depois de inicializar
// `sWLB` e chamar `where_clause_init` + `where_split` (a última coisa antes do comentário
// "Special case: No FROM clause"), a primeira metade termina com
// `return where_begin_tail(...)`. Os erros da primeira metade (`goto whereBeginError`) chamam
// `where_begin_error(db, p_parse, p_w_info)`.
//
// Convenções assumidas:
//  - `pLevel` do C vira o índice `ii` em `p_w_info.a` (evita dois empréstimos mutáveis).
//  - `&pWInfo->sWC` e `sWLB.pWC` são o mesmo objeto no C; aqui as duas leituras usam
//    `s_wlb.p_wc` (o `WhereClauseRef` do construtor).
//  - As funções de Vdbe (`vdbe_add_op*`, `vdbe_change_p*`, ...) recebem `&VdbeRef`.
//  - Os macros VdbeComment, VdbeCoverage, VdbeModuleComment, testcase, assert, ALWAYS e os
//    blocos WHERETRACE_ENABLED, SQLITE_DEBUG, STAT4, CURSOR_HINTS e COLUMN_USED_MASK não existem
//    na compilação do Debian, então somem. `sqlite3WhereAddScanStatus` é no-op sem
//    STMT_SCANSTATUS e também some.

/// Rótulo `whereBeginError` de `sqlite3WhereBegin`: restaura `nQueryLoop`, libera o WhereInfo
/// e devolve None (o retorno 0 do C).
pub fn where_begin_error(
    db: &Sqlite3Ref,
    p_parse: &ParseRef,
    p_w_info: Option<WhereInfoRef>,
) -> Option<WhereInfoRef> {
    if let Some(p_w_info) = p_w_info {
        let saved_n_query_loop = p_w_info.borrow().saved_n_query_loop;
        p_parse.borrow_mut().n_query_loop = saved_n_query_loop;
        where_info_free(db, &mut p_w_info.borrow_mut());
    }
    None
}

/// Metade final de `sqlite3WhereBegin`: do caso especial "sem cláusula FROM" até o fim.
/// Recebe as variáveis locais vivas no ponto do corte (`p_order_by` e `wctrl_flags` já com os
/// ajustes de 63 termos feitos pela primeira metade).
pub fn where_begin_tail(
    p_parse: &ParseRef,
    p_tab_list: &SrcListRef,
    p_order_by: Option<ExprListRef>,
    p_result_set: Option<ExprListRef>,
    p_select: Option<SelectRef>,
    wctrl_flags: u16,
    i_aux_arg: i32,
    mut n_tab_list: i32,
    p_w_info: WhereInfoRef,
    mut s_wlb: WhereLoopBuilder,
    db: &Sqlite3Ref,
) -> Option<WhereInfoRef> {
    let mut wctrl_flags: u32 = wctrl_flags as u32;
    let v: VdbeRef = p_parse
        .borrow()
        .p_vdbe
        .clone()
        .expect("where_begin: Parse sem Vdbe");
    let mut b_fordelete: u8 = 0;
    let fail = || where_begin_error(db, p_parse, Some(p_w_info.clone()));
    let malloc_failed = || db.borrow().malloc_failed != 0;

    if n_tab_list == 0 {
        // Caso especial: sem cláusula FROM
        if let Some(ob) = &p_order_by {
            p_w_info.borrow_mut().n_ob_sat = ob.borrow().n_expr as i8;
        }
        if (wctrl_flags & WHERE_WANT_DISTINCT) != 0
            && optimization_enabled(&db.borrow(), SQLITE_DISTINCTOPT as u32)
        {
            p_w_info.borrow_mut().e_distinct = WHERE_DISTINCT_UNIQUE as u8;
        }
        let not_multi_value = match &p_w_info.borrow().p_select {
            Some(s) => (s.borrow().sel_flags & SF_MULTIVALUE) == 0,
            None => false,
        };
        if not_multi_value {
            vdbe_explain(&mut p_parse.borrow_mut(), 0, b"SCAN CONSTANT ROW".to_vec());
        }
    } else {
        // Atribui um bit do bitmask a cada termo da cláusula FROM: o N-ésimo termo recebe 1<<N.
        // Os bitmasks são criados para todos os pTabList->nSrc termos, não só para os nTabList.
        let mut ii: i32 = 0;
        loop {
            let i_cursor = p_tab_list.borrow().a[ii as usize].i_cursor;
            create_mask(&mut p_w_info.borrow_mut().s_mask_set, i_cursor);
            {
                let mut tl = p_tab_list.borrow_mut();
                where_tab_func_args(
                    &mut p_parse.borrow_mut(),
                    &mut tl.a[ii as usize],
                    &mut s_wlb.p_wc.borrow_mut(),
                );
            }
            ii += 1;
            if ii >= p_tab_list.borrow().n_src {
                break;
            }
        }
    }

    // Analisa todas as subexpressões
    where_expr_analyze(&p_tab_list.borrow(), &mut s_wlb.p_wc.borrow_mut());
    if let Some(sel) = &p_select {
        let has_limit = sel.borrow().p_limit.is_some();
        if has_limit {
            where_add_limit(&mut s_wlb.p_wc.borrow_mut(), &sel.borrow());
        }
    }
    if p_parse.borrow().n_err != 0 {
        return fail();
    }

    // Otimização False-WHERE-Term-Bypass: se há termos WHERE falsos nenhuma linha sai, então o
    // código gerado aqui é pulado. Condições: (1) o termo não refere tabelas do join; (2) não
    // vem de um ON do lado direito de LEFT ou FULL JOIN; (3) não vem de um ON, ou não há RIGHT
    // ou FULL OUTER JOIN em pTabList; (4) a expressão não tem funções não determinísticas fora
    // de subconsulta (preserva o comportamento legado de random()).
    let mut ii: i32 = 0;
    while ii < s_wlb.p_wc.borrow().n_base {
        let p_t = s_wlb.p_wc.borrow().a[ii as usize].clone();
        if (p_t.borrow().wt_flags & TERM_VIRTUAL) == 0 {
            let p_x: ExprRef = p_t
                .borrow()
                .p_expr
                .clone()
                .expect("where_begin: termo sem expressão");
            let prereq_all = p_t.borrow().prereq_all;
            let jointype0 = p_tab_list
                .borrow()
                .a
                .first()
                .map_or(0, |item| item.fg.jointype);
            if prereq_all == 0
                && (n_tab_list == 0 || expr_is_deterministic(&p_x))
                && !(expr_has_property(&p_x.borrow(), EP_INNERON)
                    && (jointype0 & JT_LTORJ) != 0)
            {
                let i_break = p_w_info.borrow().i_break;
                expr_if_false(p_parse, &p_x, i_break, SQLITE_JUMPIFNULL as i32);
                p_t.borrow_mut().wt_flags |= TERM_CODED;
            }
        }
        ii += 1;
    }

    if (wctrl_flags & WHERE_WANT_DISTINCT) != 0 {
        if optimization_disabled(&db.borrow(), SQLITE_DISTINCTOPT as u32) {
            // Desliga a otimização DISTINCT se SQLITE_DistinctOpt foi ligado por
            // sqlite3_test_ctrl(SQLITE_TESTCTRL_OPTIMIZATIONS,...)
            wctrl_flags &= !WHERE_WANT_DISTINCT;
            p_w_info.borrow_mut().wctrl_flags &= !(WHERE_WANT_DISTINCT as u16);
        } else if {
            let result_set = p_result_set
                .as_ref()
                .expect("where_begin: DISTINCT sem conjunto de resultados")
                .borrow();
                is_distinct_redundant(
                &p_parse.borrow(),
                &p_tab_list.borrow(),
                Some(s_wlb.p_wc.clone()),
                &result_set,
            )
        } {
            // A marca DISTINCT é inútil. Ignora.
            p_w_info.borrow_mut().e_distinct = WHERE_DISTINCT_UNIQUE as u8;
        } else if p_order_by.is_none() {
            // Tenta ordenar pelo conjunto de resultados para facilitar o processamento DISTINCT
            let mut wi = p_w_info.borrow_mut();
            wi.wctrl_flags |= WHERE_DISTINCTBY as u16;
            wi.p_order_by = p_result_set.clone();
        }
    }

    // Constrói os objetos WhereLoop
    if n_tab_list != 1 || where_short_cut(&mut s_wlb) == 0 {
        let rc = where_loop_add_all(&mut s_wlb);
        if rc != 0 {
            return fail();
        }

        where_path_solver(&p_w_info, 0);
        if malloc_failed() {
            return fail();
        }
        if p_w_info.borrow().p_order_by.is_some() {
            where_interstage_heuristic(&p_w_info.borrow());
            let n_row_est = p_w_info.borrow().n_row_out.wrapping_add(1);
            where_path_solver(&p_w_info, n_row_est);
            if malloc_failed() {
                return fail();
            }
        }

        // AJUSTE: assume que DISTINCT numa subconsulta reduz o tamanho da saída por um fator
        // de 8 (LogEst -30).
        if (p_w_info.borrow().wctrl_flags as u32 & WHERE_WANT_DISTINCT) != 0 {
            let mut wi = p_w_info.borrow_mut();
            wi.n_row_out = wi.n_row_out.wrapping_sub(30);
        }
    }
    if p_w_info.borrow().p_order_by.is_none()
        && (db.borrow().flags & (SQLITE_REVERSEORDER as u64)) != 0
    {
        where_reverse_scan_order(&mut p_w_info.borrow_mut());
    }
    if p_parse.borrow().n_err != 0 {
        return fail();
    }

    // Tenta omitir do join as tabelas que não afetam o resultado. Ver o comentário de
    // whereOmitNoopJoin(), fatorada num procedimento separado para não inchar esta função.
    let mut not_ready: Bitmask = !(0 as Bitmask);
    if p_w_info.borrow().n_level >= 2
        && p_result_set.is_some()
        && 0 == (wctrl_flags & (WHERE_AGG_DISTINCT | WHERE_KEEP_ALL_JOINS))
        && optimization_enabled(&db.borrow(), SQLITE_OMITNOOPJOIN as u32)
    {
        not_ready = where_omit_noop_join(&mut p_w_info.borrow_mut(), not_ready);
        n_tab_list = p_w_info.borrow().n_level as i32;
    }

    // Vê se há loops SEARCH que se beneficiariam de um filtro de Bloom
    if p_w_info.borrow().n_level >= 2
        && optimization_enabled(&db.borrow(), SQLITE_BLOOMFILTER as u32)
    {
        where_check_if_bloom_filter_is_useful(&p_w_info.borrow());
    }
    {
        let n_row_out = p_w_info.borrow().n_row_out;
        let mut pp = p_parse.borrow_mut();
        pp.n_query_loop = pp.n_query_loop.wrapping_add(n_row_out);
    }

    // Se quem chama é um UPDATE ou DELETE pedindo o algoritmo de uma passada, decide se é
    // apropriado: vale se o scan visita no máximo uma linha, ou se o chamador permite várias
    // linhas (WHERE_ONEPASS_MULTIROW), a tabela não é virtual e o scan não usa a otimização
    // OR ou o chamador é um DELETE (WHERE_DUPLICATES_OK só é dado para DELETE).
    if (wctrl_flags & WHERE_ONEPASS_DESIRED) != 0 {
        let p_loop0: WhereLoopRef = p_w_info.borrow().a[0]
            .p_w_loop
            .clone()
            .expect("where_begin: nível sem WhereLoop");
        let ws_flags: u32 = p_loop0.borrow().ws_flags;
        let b_onerow = (ws_flags & WHERE_ONEROW) != 0;
        let p_tab0: TableRef = p_tab_list.borrow().a[0]
            .p_tab
            .clone()
            .expect("where_begin: item sem tabela");
        if b_onerow
            || (0 != (wctrl_flags & WHERE_ONEPASS_MULTIROW)
                && !is_virtual(&p_tab0.borrow())
                && (0 == (ws_flags & WHERE_MULTI_OR) || (wctrl_flags & WHERE_DUPLICATES_OK) != 0)
                && optimization_enabled(&db.borrow(), SQLITE_ONEPASS as u32))
        {
            p_w_info.borrow_mut().e_one_pass = (if b_onerow {
                ONEPASS_SINGLE
            } else {
                ONEPASS_MULTI
            }) as u8;
            if has_rowid(&p_tab0.borrow()) && (ws_flags & WHERE_IDX_ONLY) != 0 {
                if (wctrl_flags & WHERE_ONEPASS_MULTIROW) != 0 {
                    b_fordelete = OPFLAG_FORDELETE;
                }
                p_loop0.borrow_mut().ws_flags = ws_flags & !WHERE_IDX_ONLY;
            }
        }
    }

    // Abre todas as tabelas de pTabList e os índices escolhidos para pesquisá-las
    let mut ii: i32 = 0;
    while ii < n_tab_list {
        let iu = ii as usize;
        let (i_from, p_loop, i_tab_cur): (usize, WhereLoopRef, i32) = {
            let wi = p_w_info.borrow();
            (
                wi.a[iu].i_from as usize,
                wi.a[iu]
                    .p_w_loop
                    .clone()
                    .expect("where_begin: nível sem WhereLoop"),
                wi.a[iu].i_tab_cur,
            )
        };
        let (p_tab, i_cursor, jointype, col_used): (TableRef, i32, u8, u64) = {
            let tl = p_tab_list.borrow();
            let item = &tl.a[i_from];
            (
                item.p_tab.clone().expect("where_begin: item sem tabela"),
                item.i_cursor,
                item.fg.jointype,
                item.col_used,
            )
        };
        let i_db: i32 = {
            let schema = p_tab.borrow().p_schema.as_ref().and_then(|w| w.upgrade());
            let dbb = db.borrow();
            let r = match &schema {
                Some(s) => schema_to_index(&dbb, Some(s)),
                None => schema_to_index(&dbb, None),
            };
            r
        };
        let tab_flags: u32 = p_tab.borrow().tab_flags;
        if (tab_flags & TF_EPHEMERAL) != 0 || is_view(&p_tab.borrow()) {
            // Não faz nada
        } else if (p_loop.borrow().ws_flags & WHERE_VIRTUALTABLE) != 0 {
            let p4 = match get_v_table(db, &p_tab) {
                Some(p_v_tab) => P4Value::VTab(p_v_tab),
                None => P4Value::NotUsed,
            };
            vdbe_add_op4(&mut v.borrow_mut(), OP_VOPEN as i32, i_cursor, 0, 0, p4, P4_VTAB);
        } else if is_virtual(&p_tab.borrow()) {
            // noop
        } else if ((p_loop.borrow().ws_flags & WHERE_IDX_ONLY) == 0
            && (wctrl_flags & WHERE_OR_SUBCLAUSE) == 0)
            || (jointype & (JT_LTORJ | JT_RIGHT)) != 0
        {
            let mut op: u8 = OP_OPENREAD;
            if p_w_info.borrow().e_one_pass != ONEPASS_OFF as u8 {
                op = OP_OPENWRITE;
                p_w_info.borrow_mut().ai_cur_one_pass[0] = i_cursor;
            }
            open_table(
                &mut p_parse.borrow_mut(),
                i_cursor,
                i_db,
                &p_tab.borrow(),
                op as i32,
            );
            let n_col = p_tab.borrow().n_col as i32;
            if p_w_info.borrow().e_one_pass == ONEPASS_OFF as u8
                && n_col < BMS
                && (tab_flags & (TF_HASGENERATED | TF_WITHOUTROWID)) == 0
                && (p_loop.borrow().ws_flags & (WHERE_AUTO_INDEX | WHERE_BLOOMFILTER)) == 0
            {
                // Se só um prefixo do registro será usado, vale reduzir o campo "número de
                // colunas" no P4 do OP_OpenRead/Write.
                let mut b: u64 = col_used;
                let mut n: i32 = 0;
                while b != 0 {
                    b >>= 1;
                    n += 1;
                }
                vdbe_change_p4(&mut v.borrow_mut(), -1, P4Value::Int32(n), P4_INT32 as i32);
            }
            vdbe_change_p5(&mut v.borrow_mut(), b_fordelete as u16);
        } else {
            let tnum = p_tab.borrow().tnum;
            let z_name = p_tab.borrow().z_name.clone();
            table_lock(&mut p_parse.borrow_mut(), i_db, tnum, 0, &z_name);
        }
        if (p_loop.borrow().ws_flags & WHERE_INDEXED) != 0 {
            let p_ix: IndexRef = match &p_loop.borrow().u {
                WhereLoopUnion::Btree { p_index, .. } => p_index
                    .clone()
                    .expect("where_begin: WHERE_INDEXED sem índice"),
                WhereLoopUnion::Vtab { .. } => unreachable!(),
            };
            let i_index_cur: i32;
            let mut op: i32 = OP_OPENREAD as i32;
            // iAuxArg é sempre positivo se ONEPASS é possível
            if !has_rowid(&p_tab.borrow())
                && is_primary_key_index(&p_ix.borrow())
                && (wctrl_flags & WHERE_OR_SUBCLAUSE) != 0
            {
                // Um termo de otimização OR usando a PRIMARY KEY de uma tabela WITHOUT ROWID:
                // não precisa de índice separado
                i_index_cur = i_tab_cur;
                op = 0;
            } else if p_w_info.borrow().e_one_pass != ONEPASS_OFF as u8 {
                let mut p_j: Option<IndexRef> = p_tab.borrow().p_index.clone();
                let mut cur = i_aux_arg;
                while let Some(j) = p_j.clone() {
                    if Rc::ptr_eq(&j, &p_ix) {
                        break;
                    }
                    cur += 1;
                    p_j = j.borrow().p_next.clone();
                }
                i_index_cur = cur;
                op = OP_OPENWRITE as i32;
                p_w_info.borrow_mut().ai_cur_one_pass[1] = i_index_cur;
            } else if i_aux_arg != 0 && (wctrl_flags & WHERE_OR_SUBCLAUSE) != 0 {
                i_index_cur = i_aux_arg;
                op = OP_REOPENIDX as i32;
            } else {
                {
                    let mut pp = p_parse.borrow_mut();
                    i_index_cur = pp.n_tab;
                    pp.n_tab += 1;
                }
                if p_ix.borrow().b_has_expr
                    && optimization_enabled(&db.borrow(), SQLITE_INDEXEDEXPR as u32)
                {
                    where_add_indexed_expr(
                        p_parse,
                        &p_ix,
                        i_index_cur,
                        &p_tab_list.borrow().a[i_from],
                    );
                }
                let has_part_idx_where = p_ix.borrow().p_part_idx_where.is_some();
                if has_part_idx_where && (jointype & JT_RIGHT) == 0 {
                    let ixb = p_ix.borrow();
                    where_part_idx_expr(
                        p_parse,
                        &ixb,
                        ixb.p_part_idx_where
                            .as_deref()
                            .expect("where_begin: índice parcial sem WHERE"),
                        0,
                        i_index_cur,
                        &p_tab_list.borrow().a[i_from],
                    );
                }
            }
            p_w_info.borrow_mut().a[iu].i_idx_cur = i_index_cur;
            if op != 0 {
                let ix_tnum = p_ix.borrow().tnum;
                vdbe_add_op3(&mut v.borrow_mut(), op, i_index_cur, ix_tnum as i32, i_db);
                vdbe_set_p4_key_info(&mut p_parse.borrow_mut(), &p_ix);
                let ws_flags = p_loop.borrow().ws_flags;
                let (wi_wctrl_flags, e_distinct) = {
                    let wi = p_w_info.borrow();
                    (wi.wctrl_flags as u32, wi.e_distinct)
                };
                if (ws_flags & WHERE_CONSTRAINT) != 0
                    && (ws_flags & (WHERE_COLUMN_RANGE | WHERE_SKIPSCAN)) == 0
                    && (ws_flags & WHERE_BIGNULL_SORT) == 0
                    && (ws_flags & WHERE_IN_SEEKSCAN) == 0
                    && (wi_wctrl_flags & WHERE_ORDERBY_MIN) == 0
                    && e_distinct != WHERE_DISTINCT_ORDERED as u8
                {
                    vdbe_change_p5(&mut v.borrow_mut(), OPFLAG_SEEKEQ as u16);
                }
            }
        }
        if i_db >= 0 {
            code_verify_schema(&mut p_parse.borrow_mut(), i_db);
        }
        if (jointype & JT_RIGHT) != 0
            && where_malloc(
                &mut p_w_info.borrow_mut(),
                std::mem::size_of::<WhereRightJoin>() as u64,
            )
            .is_some()
        {
            let i_match: i32;
            let reg_bloom: i32;
            let reg_return: i32;
            {
                let mut pp = p_parse.borrow_mut();
                i_match = pp.n_tab;
                pp.n_tab += 1;
                pp.n_mem += 1;
                reg_bloom = pp.n_mem;
            }
            vdbe_add_op2(&mut v.borrow_mut(), OP_BLOB as i32, 65536, reg_bloom);
            {
                let mut pp = p_parse.borrow_mut();
                pp.n_mem += 1;
                reg_return = pp.n_mem;
            }
            vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, reg_return);
            if has_rowid(&p_tab.borrow()) {
                vdbe_add_op2(&mut v.borrow_mut(), OP_OPENEPHEMERAL as i32, i_match, 1);
                let p_info = key_info_alloc(db, 1, 0);
                if let Some(p_info) = p_info {
                    {
                        let mut ki = p_info.borrow_mut();
                        ki.a_coll[0] = None;
                        ki.a_sort_flags[0] = 0;
                    }
                    vdbe_append_p4(&mut v.borrow_mut(), P4Value::KeyInfo(p_info), P4_KEYINFO as i32);
                }
            } else {
                let p_pk: IndexRef = primary_key_index(&p_tab.borrow())
                    .expect("where_begin: WITHOUT ROWID sem PRIMARY KEY");
                let n_key_col = p_pk.borrow().n_key_col as i32;
                vdbe_add_op2(&mut v.borrow_mut(), OP_OPENEPHEMERAL as i32, i_match, n_key_col);
                vdbe_set_p4_key_info(&mut p_parse.borrow_mut(), &p_pk);
            }
            p_w_info.borrow_mut().a[iu].p_rj = Some(Box::new(WhereRightJoin {
                i_match,
                reg_bloom,
                reg_return,
                addr_subrtn: 0,
                end_subrtn: 0,
            }));
            p_loop.borrow_mut().ws_flags &= !WHERE_IDX_ONLY;
            // A natureza do processamento de RIGHT JOIN bagunça a ordem de saída. Então omite
            // qualquer eliminação de ORDER BY/GROUP BY: é preciso uma ordenação de verdade.
            let mut wi = p_w_info.borrow_mut();
            wi.n_ob_sat = 0;
            wi.e_distinct = WHERE_DISTINCT_UNORDERED as u8;
        }
        ii += 1;
    }
    {
        let addr = vdbe_current_addr(&v.borrow());
        p_w_info.borrow_mut().i_top = addr;
    }
    if malloc_failed() {
        return fail();
    }

    // Gera o código da pesquisa. Cada iteração do laço abaixo gera o código de um único loop
    // aninhado do programa da VM.
    let mut ii: i32 = 0;
    while ii < n_tab_list {
        let iu = ii as usize;
        if p_parse.borrow().n_err != 0 {
            return fail();
        }
        let (i_from, ws_flags): (usize, u32) = {
            let wi = p_w_info.borrow();
            let lvl = &wi.a[iu];
            (
                lvl.i_from as usize,
                lvl.p_w_loop
                    .as_ref()
                    .expect("where_begin: nível sem WhereLoop")
                    .borrow()
                    .ws_flags,
            )
        };
        let (is_materialized, is_correlated, reg_return, addr_fill_sub) = {
            let tl = p_tab_list.borrow();
            let p_src = &tl.a[i_from];
            (
                p_src.fg.is_materialized != 0,
                p_src.fg.is_correlated != 0,
                p_src.reg_return,
                p_src.addr_fill_sub,
            )
        };
        if is_materialized {
            if is_correlated {
                vdbe_add_op2(&mut v.borrow_mut(), OP_GOSUB as i32, reg_return, addr_fill_sub);
            } else {
                let i_once = vdbe_add_op0(&mut v.borrow_mut(), OP_ONCE as i32);
                vdbe_add_op2(&mut v.borrow_mut(), OP_GOSUB as i32, reg_return, addr_fill_sub);
                vdbe_jump_here(&mut v.borrow_mut(), i_once);
            }
        }
        if (ws_flags & (WHERE_AUTO_INDEX | WHERE_BLOOMFILTER)) != 0 {
            if (ws_flags & WHERE_AUTO_INDEX) != 0 {
                construct_automatic_index(
                    p_parse,
                    &s_wlb.p_wc.borrow(),
                    not_ready,
                    &mut p_w_info.borrow_mut().a[iu],
                );
            } else {
                construct_bloom_filter(&p_w_info, ii, not_ready);
            }
            if malloc_failed() {
                return fail();
            }
        }
        // O endereço do OP_Explain só serve ao scanstatus, que o Debian não liga; a chamada
        // fica pelo efeito de emitir o OP_Explain do EXPLAIN QUERY PLAN.
        let _addr_explain = where_explain_one_scan(
            p_parse,
            &p_tab_list.borrow(),
            &p_w_info.borrow().a[iu],
            wctrl_flags as u16,
        );
        {
            let addr_body = vdbe_current_addr(&v.borrow());
            p_w_info.borrow_mut().a[iu].addr_body = addr_body;
        }
        not_ready = where_code_one_loop_start(p_parse, &v, &p_w_info, ii, not_ready);
        {
            let mut wi = p_w_info.borrow_mut();
            wi.i_continue = wi.a[iu].addr_cont;
        }
        ii += 1;
    }

    // Pronto.
    {
        let addr = vdbe_current_addr(&v.borrow());
        p_w_info.borrow_mut().i_end_where = addr;
    }
    Some(p_w_info.clone())
}


// ---- part_016.rs ----

// O C define aqui a macro OpcodeRewriteTrace(D,K,P) e, sob SQLITE_DEBUG, a função
// sqlite3WhereOpcodeRewriteTrace(). Sem SQLITE_DEBUG (caso do Debian 13) a macro é um
// no-op, então tanto a função quanto as chamadas somem da tradução.

/// Gera o fim do laço WHERE. Ver os comentários de `where_begin()` para mais informações.
///
/// Consome o `WhereInfo`: ao final ele é liberado por `where_info_free()`.
pub fn where_end(p_w_info: WhereInfoRef) {
    let p_parse: ParseRef = p_w_info.borrow().p_parse.clone();
    let v: VdbeRef = p_parse.borrow().p_vdbe.clone().unwrap();
    let p_tab_list: SrcListRef = p_w_info.borrow().p_tab_list.clone();
    let db = p_parse.borrow().db.upgrade().unwrap();
    let i_end: i32 = vdbe_current_addr(&v);
    let mut n_rj: i32 = 0;

    // Índice usado pelo nível `iu` quando o WhereLoop é WHERE_INDEXED (u.btree.pIndex).
    let btree_index = |p_loop: &WhereLoopRef| -> Option<IndexRef> {
        match &p_loop.borrow().u {
            WhereLoopUnion::Btree { p_index, .. } => p_index.clone(),
            _ => None,
        }
    };
    // Índice de cobertura do nível `iu` (u.pCoveringIdx).
    let covering_index = |iu: usize| -> Option<IndexRef> {
        match &p_w_info.borrow().a[iu].u {
            WhereLevelUnion::CoveringIdx(p_ix) => p_ix.clone(),
            _ => None,
        }
    };

    // Gera o código de terminação dos laços.
    let mut i: i32 = p_w_info.borrow().n_level as i32 - 1;
    while i >= 0 {
        let iu = i as usize;
        let has_rj = p_w_info.borrow().a[iu].p_rj.is_some();
        if has_rj {
            // Termina a sub-rotina que forma o interior do laço da tabela do RIGHT JOIN.
            let (addr_cont, reg_return, addr_subrtn) = {
                let wi = p_w_info.borrow();
                let p_rj = wi.a[iu].p_rj.as_ref().unwrap();
                (wi.a[iu].addr_cont, p_rj.reg_return, p_rj.addr_subrtn)
            };
            vdbe_resolve_label(&v, addr_cont);
            let end_subrtn = vdbe_current_addr(&v);
            {
                let mut wi = p_w_info.borrow_mut();
                wi.a[iu].addr_cont = 0;
                wi.a[iu].p_rj.as_mut().unwrap().end_subrtn = end_subrtn;
            }
            vdbe_add_op3(&v, OP_RETURN, reg_return, addr_subrtn, 1);
            n_rj += 1;
        }
        let p_loop: WhereLoopRef = p_w_info.borrow().a[iu].p_w_loop.clone().unwrap();
        let ws_flags: u32 = p_loop.borrow().ws_flags;
        let (
            level_op,
            level_p1,
            level_p2,
            level_p3,
            level_p5,
            addr_cont,
            reg_bignull,
            addr_bignull,
            i_idx_cur,
            addr_nxt,
            addr_brk,
            addr_skip,
            addr_like_rep,
            i_like_rep_cntr,
            i_left_join,
            i_from,
            i_tab_cur,
            addr_first,
        ) = {
            let wi = p_w_info.borrow();
            let l = &wi.a[iu];
            (
                l.op,
                l.p1,
                l.p2,
                l.p3,
                l.p5,
                l.addr_cont,
                l.reg_bignull,
                l.addr_bignull,
                l.i_idx_cur,
                l.addr_nxt,
                l.addr_brk,
                l.addr_skip,
                l.addr_like_rep,
                l.i_like_rep_cntr,
                l.i_left_join,
                l.i_from,
                l.i_tab_cur,
                l.addr_first,
            )
        };
        if level_op != OP_NOOP {
            // Otimização skip-ahead para DISTINCT (SQLITE_DISABLE_SKIPAHEAD_DISTINCT não está definido).
            let mut addr_seek: i32 = 0;
            let mut seek_n: i32 = 0;
            {
                let (e_distinct, n_level) = {
                    let wi = p_w_info.borrow();
                    (wi.e_distinct, wi.n_level)
                };
                if e_distinct as u32 == WHERE_DISTINCT_ORDERED
                    && i == n_level as i32 - 1 // Ticket [ef9318757b152e3] 2017-10-21
                    && (ws_flags & WHERE_INDEXED) != 0
                {
                    let n_distinct_col: u16 = match &p_loop.borrow().u {
                        WhereLoopUnion::Btree { n_distinct_col, .. } => *n_distinct_col,
                        _ => 0,
                    };
                    if let Some(p_idx) = btree_index(&p_loop) {
                        let ix = p_idx.borrow();
                        if ix.has_stat1
                            && n_distinct_col > 0
                            && ix.ai_row_log_est[n_distinct_col as usize] >= 36
                        {
                            seek_n = n_distinct_col as i32;
                        }
                    }
                }
            }
            if seek_n > 0 {
                let n = seek_n;
                let r1: i32 = p_parse.borrow().n_mem + 1;
                for j in 0..n {
                    vdbe_add_op3(&v, OP_COLUMN, i_idx_cur, j, r1 + j);
                }
                p_parse.borrow_mut().n_mem += n + 1;
                let op: u8 = if level_op == OP_PREV { OP_SEEKLT } else { OP_SEEKGT };
                addr_seek = vdbe_add_op4_int(&v, op, i_idx_cur, 0, r1, n);
                vdbe_add_op2(&v, OP_GOTO, 1, level_p2);
            }
            // O caso comum: avança para a próxima linha.
            if addr_cont != 0 {
                vdbe_resolve_label(&v, addr_cont);
            }
            vdbe_add_op3(&v, level_op, level_p1, level_p2, level_p3 as i32);
            vdbe_change_p5(&v, level_p5 as u16);
            if reg_bignull != 0 {
                vdbe_resolve_label(&v, addr_bignull);
                vdbe_add_op2(&v, OP_DECRJUMPZERO, reg_bignull, level_p2 - 1);
            }
            if addr_seek != 0 {
                vdbe_jump_here(&v, addr_seek);
            }
        } else if addr_cont != 0 {
            vdbe_resolve_label(&v, addr_cont);
        }
        if (ws_flags & WHERE_IN_ABLE) != 0 {
            let (n_in, a_in_loop): (i32, Vec<InLoop>) = match &p_w_info.borrow().a[iu].u {
                WhereLevelUnion::In { n_in, a_in_loop } => (*n_in, a_in_loop.clone()),
                _ => (0, Vec::new()),
            };
            if n_in > 0 {
                vdbe_resolve_label(&v, addr_nxt);
                let mut j = n_in;
                while j > 0 {
                    let p_in = &a_in_loop[(j - 1) as usize];
                    vdbe_jump_here(&v, p_in.addr_in_top + 1);
                    if p_in.e_end_loop_op != OP_NOOP {
                        if p_in.n_prefix != 0 {
                            let b_early_out: i32 = ((ws_flags & WHERE_VIRTUALTABLE) == 0
                                && (ws_flags & WHERE_IN_EARLYOUT) != 0)
                                as i32;
                            if i_left_join != 0 {
                                // Em consultas LEFT JOIN o cursor pIn->iCur pode não ter sido
                                // aberto ainda. Isso ocorre em cláusulas WHERE como
                                // "a = ? AND b IN (...)", em que o índice é (a, b). Se o lado
                                // direito de (a=?) for NULL, o "b IN (...)" pode nunca ter sido
                                // codificado, mas o corpo do laço roda para devolver a linha
                                // nula. Então, se o cursor ainda não está aberto, salta o
                                // OP_Next ou OP_Prev prestes a ser codificado.
                                let addr_now = vdbe_current_addr(&v);
                                vdbe_add_op2(&v, OP_IFNOTOPEN, p_in.i_cur, addr_now + 2 + b_early_out);
                            }
                            if b_early_out != 0 {
                                let addr_now = vdbe_current_addr(&v);
                                vdbe_add_op4_int(
                                    &v,
                                    OP_IFNOHOPE,
                                    i_idx_cur,
                                    addr_now + 2,
                                    p_in.i_base,
                                    p_in.n_prefix,
                                );
                                // Redireciona o OP_IsNull contra o operando esquerdo do IN para
                                // que salte além do OP_IfNoHope. Isso porque o OP_IsNull também
                                // pula o OP_Affinity exigido pelo OP_IfNoHope.
                                vdbe_jump_here(&v, p_in.addr_in_top + 1);
                            }
                        }
                        vdbe_add_op2(&v, p_in.e_end_loop_op, p_in.i_cur, p_in.addr_in_top);
                    }
                    vdbe_jump_here(&v, p_in.addr_in_top - 1);
                    j -= 1;
                }
            }
        }
        vdbe_resolve_label(&v, addr_brk);
        if has_rj {
            let reg_return = p_w_info.borrow().a[iu].p_rj.as_ref().unwrap().reg_return;
            vdbe_add_op3(&v, OP_RETURN, reg_return, 0, 1);
        }
        if addr_skip != 0 {
            vdbe_goto(&v, addr_skip);
            vdbe_jump_here(&v, addr_skip);
            vdbe_jump_here(&v, addr_skip - 2);
        }
        // SQLITE_LIKE_DOESNT_MATCH_BLOBS não está definido.
        if addr_like_rep != 0 {
            vdbe_add_op2(&v, OP_DECRJUMPZERO, (i_like_rep_cntr >> 1) as i32, addr_like_rep);
        }
        if i_left_join != 0 {
            let ws = ws_flags;
            let addr = vdbe_add_op1(&v, OP_IFPOS, i_left_join);
            if (ws & WHERE_IDX_ONLY) == 0 {
                let (via_coroutine, reg_result, p_src_tab) = {
                    let tl = p_tab_list.borrow();
                    let p_src = &tl.a[i_from as usize];
                    (p_src.fg.via_coroutine != 0, p_src.reg_result, p_src.p_tab.clone())
                };
                if via_coroutine {
                    let n = reg_result;
                    let m = p_src_tab.unwrap().borrow().n_col as i32;
                    vdbe_add_op3(&v, OP_NULL, 0, n, n + m - 1);
                }
                vdbe_add_op1(&v, OP_NULLROW, i_tab_cur);
            }
            if (ws & WHERE_INDEXED) != 0
                || ((ws & WHERE_MULTI_OR) != 0 && covering_index(iu).is_some())
            {
                if (ws & WHERE_MULTI_OR) != 0 {
                    let p_ix: IndexRef = covering_index(iu).unwrap();
                    let (p_schema, tnum) = {
                        let ix = p_ix.borrow();
                        (ix.p_schema.as_ref().and_then(|w| w.upgrade()), ix.tnum)
                    };
                    let i_db = schema_to_index(&db, p_schema);
                    vdbe_add_op3(&v, OP_REOPENIDX, i_idx_cur, tnum as i32, i_db);
                    vdbe_set_p4_key_info(&p_parse, &p_ix);
                }
                vdbe_add_op1(&v, OP_NULLROW, i_idx_cur);
            }
            if level_op == OP_RETURN {
                vdbe_add_op2(&v, OP_GOSUB, level_p1, addr_first);
            } else {
                vdbe_goto(&v, addr_first);
            }
            vdbe_jump_here(&v, addr);
        }
        i -= 1;
    }

    // Reescreve os opcodes que referenciam a tabela para referenciar o índice.
    let n_level: usize = p_w_info.borrow().n_level as usize;
    for i in 0..n_level {
        let (i_from, has_rj, addr_body, i_tab_cur, i_idx_cur) = {
            let wi = p_w_info.borrow();
            let l = &wi.a[i];
            (l.i_from, l.p_rj.is_some(), l.addr_body, l.i_tab_cur, l.i_idx_cur)
        };
        let (via_coroutine, reg_result, p_tab_opt) = {
            let tl = p_tab_list.borrow();
            let p_tab_item = &tl.a[i_from as usize];
            (p_tab_item.fg.via_coroutine != 0, p_tab_item.reg_result, p_tab_item.p_tab.clone())
        };
        let p_loop: WhereLoopRef = p_w_info.borrow().a[i].p_w_loop.clone().unwrap();

        // Faz o processamento de RIGHT JOIN. Gera código que devolve as linhas sem par
        // do operando direito do RIGHT JOIN, com todas as colunas do operando esquerdo
        // em NULL.
        if has_rj {
            where_right_join_loop(&p_w_info, i as i32);
            continue;
        }

        // Para uma co-rotina, troca todas as referências OP_Column à tabela da co-rotina
        // por OP_Copy do resultado contido num registrador. OP_Rowid vira OP_Null.
        if via_coroutine {
            translate_column_to_copy(&p_parse, addr_body, i_tab_cur, reg_result, 0);
            continue;
        }

        // Se esta varredura usa um índice, faz substituições no código VDBE para ler os
        // dados do índice em vez da tabela, quando possível. Em alguns casos isso evita
        // que a tabela chegue a ser lida, o que pode dar um ganho grande de desempenho.
        //
        // As chamadas ao gerador de código entre where_begin e where_end criaram código
        // que referencia a tabela diretamente. Este laço varre todo esse código atrás de
        // opcodes que referenciam a tabela e os converte em opcodes que referenciam o
        // índice.
        let ws_flags: u32 = p_loop.borrow().ws_flags;
        let mut p_idx: Option<IndexRef> = None;
        if (ws_flags & (WHERE_INDEXED | WHERE_IDX_ONLY)) != 0 {
            p_idx = btree_index(&p_loop);
        } else if (ws_flags & WHERE_MULTI_OR) != 0 {
            p_idx = covering_index(i);
        }
        let malloc_failed = db.borrow().malloc_failed != 0;
        if let (Some(p_idx), false) = (p_idx, malloc_failed) {
            let p_tab: TableRef = p_tab_opt.unwrap();
            let (e_one_pass, i_end_where) = {
                let wi = p_w_info.borrow();
                (wi.e_one_pass, wi.i_end_where)
            };
            let last: i32 = if e_one_pass == ONEPASS_OFF {
                i_end
            } else {
                let p_idx_table = p_idx.borrow().p_table.upgrade().unwrap();
                if !has_rowid(&p_idx_table.borrow()) {
                    i_end
                } else {
                    i_end_where
                }
            };
            if p_idx.borrow().b_has_expr {
                let mut p_parse_mut = p_parse.borrow_mut();
                let mut cur = p_parse_mut.p_idx_epr.as_mut();
                while let Some(p) = cur {
                    if p.i_idx_cur == i_idx_cur {
                        p.i_data_cur = -1;
                        p.i_idx_cur = -1;
                    }
                    cur = p.p_ie_next.as_mut();
                }
            }
            let k: i32 = addr_body + 1;
            // O laço é um do-while: o opcode em `k` é sempre visitado, e a varredura
            // termina quando o índice alcança `last`.
            let last_idx = last as usize;
            let mut idx = k as usize;
            loop {
                let (opcode, op_p1, op_p2) = {
                    let vb = v.borrow();
                    let p_op = &vb.a_op[idx];
                    (p_op.opcode, p_op.p1, p_op.p2)
                };
                if op_p1 != i_tab_cur {
                    // nada a fazer
                } else if opcode == OP_COLUMN {
                    let mut x: i32 = op_p2;
                    if !has_rowid(&p_tab.borrow()) {
                        let p_pk = primary_key_index(&p_tab);
                        x = p_pk.borrow().ai_column[x as usize] as i32;
                    } else {
                        x = storage_column_to_table(&p_tab.borrow(), x as i16) as i32;
                    }
                    x = table_column_to_index(&p_idx.borrow(), x);
                    if x >= 0 {
                        let mut vb = v.borrow_mut();
                        vb.a_op[idx].p2 = x;
                        vb.a_op[idx].p1 = i_idx_cur;
                    } else {
                        // Não foi possível traduzir a referência à tabela para uma
                        // referência ao índice. Verifica que isso é inofensivo, isto é,
                        // que a tabela referenciada realmente está aberta.
                        if (ws_flags & WHERE_IDX_ONLY) != 0 {
                            error_msg(&p_parse, b"internal query planner error");
                            p_parse.borrow_mut().rc = SQLITE_INTERNAL;
                        }
                    }
                } else if opcode == OP_ROWID {
                    let mut vb = v.borrow_mut();
                    vb.a_op[idx].p1 = i_idx_cur;
                    vb.a_op[idx].opcode = OP_IDXROWID;
                } else if opcode == OP_IFNULLROW {
                    v.borrow_mut().a_op[idx].p1 = i_idx_cur;
                }
                idx += 1;
                if idx >= last_idx {
                    break;
                }
            }
        }
    }

    // O ponto de "break" fica aqui, logo depois do fim do laço externo. Define-o.
    let i_break = p_w_info.borrow().i_break;
    vdbe_resolve_label(&v, i_break);

    // Limpeza final.
    let saved_n_query_loop = p_w_info.borrow().saved_n_query_loop;
    p_parse.borrow_mut().n_query_loop = saved_n_query_loop as LogEst;
    where_info_free(&db, p_w_info);
    {
        let mut pp = p_parse.borrow_mut();
        pp.within_rj_subrtn = pp.within_rj_subrtn.wrapping_sub(n_rj as u8);
    }
}


// ---- part_017.rs ----

// O trecho C `where_c.017.c` é vazio (só uma linha em branco depois do fim de
// `sqlite3WhereEnd()`), então não há itens a traduzir nesta parte.

