// Mesclado das partes traduzidas de expr_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// As declarações de encaminhamento do C (expr_code_between, expr_code_vector) não existem em
// Rust: as funções são definidas nas partes seguintes do módulo.

/// Retorna o caractere de afinidade de uma coluna única de uma tabela.
pub fn table_column_affinity(tab: &Table, i_col: i32) -> u8 {
    if i_col < 0 || i_col >= tab.n_col as i32 {
        return SQLITE_AFF_INTEGER;
    }
    tab.a_col[i_col as usize].affinity
}

/// Retorna a afinidade da expressão `p_expr`, se houver.
///
/// Se `p_expr` é uma coluna, uma referência a uma coluna via alias AS,
/// ou uma subconsulta que retorna uma coluna, a afinidade daquela coluna é
/// retornada. Caso contrário, 0x00 é retornado, indicando ausência de afinidade
/// para a expressão.
pub fn expr_affinity(mut p_expr: &Expr) -> u8 {
    let mut op = p_expr.op;
    loop {
        if op == TK_COLUMN || (op == TK_AGG_COLUMN && p_expr.y.p_tab.is_some()) {
            assert!(expr_use_y_tab(p_expr));
            assert!(p_expr.y.p_tab.is_some());
            return table_column_affinity(
                &p_expr.y.p_tab.as_ref().unwrap().borrow(),
                p_expr.i_column as i32,
            );
        }
        if op == TK_SELECT {
            assert!(expr_use_x_select(p_expr));
            assert!(p_expr.x.p_select.is_some());
            let sel = p_expr.x.p_select.as_ref().unwrap();
            assert!(sel.p_e_list.is_some());
            let e_list = sel.p_e_list.as_ref().unwrap();
            assert!(!e_list.a.is_empty());
            assert!(e_list.a[0].p_expr.is_some());
            return expr_affinity(e_list.a[0].p_expr.as_ref().unwrap());
        }
        #[cfg(not(feature = "SQLITE_OMIT_CAST"))]
        {
            if op == TK_CAST {
                assert!(!expr_has_property(p_expr, EP_INTVALUE));
                return affinity_type(p_expr.u.z_token.as_ref().unwrap(), 0);
            }
        }
        if op == TK_SELECT_COLUMN {
            assert!(p_expr.p_left.is_some());
            assert!(expr_use_x_select(p_expr.p_left.as_ref().unwrap()));
            assert!(p_expr.i_column < p_expr.i_table as i32);
            assert!(p_expr.i_column >= 0);
            let p_left = p_expr.p_left.as_ref().unwrap();
            assert!(p_left.x.p_select.is_some());
            let sel = p_left.x.p_select.as_ref().unwrap();
            assert!(sel.p_e_list.is_some());
            let e_list = sel.p_e_list.as_ref().unwrap();
            assert!(p_expr.i_column < e_list.n_expr as i32);
            return expr_affinity(e_list.a[p_expr.i_column as usize].p_expr.as_ref().unwrap());
        }
        if op == TK_VECTOR {
            assert!(expr_use_x_list(p_expr));
            return expr_affinity(p_expr.x.p_list.as_ref().unwrap().a[0].p_expr.as_ref().unwrap());
        }
        if expr_has_property(p_expr, EP_SKIP | EP_IFNULLROW) {
            assert!(
                p_expr.op == TK_COLLATE
                    || p_expr.op == TK_IF_NULL_ROW
                    || (p_expr.op == TK_REGISTER && p_expr.op2 == TK_IF_NULL_ROW)
            );
            p_expr = p_expr.p_left.as_ref().unwrap();
            op = p_expr.op;
            continue;
        }
        if op != TK_REGISTER || {
            op = p_expr.op2;
            op == TK_REGISTER
        } {
            break;
        }
    }
    p_expr.aff_expr
}

/// Faz uma estimativa de todos os tipos de dado possíveis do resultado que pode
/// ser retornado por uma expressão. Retorna uma máscara de bits indicando a resposta:
///
///     0x01         Numérico
///     0x02         Texto
///     0x04         Blob
///
/// Se a expressão deve retornar NULL, então 0x00 é retornado.
pub fn expr_data_type(mut p_expr: Option<&Expr>) -> u32 {
    while let Some(p) = p_expr {
        match p.op {
            TK_COLLATE | TK_IF_NULL_ROW | TK_UPLUS => {
                p_expr = p.p_left.as_ref().map(|b| b.as_ref());
            }
            TK_NULL => {
                p_expr = None;
            }
            TK_STRING => {
                return 0x02;
            }
            TK_BLOB => {
                return 0x04;
            }
            TK_CONCAT => {
                return 0x06;
            }
            TK_VARIABLE | TK_AGG_FUNCTION | TK_FUNCTION => {
                return 0x07;
            }
            TK_COLUMN | TK_AGG_COLUMN | TK_SELECT | TK_CAST | TK_SELECT_COLUMN | TK_VECTOR => {
                let aff = expr_affinity(p);
                if aff >= SQLITE_AFF_NUMERIC {
                    return 0x05;
                }
                if aff == SQLITE_AFF_TEXT {
                    return 0x06;
                }
                return 0x07;
            }
            TK_CASE => {
                let mut res = 0u32;
                let p_list = p.x.p_list.as_ref().unwrap();
                assert!(expr_use_x_list(p));
                assert!(!p_list.a.is_empty());
                for ii in (1..p_list.n_expr).step_by(2) {
                    res |= expr_data_type(p_list.a[ii as usize].p_expr.as_ref().map(|b| b.as_ref()));
                }
                if p_list.n_expr % 2 != 0 {
                    res |= expr_data_type(
                        p_list.a[(p_list.n_expr - 1) as usize]
                            .p_expr
                            .as_ref()
                            .map(|b| b.as_ref()),
                    );
                }
                return res;
            }
            _ => {
                return 0x01;
            }
        }
    }
    0x00
}

/// Define a sequência de colação para a expressão `p_expr` como sendo a sequência de colação
/// nomeada por `p_coll_name`. Retorna um ponteiro para um novo nó Expr que implementa o
/// operador COLLATE.
///
/// Se um erro de alocação de memória ocorrer, esse fato é registrado em `p_parse.db`
/// e o parâmetro `p_expr` é retornado inalterado.
pub fn expr_add_collate_token(
    p_parse: &Parse,
    mut p_expr: Box<Expr>,
    p_coll_name: &Token,
    dequote: i32,
) -> Box<Expr> {
    if p_coll_name.n > 0 {
        let db = p_parse.db.upgrade().unwrap();
        if let Some(mut p_new) = expr_alloc(&db.borrow(), TK_COLLATE, p_coll_name, dequote != 0) {
            p_new.p_left = Some(p_expr);
            p_new.flags |= EP_COLLATE | EP_SKIP;
            return p_new;
        }
    }
    p_expr
}

/// Define a sequência de colação para a expressão `p_expr` como sendo a sequência de colação
/// nomeada por `z_c`.
pub fn expr_add_collate_string(p_parse: &Parse, p_expr: Box<Expr>, z_c: &[u8]) -> Box<Expr> {
    let mut s = Token::default();
    assert!(!z_c.is_empty());
    token_init(&mut s, z_c);
    expr_add_collate_token(p_parse, p_expr, &s, 0)
}

/// Pula sobre qualquer operador TK_COLLATE.
pub fn expr_skip_collate(mut p_expr: Option<&Expr>) -> Option<&Expr> {
    while let Some(e) = p_expr {
        if expr_has_property(e, EP_SKIP) {
            assert!(e.op == TK_COLLATE);
            p_expr = e.p_left.as_ref().map(|b| b.as_ref());
        } else {
            return Some(e);
        }
    }
    None
}

/// Pula sobre qualquer operador TK_COLLATE e/ou qualquer função
/// unlikely(), likelihood() ou likely() na raiz de uma expressão.
pub fn expr_skip_collate_and_likely(mut p_expr: Option<&Expr>) -> Option<&Expr> {
    while let Some(e) = p_expr {
        if !expr_has_property(e, EP_SKIP | EP_UNLIKELY) {
            break;
        }
        if expr_has_property(e, EP_UNLIKELY) {
            assert!(expr_use_x_list(e));
            assert!(!e.x.p_list.as_ref().unwrap().a.is_empty());
            assert!(e.op == TK_FUNCTION);
            p_expr = e.x.p_list.as_ref().unwrap().a[0].p_expr.as_ref().map(|b| b.as_ref());
        } else if e.op == TK_COLLATE {
            p_expr = e.p_left.as_ref().map(|b| b.as_ref());
        } else {
            break;
        }
    }
    p_expr
}

/// Retorna a sequência de colação para a expressão `p_expr`. Se não há uma
/// sequência de colação definida, retorna None.
///
/// Veja também: `expr_nn_coll_seq()`
///
/// O `expr_nn_coll_seq()` funciona da mesma forma exceto que retorna a colação
/// padrão se `p_expr` não tem uma colação definida.
///
/// A sequência de colação pode ser determinada por um operador COLLATE ou pela
/// presença de uma coluna com uma sequência de colação definida. Operadores COLLATE
/// têm precedência. Os operandos esquerdos têm precedência sobre os direitos.
pub fn expr_coll_seq(p_parse: &mut Parse, p_expr: &Expr) -> Option<CollSeqRef> {
    let db = p_parse.db.upgrade().unwrap();
    // O RefCell do db não pode ficar emprestado durante get_coll_seq/check_coll_seq,
    // que podem alterá-lo: cada empréstimo é curto.
    let enc = db.borrow().enc;
    let mut p_coll: Option<CollSeqRef> = None;
    let mut p = Some(p_expr);

    while let Some(curr) = p {
        let mut op = curr.op;
        if op == TK_REGISTER {
            op = curr.op2;
        }
        if (op == TK_AGG_COLUMN && curr.y.p_tab.is_some())
            || op == TK_COLUMN
            || op == TK_TRIGGER
        {
            assert!(expr_use_y_tab(curr));
            assert!(curr.y.p_tab.is_some());
            if let Some(tab_ref) = &curr.y.p_tab {
                let tab = tab_ref.borrow();
                if curr.i_column >= 0 {
                    let j = curr.i_column as usize;
                    let z_coll = column_coll(&tab.a_col[j]);
                    p_coll = find_coll_seq(&db.borrow(), enc, z_coll, 0);
                }
            }
            break;
        }
        if op == TK_CAST || op == TK_UPLUS {
            p = curr.p_left.as_ref().map(|b| b.as_ref());
            continue;
        }
        if op == TK_VECTOR {
            assert!(expr_use_x_list(curr));
            p = curr
                .x
                .p_list
                .as_ref()
                .and_then(|l| l.a.get(0))
                .and_then(|item| item.p_expr.as_ref().map(|b| b.as_ref()));
            continue;
        }
        if op == TK_COLLATE {
            assert!(!expr_use_u_token(curr));
            if let Some(z_token) = &curr.u.z_token {
                p_coll = get_coll_seq(p_parse, enc, 0, z_token);
            }
            break;
        }
        if expr_has_property(curr, EP_COLLATE) {
            if let Some(p_left) = curr.p_left.as_ref() {
                if expr_has_property(p_left, EP_COLLATE) {
                    p = Some(p_left.as_ref());
                    continue;
                }
            }
            let mut p_next: Option<&Expr> = curr.p_right.as_ref().map(|b| b.as_ref());
            // A união Expr.x nunca é usada ao mesmo tempo que Expr.pRight
            assert!(
                !expr_use_x_list(curr)
                    || curr.x.p_list.is_none()
                    || curr.p_right.is_none()
            );
            if expr_use_x_list(curr) && curr.x.p_list.is_some() && !db.borrow().malloc_failed {
                if let Some(p_list) = &curr.x.p_list {
                    for i in 0..p_list.n_expr {
                        if let Some(item_expr) = &p_list.a[i as usize].p_expr {
                            if expr_has_property(item_expr.as_ref(), EP_COLLATE) {
                                p_next = Some(item_expr.as_ref());
                                break;
                            }
                        }
                    }
                }
            }
            p = p_next;
        } else {
            break;
        }
    }

    if check_coll_seq(p_parse, p_coll.as_ref()) {
        p_coll = None;
    }
    p_coll
}

/// Retorna a sequência de colação para a expressão `p_expr`. Se não há uma
/// sequência de colação definida, retorna um ponteiro para a sequência de
/// colação padrão.
///
/// Veja também: `expr_coll_seq()`
///
/// O `expr_coll_seq()` funciona do mesmo jeito exceto que retorna None se não há
/// uma colação definida.
pub fn expr_nn_coll_seq(p_parse: &mut Parse, p_expr: &Expr) -> CollSeqRef {
    let mut p = expr_coll_seq(p_parse, p_expr);
    if p.is_none() {
        let db = p_parse.db.upgrade().unwrap();
        p = db.borrow().p_dflt_coll.clone();
    }
    assert!(p.is_some());
    p.unwrap()
}

/// Retorna verdadeiro se as duas expressões têm sequências de colação equivalentes.
pub fn expr_coll_seq_match(p_parse: &mut Parse, p_e1: &Expr, p_e2: &Expr) -> bool {
    let p_coll1 = expr_nn_coll_seq(p_parse, p_e1);
    let p_coll2 = expr_nn_coll_seq(p_parse, p_e2);
    let coll1 = p_coll1.borrow();
    let coll2 = p_coll2.borrow();
    str_i_cmp(&coll1.z_name, &coll2.z_name) == 0
}

/// `p_expr` é um operando de um operador de comparação. `aff2` é a afinidade de
/// tipo do outro operando. Esta rotina retorna a afinidade de tipo que deve ser
/// usada para o operador de comparação.
pub fn compare_affinity(p_expr: &Expr, aff2: u8) -> u8 {
    let aff1 = expr_affinity(p_expr);
    if aff1 > SQLITE_AFF_NONE && aff2 > SQLITE_AFF_NONE {
        // Ambos os lados da comparação são colunas. Se um tem afinidade numérica,
        // use isso. Caso contrário, use nenhuma afinidade.
        if is_numeric_affinity(aff1) || is_numeric_affinity(aff2) {
            SQLITE_AFF_NUMERIC
        } else {
            SQLITE_AFF_BLOB
        }
    } else {
        // Um lado é uma coluna, o outro não. Use a afinidade da coluna.
        assert!(aff1 <= SQLITE_AFF_NONE || aff2 <= SQLITE_AFF_NONE);
        (if aff1 <= SQLITE_AFF_NONE { aff2 } else { aff1 }) | SQLITE_AFF_NONE
    }
}


// ---- part_001.rs ----

/// pExpr é um operador de comparação. Retorna a afinidade de tipo que deve ser aplicada
/// aos dois operandos antes de fazer a comparação.
fn comparison_affinity(p_expr: &Expr) -> u8 {
    debug_assert!(
        p_expr.op == TK_EQ
            || p_expr.op == TK_IN
            || p_expr.op == TK_LT
            || p_expr.op == TK_GT
            || p_expr.op == TK_GE
            || p_expr.op == TK_LE
            || p_expr.op == TK_NE
            || p_expr.op == TK_IS
            || p_expr.op == TK_ISNOT
    );
    debug_assert!(p_expr.p_left.is_some());

    let mut aff = expr_affinity(p_expr.p_left.as_ref().unwrap());
    if p_expr.p_right.is_some() {
        aff = compare_affinity(p_expr.p_right.as_ref().unwrap(), aff);
    } else if expr_use_x_select(p_expr) {
        aff = compare_affinity(
            p_expr.x.p_select.as_ref().unwrap().p_e_list.as_ref().unwrap().a[0]
                .p_expr
                .as_ref()
                .unwrap(),
            aff,
        );
    } else if aff == 0 {
        aff = SQLITE_AFF_BLOB;
    }
    aff
}

/// pExpr é uma expressão de comparação (ex: "=", "<", IN(...) etc).
/// idx_affinity é a afinidade de uma coluna indexada. Retorna true se o índice
/// com afinidade idx_affinity pode ser usado para implementar a comparação em pExpr.
pub fn index_affinity_ok(p_expr: &Expr, idx_affinity: u8) -> bool {
    let aff = comparison_affinity(p_expr);
    if aff < SQLITE_AFF_TEXT {
        return true;
    }
    if aff == SQLITE_AFF_TEXT {
        return idx_affinity == SQLITE_AFF_TEXT;
    }
    is_numeric_affinity(idx_affinity)
}

/// Retorna o valor P5 que deve ser usado para um opcode de comparação binária
/// (OP_Eq, OP_Ge etc) usado para comparar pExpr1 e pExpr2.
fn binary_compare_p5(p_expr1: &Expr, p_expr2: &Expr, jump_if_null: u8) -> u8 {
    let aff = expr_affinity(p_expr2) as u8;
    (compare_affinity(p_expr1, aff) as u8) | jump_if_null
}

/// Retorna um ponteiro à sequência de colação que deve ser usada por um
/// operador de comparação binária comparando pLeft e pRight.
///
/// Se a expressão do lado esquerdo tem um tipo de sequência de colação, este é usado.
/// Caso contrário, a sequência de colação para a expressão do lado direito é usada,
/// ou o padrão (BINARY) se nenhuma expressão tem um tipo de colação.
///
/// O argumento pRight (mas não pLeft) pode ser um ponteiro nulo. Neste caso,
/// não é considerado.
pub fn binary_compare_coll_seq(
    p_parse: &mut Parse,
    p_left: &Expr,
    p_right: Option<&Expr>,
) -> Option<CollSeqRef> {
    let mut p_coll;
    if expr_has_property(p_left, EP_COLLATE) {
        p_coll = expr_coll_seq(p_parse, p_left);
    } else if p_right.is_some() && expr_has_property(p_right.unwrap(), EP_COLLATE) {
        p_coll = expr_coll_seq(p_parse, p_right.unwrap());
    } else {
        p_coll = expr_coll_seq(p_parse, p_left);
        if p_coll.is_none() {
            // No C, pRight pode ser nulo aqui e sqlite3ExprCollSeq(NULL) devolve NULL.
            if let Some(r) = p_right {
                p_coll = expr_coll_seq(p_parse, r);
            }
        }
    }
    p_coll
}

/// A expressão p é um operador de comparação. Retorna uma sequência de colação
/// apropriada para o operador de comparação.
///
/// Esta é normalmente apenas uma embrulho em torno de sqlite3BinaryCompareCollSeq().
/// Porém, se a flag OP_Commuted estiver ligada, a ordem dos operandos é invertida
/// na chamada sqlite3BinaryCompareCollSeq() para que a sequência de colação correta
/// seja encontrada.
pub fn expr_compare_coll_seq(p_parse: &mut Parse, p: &Expr) -> Option<CollSeqRef> {
    if expr_has_property(p, EP_COMMUTED) {
        binary_compare_coll_seq(
            p_parse,
            p.p_right.as_ref().unwrap(),
            p.p_left.as_ref().map(|b| b.as_ref()),
        )
    } else {
        binary_compare_coll_seq(
            p_parse,
            p.p_left.as_ref().unwrap(),
            p.p_right.as_ref().map(|b| b.as_ref()),
        )
    }
}

/// Gera código para um operador de comparação.
fn code_compare(
    p_parse: &mut Parse,
    p_left: &Expr,
    p_right: &Expr,
    opcode: u8,
    in1: i32,
    in2: i32,
    dest: i32,
    jump_if_null: u8,
    is_commuted: bool,
) -> i32 {
    if p_parse.n_err != 0 {
        return 0;
    }

    let p4 = if is_commuted {
        binary_compare_coll_seq(p_parse, p_right, Some(p_left))
    } else {
        binary_compare_coll_seq(p_parse, p_left, Some(p_right))
    };

    let p5 = binary_compare_p5(p_left, p_right, jump_if_null);
    let v = p_parse.p_vdbe.clone().unwrap();
    let addr = vdbe_add_op4(&v, opcode as i32, in2, dest, in1, P4::CollSeq(p4), P4_COLLSEQ);
    vdbe_change_p5(&v, p5);
    addr
}

/// Retorna true se a expressão pExpr é um vetor, false caso contrário.
///
/// Um vetor é definido como qualquer expressão que resulta em duas ou mais
/// colunas de resultado. Cada nó TK_VECTOR é um vetor porque o analisador
/// não gerará um TK_VECTOR com menos de duas entradas. Mas um TK_SELECT pode
/// ser um vetor ou um escalar. É considerado um vetor se tiver duas ou mais
/// colunas de resultado.
pub fn expr_is_vector(p_expr: &Expr) -> bool {
    expr_vector_size(p_expr) > 1
}

/// Se a expressão passada como único argumento é do tipo TK_VECTOR,
/// retorna o número de expressões no vetor. Ou, se a expressão é uma subconsulta,
/// retorna o número de colunas na subconsulta. Para qualquer outro tipo de expressão,
/// retorna 1.
pub fn expr_vector_size(p_expr: &Expr) -> i32 {
    let mut op = p_expr.op;
    if op == TK_REGISTER {
        op = p_expr.op2;
    }
    if op == TK_VECTOR {
        debug_assert!(expr_use_x_list(p_expr));
        p_expr.x.p_list.as_ref().unwrap().n_expr
    } else if op == TK_SELECT {
        debug_assert!(expr_use_x_select(p_expr));
        p_expr
            .x
            .p_select
            .as_ref()
            .unwrap()
            .p_e_list
            .as_ref()
            .unwrap()
            .n_expr
    } else {
        1
    }
}

/// Retorna um ponteiro a uma subexpressão de pVector que é a i-ésima coluna
/// do vetor (numerada começando de 0). O chamador deve garantir que i está
/// no intervalo.
///
/// Se pVector é realmente um escalar (e "escalar" aqui inclui subconsultas
/// que retornam uma coluna única!), então retorna pVector sem modificação.
///
/// pVector retém propriedade do subexpressão retornado.
///
/// Se o vetor é uma (SELECT ...), então a expressão retornado é apenas
/// a expressão para o i-ésimo termo do conjunto de resultados, e pode
/// não estar pronto para avaliação porque o cursor da tabela ainda não
/// foi posicionado.
pub fn vector_field_subexpr(p_vector: &Expr, i: i32) -> &Expr {
    debug_assert!(i < expr_vector_size(p_vector) || p_vector.op == TK_ERROR);
    if expr_is_vector(p_vector) {
        debug_assert!(p_vector.op2 == 0 || p_vector.op == TK_REGISTER);
        if p_vector.op == TK_SELECT || p_vector.op2 == TK_SELECT {
            debug_assert!(expr_use_x_select(p_vector));
            p_vector.x.p_select.as_ref().unwrap().p_e_list.as_ref().unwrap().a[i as usize]
                .p_expr
                .as_ref()
                .unwrap()
        } else {
            debug_assert!(expr_use_x_list(p_vector));
            p_vector.x.p_list.as_ref().unwrap().a[i as usize].p_expr.as_ref().unwrap()
        }
    } else {
        p_vector
    }
}

/// Calcula e retorna um novo objeto Expr que, quando passado a
/// sqlite3ExprCode(), gerará todo o código necessário para calcular
/// a coluna iField-ésima da expressão de vetor pVector.
///
/// É aceitável que pVector seja um escalar (enquanto iField==0).
/// Neste caso, esta rotina funciona como sqlite3ExprDup().
///
/// O chamador é proprietário do objeto Expr retornado e é responsável
/// por garantir que o valor retornado seja eventualmente liberado.
///
/// O chamador retém propriedade de pVector. Se pVector é um TK_SELECT,
/// então o objeto retornado fará referência a pVector e portanto pVector
/// deve permanecer válido pela vida do objeto retornado. Se pVector é um
/// TK_VECTOR ou uma expressão escalar, então pode ser deletado assim que
/// esta rotina retorna.
///
/// Um truque para causar um pVector de TK_SELECT ser deletado junto com
/// o objeto Expr retornado é anexar o pVector ao campo pRight do objeto
/// Expr retornado de TK_SELECT_COLUMN.
pub fn expr_for_vector_field(
    p_parse: &mut Parse,
    p_vector: &mut Expr,
    i_field: i32,
    n_field: i32,
) -> Option<Box<Expr>> {
    if p_vector.op == TK_SELECT {
        debug_assert!(expr_use_x_select(p_vector));
        // O nó Expr de TK_SELECT_COLUMN:
        //
        // pLeft:           pVector contendo TK_SELECT. Não deletado.
        // pRight:          não usado. Mas deletado recursivamente.
        // iColumn:         Índice de uma coluna em pVector
        // iTable:          0 ou o número de colunas no LHS de uma atribuição
        // pLeft->iTable:   Primeiro num array de registro mantendo resultado, ou 0
        //                  se o resultado ainda não foi calculado.
        //
        // sqlite3ExprDelete() especificamente pula a deleção recursiva de
        // pLeft em nós TK_SELECT_COLUMN. Mas pRight é seguido, então pVector
        // pode ser anexado a pRight para fazer este nó tomar propriedade de
        // pVector. Tipicamente haverá múltiplos nós TK_SELECT_COLUMN com o
        // mesmo ponteiro pLeft ao pVector, mas apenas um deles será proprietário
        // do pVector.
        let p_ret_opt = p_expr(p_parse, TK_SELECT_COLUMN, None, None);
        if let Some(mut p_ret) = p_ret_opt {
            expr_set_property(&mut p_ret, EP_FULLSIZE);
            p_ret.i_table = n_field;
            p_ret.i_column = i_field; // ynVar é i32 com SQLITE_MAX_VARIABLE_NUMBER=250000
            // INTEGRADOR: no C, pRet->pLeft = pVector é um ponteiro NÃO dono compartilhado
            // entre vários nós TK_SELECT_COLUMN (o pVector é lido depois para pegar
            // pLeft->iTable e o pSelect). Isso exige que Expr.p_left deste caso seja um
            // Rc/Weak (ou um índice numa arena de Expr); com Option<Box<Expr>> não há
            // como modelar sem cópia. Aplicar a decisão do integrador aqui.
            Some(p_ret)
        } else {
            None
        }
    } else {
        let mut p_vector_ref: &Expr = p_vector;
        if p_vector.op == TK_VECTOR {
            debug_assert!(expr_use_x_list(p_vector));
            if in_rename_object(p_parse) {
                // Isto deve ser um UPDATE de vetor dentro de um trigger
                let slot = &mut p_vector.x.p_list.as_mut().unwrap().a[i_field as usize].p_expr;
                return slot.take();
            }
            p_vector_ref = p_vector.x.p_list.as_ref().unwrap().a[i_field as usize]
                .p_expr
                .as_ref()
                .unwrap();
        }
        let db = p_parse.db.upgrade().unwrap();
        expr_dup(&db.borrow(), p_vector_ref, 0)
    }
}

/// Se a expressão passada como único argumento é do tipo TK_SELECT, gera código
/// para avaliá-la. Retorna o registro em que o resultado é armazenado
/// (ou, se a subconsulta retorna mais de uma coluna, o primeiro num array de
/// registros em que o resultado é armazenado).
///
/// Se pExpr não é uma expressão TK_SELECT, retorna 0.
fn expr_code_subselect(p_parse: &mut Parse, p_expr: &Expr) -> i32 {
    let mut reg = 0;
    if p_expr.op == TK_SELECT {
        reg = code_subselect(p_parse, p_expr);
    }
    reg
}

/// O argumento pVector aponta para uma expressão de vetor, seja um TK_VECTOR
/// ou TK_SELECT que retorna mais de uma coluna. Esta função retorna o número
/// do registro contendo o valor do elemento iField do vetor.
///
/// Se pVector é uma expressão TK_SELECT, então o código para ela deve ter sido
/// já gerado usando a rotina exprCodeSubselect(). Neste caso o parâmetro
/// regSelect deve ser o primeiro num array de registros contendo os resultados
/// da subconsulta.
///
/// Se pVector é do tipo TK_VECTOR, então o código para o campo solicitado é
/// gerado. Neste caso (*pRegFree) pode ser configurado para o número de um
/// registro temporário a ser liberado pelo chamador antes de retornar.
///
/// Antes de retornar, o parâmetro de saída (*ppExpr) é configurado para apontar
/// ao objeto Expr correspondente ao elemento iElem do vetor.
fn expr_vector_register<'a>(
    p_parse: &mut Parse,
    p_vector: &'a Expr,
    i_field: i32,
    reg_select: i32,
    pp_expr: &mut Option<&'a Expr>,
    p_reg_free: &mut i32,
) -> i32 {
    let op = p_vector.op;
    debug_assert!(
        op == TK_VECTOR
            || op == TK_REGISTER
            || op == TK_SELECT
            || op == TK_ERROR
    );
    if op == TK_REGISTER {
        *pp_expr = Some(vector_field_subexpr(p_vector, i_field));
        return p_vector.i_table + i_field;
    }
    if op == TK_SELECT {
        debug_assert!(expr_use_x_select(p_vector));
        *pp_expr = Some(
            p_vector.x.p_select.as_ref().unwrap().p_e_list.as_ref().unwrap().a[i_field as usize]
                .p_expr
                .as_ref()
                .unwrap(),
        );
        return reg_select + i_field;
    }
    if op == TK_VECTOR {
        debug_assert!(expr_use_x_list(p_vector));
        let e: &'a Expr = p_vector.x.p_list.as_ref().unwrap().a[i_field as usize]
            .p_expr
            .as_ref()
            .unwrap();
        *pp_expr = Some(e);
        return expr_code_temp(p_parse, e, p_reg_free);
    }
    0
}

/// A expressão pExpr é uma comparação entre dois valores vetoriais. Calcula
/// o resultado da comparação (1, 0 ou NULL) e escreve esse resultado no
/// registro dest.
///
/// O chamador deve satisfazer as seguintes pré-condições:
///
///    se pExpr->op==TK_IS:      op==TK_EQ e p5==SQLITE_NULLEQ
///    se pExpr->op==TK_ISNOT:   op==TK_NE e p5==SQLITE_NULLEQ
///    caso contrário:                op==pExpr->op e p5==0
fn code_vector_compare(
    p_parse: &mut Parse,
    p_expr: &Expr,
    dest: i32,
    op: u8,
    p5: u8,
) {
    let v = p_parse.p_vdbe.clone().unwrap();
    let p_left = p_expr.p_left.as_ref().unwrap();
    let p_right = p_expr.p_right.as_ref().unwrap();
    let n_left = expr_vector_size(p_left);
    let reg_left;
    let reg_right;
    let mut opx = op;
    let mut addr_cmp = 0;
    let addr_done = vdbe_make_label(p_parse);
    let is_commuted = expr_has_property(p_expr, EP_COMMUTED);

    if p_parse.n_err != 0 {
        return;
    }
    if n_left != expr_vector_size(p_right) {
        error_msg(p_parse, "row value misused");
        return;
    }
    debug_assert!(
        p_expr.op == TK_EQ
            || p_expr.op == TK_NE
            || p_expr.op == TK_IS
            || p_expr.op == TK_ISNOT
            || p_expr.op == TK_LT
            || p_expr.op == TK_GT
            || p_expr.op == TK_LE
            || p_expr.op == TK_GE
    );
    debug_assert!(
        p_expr.op == op
            || (p_expr.op == TK_IS && op == TK_EQ)
            || (p_expr.op == TK_ISNOT && op == TK_NE)
    );
    debug_assert!(p5 == 0 || p_expr.op != op);
    debug_assert!(p5 == SQLITE_NULLEQ || p_expr.op == op);

    if op == TK_LE {
        opx = TK_LT;
    }
    if op == TK_GE {
        opx = TK_GT;
    }
    if op == TK_NE {
        opx = TK_EQ;
    }

    reg_left = expr_code_subselect(p_parse, p_left);
    reg_right = expr_code_subselect(p_parse, p_right);

    vdbe_add_op2(&v, OP_INTEGER as i32, 1, dest);
    let mut i = 0;
    loop {
        let mut reg_free1 = 0;
        let mut reg_free2 = 0;
        let mut p_l: Option<&Expr> = None;
        let mut p_r: Option<&Expr> = None;
        let r1;
        let r2;
        debug_assert!(i >= 0 && i < n_left);
        if addr_cmp != 0 {
            vdbe_jump_here(&v, addr_cmp);
        }
        r1 = expr_vector_register(p_parse, p_left, i, reg_left, &mut p_l, &mut reg_free1);
        r2 = expr_vector_register(p_parse, p_right, i, reg_right, &mut p_r, &mut reg_free2);
        addr_cmp = vdbe_current_addr(&v);
        code_compare(
            p_parse,
            p_l.unwrap(),
            p_r.unwrap(),
            opx,
            r1,
            r2,
            addr_done,
            p5,
            is_commuted,
        );
        release_temp_reg(p_parse, reg_free1);
        release_temp_reg(p_parse, reg_free2);
        if (opx == TK_LT || opx == TK_GT) && i < n_left - 1 {
            addr_cmp = vdbe_add_op0(&v, OP_ELSEEQ as i32);
        }
        if p5 == SQLITE_NULLEQ {
            vdbe_add_op2(&v, OP_INTEGER as i32, 0, dest);
        } else {
            vdbe_add_op3(&v, OP_ZEROORNULL as i32, r1, dest, r2);
        }
        if i == n_left - 1 {
            break;
        }
        if opx == TK_EQ {
            vdbe_add_op2(&v, OP_NOTNULL as i32, dest, addr_done);
        } else {
            debug_assert!(op == TK_LT || op == TK_GT || op == TK_LE || op == TK_GE);
            vdbe_add_op2(&v, OP_GOTO as i32, 0, addr_done);
            if i == n_left - 2 {
                opx = op;
            }
        }
        i += 1;
    }
    vdbe_jump_here(&v, addr_cmp);
    vdbe_resolve_label(&v, addr_done);
    if op == TK_NE {
        vdbe_add_op2(&v, OP_NOT as i32, dest, dest);
    }
}


// ---- part_002.rs ----

// SQLITE_MAX_EXPR_DEPTH vale 1000 no Debian 13 (maior que zero), então só o ramo com a
// imposição de altura existe aqui.

/// Verifica se a altura do argumento `n_height` é menor ou igual à profundidade máxima
/// de expressão permitida. Se não for, deixa uma mensagem de erro em `p_parse`.
pub fn expr_check_height(p_parse: &mut Parse, n_height: i32) -> i32 {
    let mut rc = SQLITE_OK;
    let mx_height = {
        let db = p_parse.db.upgrade().unwrap();
        let v = db.borrow().a_limit[SQLITE_LIMIT_EXPR_DEPTH as usize];
        v
    };
    if n_height > mx_height {
        error_msg(
            p_parse,
            &format!("Expression tree is too large (maximum depth {})", mx_height),
        );
        rc = SQLITE_ERROR;
    }
    rc
}

/// Determina a altura máxima de qualquer árvore de expressão referenciada pela
/// estrutura passada como primeiro argumento (as três funções `height_of_*`).
///
/// Se essa altura máxima for maior que o valor atual apontado por `pn_height`,
/// o segundo parâmetro, então define `pn_height` para esse valor.
fn height_of_expr(p: Option<&Expr>, pn_height: &mut i32) {
    if let Some(expr) = p {
        if expr.n_height > *pn_height {
            *pn_height = expr.n_height;
        }
    }
}

fn height_of_expr_list(p: Option<&ExprList>, pn_height: &mut i32) {
    if let Some(list) = p {
        for i in 0..list.n_expr {
            height_of_expr(list.a[i as usize].p_expr.as_deref(), pn_height);
        }
    }
}

fn height_of_select(p_select: Option<&Select>, pn_height: &mut i32) {
    let mut p = p_select;
    while let Some(select) = p {
        height_of_expr(select.p_where.as_deref(), pn_height);
        height_of_expr(select.p_having.as_deref(), pn_height);
        height_of_expr(select.p_limit.as_deref(), pn_height);
        height_of_expr_list(select.p_e_list.as_deref(), pn_height);
        height_of_expr_list(select.p_group_by.as_deref(), pn_height);
        height_of_expr_list(select.p_order_by.as_deref(), pn_height);
        p = select.p_prior.as_deref();
    }
}

/// Define a variável `Expr.n_height` na estrutura passada como argumento.
/// Uma expressão sem filhos, sem `Expr.p_list` e sem `Expr.p_select` tem altura 1.
/// Qualquer outra expressão tem altura igual à altura máxima de qualquer outra
/// `Expr` referenciada mais um.
///
/// Também propaga as flags EP_PROPAGATE de `Expr.x.p_list` para `Expr.flags`,
/// se apropriado.
fn expr_set_height(p: &mut Expr) {
    let mut n_height = match &p.p_left {
        Some(left) => left.n_height,
        None => 0,
    };
    if let Some(right) = &p.p_right {
        if right.n_height > n_height {
            n_height = right.n_height;
        }
    }
    if expr_use_x_select(p) {
        height_of_select(p.x.p_select.as_deref(), &mut n_height);
    } else if let Some(list) = &p.x.p_list {
        height_of_expr_list(Some(list), &mut n_height);
        p.flags |= EP_PROPAGATE & expr_list_flags(Some(list));
    }
    p.n_height = n_height + 1;
}

/// Define a variável `Expr.n_height` usando a função `expr_set_height()`. Se a
/// altura for maior que a profundidade máxima de expressão permitida, deixa um
/// erro em `p_parse`.
///
/// Também propaga todas as flags EP_PROPAGATE de `Expr.x.p_list` para `Expr.flags`.
pub fn expr_set_height_and_flags(p_parse: &mut Parse, p: &mut Expr) {
    if p_parse.n_err != 0 {
        return;
    }
    expr_set_height(p);
    expr_check_height(p_parse, p.n_height);
}

/// Retorna a altura máxima de qualquer árvore de expressão referenciada
/// pela instrução select passada como argumento.
pub fn select_expr_height(p: Option<&Select>) -> i32 {
    let mut n_height = 0;
    height_of_select(p, &mut n_height);
    n_height
}

/// Define o deslocamento de erro para um nó `Expr`, se possível.
pub fn expr_set_error_offset(p_expr: Option<&mut Expr>, i_ofst: i32) {
    let p_expr = match p_expr {
        Some(e) => e,
        None => return,
    };
    if expr_use_w_join(p_expr) {
        return;
    }
    p_expr.w.i_ofst = i_ofst;
}

/// Alocador central para nós `Expr`.
///
/// Constrói um novo nó de expressão e o devolve. No C, a memória do nó e a do
/// argumento `p_token` é uma única alocação; aqui o texto do token fica em
/// `u.z_token` (um `Vec<u8>` sem o byte zero final). A função chamadora é
/// responsável por garantir que o nó eventualmente seja liberado.
///
/// Se `dequote` for verdadeiro, o token (se existir) perde as aspas. Se for falso,
/// nenhuma remoção de aspas é feita. O parâmetro é ignorado se `p_token` for nulo ou
/// se o token não parecer estar entre aspas. Se as aspas forem do tipo "..." (aspas
/// duplas), a flag EP_DBLQUOTED é ligada no nó de expressão.
///
/// Caso especial (tag-20240227-a): se `op==TK_INTEGER` e `p_token` aponta para
/// uma string que pode ser traduzida para um inteiro de 32 bits, o token não é
/// guardado em `u.z_token`. Em vez disso, o valor inteiro é escrito em `u.i_value`
/// e a flag EP_INTVALUE é ligada. Nenhum armazenamento extra é alocado para o texto
/// do inteiro e a flag `dequote` é ignorada. Veja também tag-20240227-b.
pub fn expr_alloc(
    db: &sqlite3,
    op: i32,
    p_token: Option<&Token>,
    dequote: bool,
) -> Option<Box<Expr>> {
    let _ = db;
    let mut n_extra: u32 = 0;
    let mut i_value: i32 = 0;

    if let Some(token) = p_token {
        if op != TK_INTEGER
            || token.z.is_none()
            || !get_int32(token.z.as_ref().unwrap(), &mut i_value)
        {
            n_extra = token.n + 1; // tag-20240227-a
            debug_assert!(i_value >= 0);
        }
    }

    let mut p_new = Box::new(Expr::default());
    p_new.op = (op & 0xff) as u8;
    p_new.i_agg = -1;
    if let Some(token) = p_token {
        if n_extra == 0 {
            p_new.flags |= EP_INTVALUE | EP_LEAF | (if i_value != 0 { EP_ISTRUE } else { EP_ISFALSE });
            p_new.u.i_value = i_value;
        } else {
            let mut z_token: Vec<u8> = Vec::with_capacity(token.n as usize);
            debug_assert!(token.z.is_some() || token.n == 0);
            if token.n != 0 {
                if let Some(z) = &token.z {
                    z_token.extend_from_slice(&z[..token.n as usize]);
                }
            }
            let first = z_token.first().copied().unwrap_or(0);
            p_new.u.z_token = Some(z_token);
            if dequote && is_quote(first) {
                dequote_expr(&mut p_new);
            }
        }
    }
    p_new.n_height = 1;
    Some(p_new)
}

/// Aloca um novo nó de expressão a partir de um token que já foi desaspado
/// (no C, terminado em zero).
pub fn expr(db: &sqlite3, op: i32, z_token: Option<&[u8]>) -> Option<Box<Expr>> {
    let x = Token {
        z: z_token.map(|z| z.to_vec()),
        n: z_token.map(|z| strlen_30(z) as u32).unwrap_or(0),
    };
    expr_alloc(db, op, Some(&x), false)
}

/// Anexa as subárvores `p_left` e `p_right` ao nó `Expr` `p_root`.
///
/// Se `p_root` for nulo, isso significa que ocorreu um erro de alocação de memória.
/// Nesse caso, as subárvores `p_left` e `p_right` são destruídas (aqui, pelo drop).
pub fn expr_attach_subtrees(
    _db: &sqlite3,
    p_root: Option<&mut Expr>,
    p_left: Option<Box<Expr>>,
    p_right: Option<Box<Expr>>,
) {
    match p_root {
        None => {
            drop(p_left);
            drop(p_right);
        }
        Some(root) => {
            debug_assert!(expr_use_x_list(root));
            debug_assert!(root.x.p_select.is_none());
            if let Some(right) = p_right {
                root.flags |= EP_PROPAGATE & right.flags;
                root.n_height = right.n_height + 1;
                root.p_right = Some(right);
            } else {
                root.n_height = 1;
            }
            if let Some(left) = p_left {
                root.flags |= EP_PROPAGATE & left.flags;
                if left.n_height >= root.n_height {
                    root.n_height = left.n_height + 1;
                }
                root.p_left = Some(left);
            }
        }
    }
}

/// Aloca um nó `Expr` que une até duas subárvores.
///
/// Uma ou as duas subárvores podem ser nulas. Devolve o novo nó `Expr`.
pub fn p_expr(
    p_parse: &mut Parse,
    op: i32,
    p_left: Option<Box<Expr>>,
    p_right: Option<Box<Expr>>,
) -> Option<Box<Expr>> {
    let mut p = Box::new(Expr::default());
    p.op = (op & 0xff) as u8;
    p.i_agg = -1;
    {
        let db = p_parse.db.upgrade().unwrap();
        expr_attach_subtrees(&db.borrow(), Some(&mut p), p_left, p_right);
    }
    expr_check_height(p_parse, p.n_height);
    Some(p)
}

/// Adiciona `p_select` ao campo `Expr.x.p_select`. Ou, se `p_expr` for nulo (por
/// falha de alocação de memória), destrói o objeto `p_select`.
pub fn p_expr_add_select(
    p_parse: &mut Parse,
    p_expr: Option<&mut Expr>,
    p_select: Option<Box<Select>>,
) {
    if let Some(e) = p_expr {
        e.x.p_select = p_select;
        expr_set_property(e, EP_XISSELECT | EP_SUBQUERY);
        expr_set_height_and_flags(p_parse, e);
    } else {
        drop(p_select);
    }
}

/// A lista de expressões `p_e_list` é uma lista de valores de vetor. Esta função
/// converte o conteúdo de `p_e_list` numa instrução select VALUES(...) que
/// devolve 1 linha para cada elemento da lista. Por exemplo, a lista:
///
///   ( (1,2), (3,4) (5,6) )
///
/// é traduzida para o equivalente de:
///
///   VALUES(1,2), (3,4), (5,6)
///
/// Cada um dos valores de vetor em `p_e_list` deve conter exatamente `n_elem` termos.
/// Se um elemento da lista não é um vetor ou não contém `n_elem` termos,
/// uma mensagem de erro é deixada em `p_parse`.
///
/// Usado no processamento de expressões IN(...) com uma lista de vetores no lado
/// direito, por exemplo "... IN ((1,2), (3,4), (5,6))".
pub fn expr_list_to_values(
    p_parse: &mut Parse,
    n_elem: i32,
    mut p_e_list: Box<ExprList>,
) -> Option<Box<Select>> {
    let mut p_ret: Option<Box<Select>> = None;
    debug_assert!(n_elem > 1);
    for ii in 0..p_e_list.n_expr {
        let n_expr_elem = {
            let p_expr = p_e_list.a[ii as usize].p_expr.as_ref().unwrap();
            if p_expr.op == TK_VECTOR {
                debug_assert!(expr_use_x_list(p_expr));
                p_expr.x.p_list.as_ref().unwrap().n_expr
            } else {
                1
            }
        };
        if n_expr_elem != n_elem {
            error_msg(
                p_parse,
                &format!(
                    "IN(...) element has {} term{} - expected {}",
                    n_expr_elem,
                    if n_expr_elem > 1 { "s" } else { "" },
                    n_elem
                ),
            );
            break;
        }
        let p_expr = p_e_list.a[ii as usize].p_expr.as_mut().unwrap();
        debug_assert!(expr_use_x_list(p_expr));
        let p_list = p_expr.x.p_list.take();
        let p_sel = select_new(p_parse, p_list, None, None, None, None, None, SF_VALUES, None);
        if let Some(mut sel) = p_sel {
            if let Some(prev) = p_ret.take() {
                sel.op = TK_ALL;
                sel.p_prior = Some(prev);
            }
            p_ret = Some(sel);
        }
    }

    if let Some(ret) = &mut p_ret {
        if ret.p_prior.is_some() {
            ret.sel_flags |= SF_MULTIVALUE;
        }
    }
    // sqlite3ExprListDelete: a lista é liberada pelo drop.
    drop(p_e_list);
    p_ret
}

/// Une duas expressões usando um operador AND. Se qualquer uma das expressões for
/// nula, devolve apenas a outra.
///
/// Se um lado do AND é sabidamente falso, e nenhum dos lados faz parte de uma
/// cláusula ON, então em vez de devolver uma expressão AND devolve uma expressão
/// constante com valor falso.
pub fn expr_and(
    p_parse: &mut Parse,
    p_left: Option<Box<Expr>>,
    p_right: Option<Box<Expr>>,
) -> Option<Box<Expr>> {
    let db = p_parse.db.upgrade().unwrap();
    match (p_left, p_right) {
        (None, p_right) => p_right,
        (p_left, None) => p_left,
        (Some(left), Some(right)) => {
            let f = left.flags | right.flags;
            if (f & (EP_OUTERON | EP_INNERON | EP_ISFALSE)) == EP_ISFALSE
                && !in_rename_object(p_parse)
            {
                expr_deferred_delete(p_parse, left);
                expr_deferred_delete(p_parse, right);
                expr(&db.borrow(), TK_INTEGER, Some(b"0"))
            } else {
                p_expr(p_parse, TK_AND, Some(left), Some(right))
            }
        }
    }
}


// ---- part_003.rs ----

/// Constrói um novo nó de expressão para uma função com vários argumentos.
pub fn expr_function(
    p_parse: &mut Parse,
    p_list: Option<Box<ExprList>>,
    p_token: &Token,
    e_distinct: i32,
) -> Option<Box<Expr>> {
    let db = p_parse.db.upgrade().unwrap();
    let p_new = expr_alloc(&db.borrow(), TK_FUNCTION, Some(p_token), true);
    let mut p_new = match p_new {
        Some(p) => p,
        None => {
            // Evita vazamento de memória quando a alocação falha (o drop libera a lista).
            drop(p_list);
            return None;
        }
    };
    debug_assert!(!expr_has_property(&p_new, EP_INNERON | EP_OUTERON));
    // INTEGRADOR: no C é a diferença de ponteiros (pToken->z - pParse->zTail). Como o token
    // aqui é uma cópia, o deslocamento dentro do SQL precisa vir do analisador léxico.
    p_new.w.i_ofst = token_offset_from_tail(p_parse, p_token);
    let limit = db.borrow().a_limit[SQLITE_LIMIT_FUNCTION_ARG as usize];
    if let Some(list) = &p_list {
        if list.n_expr > limit && p_parse.nested == 0 {
            let name = &p_token.z.as_ref().unwrap()[..p_token.n as usize];
            error_msg(
                p_parse,
                &format!(
                    "too many arguments on function {}",
                    String::from_utf8_lossy(name)
                ),
            );
        }
    }
    p_new.x.p_list = p_list;
    expr_set_property(&mut p_new, EP_HASFUNC);
    debug_assert!(expr_use_x_list(&p_new));
    expr_set_height_and_flags(p_parse, &mut p_new);
    if e_distinct == SF_DISTINCT as i32 {
        expr_set_property(&mut p_new, EP_DISTINCT);
    }
    Some(p_new)
}

/// Relata um erro ao tentar usar uma cláusula ORDER BY dentro dos argumentos
/// de uma função que não é de agregação.
pub fn expr_order_by_aggregate_error(p_parse: &mut Parse, p: &Expr) {
    let name = p.u.z_token.as_deref().unwrap_or(&[]);
    error_msg(
        p_parse,
        &format!(
            "ORDER BY may not be used with non-aggregate {}()",
            String::from_utf8_lossy(name)
        ),
    );
}

/// Anexa uma cláusula ORDER BY a uma chamada de função.
///
///     functionname( arguments ORDER BY sortlist )
///     \_____________________/          \______/
///             pExpr                    pOrderBy
///
/// A cláusula ORDER BY é inserida num novo nó `Expr` do tipo TK_ORDER e
/// adicionada ao campo `Expr.p_left` do nó pai TK_FUNCTION.
pub fn expr_add_function_order_by(
    p_parse: &mut Parse,
    p_expr: Option<&mut Expr>,
    p_order_by: Option<Box<ExprList>>,
) {
    let db = p_parse.db.upgrade().unwrap();
    let p_order_by = match p_order_by {
        Some(o) => o,
        None => {
            debug_assert!(db.borrow().malloc_failed);
            return;
        }
    };
    let p_expr = match p_expr {
        Some(e) => e,
        None => {
            debug_assert!(db.borrow().malloc_failed);
            drop(p_order_by);
            return;
        }
    };
    debug_assert!(p_expr.op == TK_FUNCTION);
    debug_assert!(p_expr.p_left.is_none());
    debug_assert!(expr_use_x_list(p_expr));
    let no_args = match &p_expr.x.p_list {
        None => true,
        Some(l) => l.n_expr == 0,
    };
    if no_args {
        // Ignora ORDER BY em agregados sem argumentos
        parser_add_cleanup(p_parse, ParseCleanup::ExprList(p_order_by));
        return;
    }
    if is_window_func(p_expr) {
        expr_order_by_aggregate_error(p_parse, p_expr);
        drop(p_order_by);
        return;
    }

    let p_ob = expr_alloc(&db.borrow(), TK_ORDER, None, false);
    let mut p_ob = match p_ob {
        Some(o) => o,
        None => {
            drop(p_order_by);
            return;
        }
    };
    p_ob.x.p_list = Some(p_order_by);
    debug_assert!(expr_use_x_list(&p_ob));
    expr_set_property(&mut p_ob, EP_FULLSIZE);
    p_expr.p_left = Some(p_ob);
}

/// Verifica se uma função é utilizável de acordo com as regras de acesso atuais:
///
///    SQLITE_FUNC_DIRECT    -     Só utilizável em SQL de nível superior
///
///    SQLITE_FUNC_UNSAFE    -     Utilizável se TRUSTED_SCHEMA ou em SQL
///                                de nível superior
///
/// Se a função não é utilizável, cria um erro.
pub fn expr_function_usable(p_parse: &mut Parse, p_expr: &Expr, p_def: &FuncDef) {
    debug_assert!(!in_rename_object(p_parse));
    debug_assert!((p_def.func_flags & (SQLITE_FUNC_DIRECT | SQLITE_FUNC_UNSAFE)) != 0);
    if expr_has_property(p_expr, EP_FROMDDL) {
        let db_flags = p_parse.db.upgrade().unwrap().borrow().flags;
        if (p_def.func_flags & SQLITE_FUNC_DIRECT) != 0 || (db_flags & SQLITE_TRUSTEDSCHEMA) == 0 {
            // Funções proibidas em triggers e views se:
            //     (1) marcadas com SQLITE_DIRECTONLY
            //     (2) não marcadas com SQLITE_INNOCUOUS (o que significa que estão
            //         marcadas com SQLITE_FUNC_UNSAFE) e SQLITE_DBCONFIG_TRUSTED_SCHEMA
            //         está desligado (o esquema pode estar contaminado).
            let name = p_expr.u.z_token.as_deref().unwrap_or(&[]);
            error_msg(
                p_parse,
                &format!("unsafe use of {}()", String::from_utf8_lossy(name)),
            );
        }
    }
}

/// Atribui um número de variável a uma expressão que codifica um curinga
/// no SQL original.
///
/// Curingas formados por um único "?" recebem o próximo número de variável sequencial.
///
/// Curingas da forma "?nnn" recebem o número "nnn". Garantimos que "nnn" não é grande
/// demais, para evitar um ataque de negação de serviço quando o SQL vem de fonte externa.
///
/// Curingas da forma ":aaa", "@aaa" ou "$aaa" recebem o mesmo número da ocorrência
/// anterior do mesmo curinga. Ou, se é a primeira ocorrência, recebem o próximo número
/// de variável sequencial.
pub fn expr_assign_var_number(p_parse: &mut Parse, p_expr: Option<&mut Expr>, n: u32) {
    let db = p_parse.db.upgrade().unwrap();
    let p_expr = match p_expr {
        Some(e) => e,
        None => return,
    };
    debug_assert!(!expr_has_property(p_expr, EP_INTVALUE | EP_REDUCED | EP_TOKENONLY));
    let z: Vec<u8> = p_expr.u.z_token.clone().unwrap();
    debug_assert!(!z.is_empty());
    debug_assert!(z[0] != 0);
    debug_assert!(n == strlen_30(&z) as u32);
    let var_limit = db.borrow().a_limit[SQLITE_LIMIT_VARIABLE_NUMBER as usize];
    let x: i32;
    if z.get(1).copied().unwrap_or(0) == 0 {
        // Curinga da forma "?". Atribui o próximo número de variável
        debug_assert!(z[0] == b'?');
        p_parse.n_var += 1;
        x = p_parse.n_var;
    } else {
        let mut do_add = false;
        if z[0] == b'?' {
            // Curinga da forma "?nnn". Converte "nnn" em inteiro e o usa como número
            // da variável
            let mut i: i64 = 0;
            let b_ok: bool;
            if n == 2 {
                i = (z[1] as i64) - (b'0' as i64); // O caso comum ?N de um único dígito N
                b_ok = true;
            } else {
                b_ok = 0 == atoi64(&z[1..], &mut i, (n - 1) as i32, SQLITE_UTF8);
            }
            if !b_ok || i < 1 || i > var_limit as i64 {
                error_msg(
                    p_parse,
                    &format!("variable number must be between ?1 and ?{}", var_limit),
                );
                record_error_offset_of_expr(&db.borrow(), p_expr);
                return;
            }
            x = i as i32;
            if x > p_parse.n_var {
                p_parse.n_var = x;
                do_add = true;
            } else if vlist_num_to_name(&p_parse.p_v_list, x).is_none() {
                do_add = true;
            }
        } else {
            // Curingas como ":aaa", "$aaa" ou "@aaa". Reaproveita o mesmo número de
            // variável da ocorrência anterior do mesmo nome ou, se o nome nunca
            // apareceu, usa o próximo número sequencial.
            let mut xx = vlist_name_to_num(&p_parse.p_v_list, &z, n as i32);
            if xx == 0 {
                p_parse.n_var += 1;
                xx = p_parse.n_var;
                do_add = true;
            }
            x = xx;
        }
        if do_add {
            let old = std::mem::take(&mut p_parse.p_v_list);
            p_parse.p_v_list = vlist_add(&db.borrow(), old, &z, n as i32, x);
        }
    }
    p_expr.i_column = x;
    if x > var_limit {
        error_msg(p_parse, "too many SQL variables");
        record_error_offset_of_expr(&db.borrow(), p_expr);
    }
}

/// Apaga recursivamente uma árvore de expressão.
fn expr_delete_nn(db: &sqlite3, p: Box<Expr>) {
    let mut p = p;
    // exprDeleteRestart
    loop {
        debug_assert!(p.op != TK_FUNCTION || !expr_use_y_sub(&p));
        if !expr_has_property(&p, EP_TOKENONLY | EP_LEAF) {
            // A união Expr.x nunca é usada ao mesmo tempo que Expr.pRight
            if let Some(right) = p.p_right.take() {
                debug_assert!(!expr_has_property(&p, EP_WINFUNC));
                expr_delete_nn(db, right);
            } else if expr_use_x_select(&p) {
                debug_assert!(!expr_has_property(&p, EP_WINFUNC));
                select_delete(db, p.x.p_select.take());
            } else {
                expr_list_delete(db, p.x.p_list.take());
                if expr_has_property(&p, EP_WINFUNC) {
                    window_delete(db, p.y.p_win.take());
                }
            }
            // INTEGRADOR: no C, o pLeft de TK_SELECT_COLUMN não é dono (apontador
            // compartilhado). Aqui ele é largado junto com `p`, o que só é correto
            // se esse p_left for uma referência contada (veja expr_for_vector_field).
            if p.op != TK_SELECT_COLUMN && p.p_left.is_some() {
                let p_left = p.p_left.take().unwrap();
                if !expr_has_property(&p, EP_STATIC) && !expr_has_property(&p_left, EP_STATIC) {
                    // Evita recursão desnecessária em operadores unários
                    drop(p);
                    p = p_left;
                    continue;
                } else {
                    expr_delete_nn(db, p_left);
                }
            }
        }
        if !expr_has_property(&p, EP_STATIC) {
            drop(p);
        } else {
            // Nó estático: no C nunca é liberado.
            std::mem::forget(p);
        }
        return;
    }
}

pub fn expr_delete(db: &sqlite3, p: Option<Box<Expr>>) {
    if let Some(p) = p {
        expr_delete_nn(db, p);
    }
}

pub fn expr_delete_generic(db: &sqlite3, p: Option<Box<Expr>>) {
    if let Some(p) = p {
        expr_delete_nn(db, p);
    }
}

/// Limpa os dois elementos de um objeto `OnOrUsing`.
pub fn clear_on_or_using(db: &sqlite3, p: Option<&mut OnOrUsing>) {
    match p {
        None => {
            // Nada a limpar
        }
        Some(p) => {
            if let Some(on) = p.p_on.take() {
                expr_delete_nn(db, on);
            } else if let Some(using) = p.p_using.take() {
                id_list_delete(db, Some(using));
            }
        }
    }
}

/// Providencia para que `p_expr` seja apagada quando o `p_parse` for apagado.
/// É parecido com `expr_delete()`, exceto que a exclusão é adiada até que o
/// `p_parse` seja apagado.
///
/// A `p_expr` pode ser apagada imediatamente num erro de memória (OOM).
///
/// Devolve 0 se a exclusão foi adiada com sucesso. Devolve diferente de zero
/// se a exclusão aconteceu imediatamente por causa de um OOM.
pub fn expr_deferred_delete(p_parse: &mut Parse, p_expr: Box<Expr>) -> i32 {
    (0 == parser_add_cleanup(p_parse, ParseCleanup::Expr(p_expr))) as i32
}

/// Invoca `rename_expr_unmap()` e `expr_delete()` sobre a expressão.
pub fn expr_unmap_and_delete(p_parse: &mut Parse, p: Option<Box<Expr>>) {
    if let Some(p) = p {
        if in_rename_object(p_parse) {
            rename_expr_unmap(p_parse, &p);
        }
        let db = p_parse.db.upgrade().unwrap();
        expr_delete_nn(&db.borrow(), p);
    }
}

/// Devolve o número de bytes alocados para a estrutura de expressão passada como
/// primeiro argumento. É sempre um entre EXPR_FULLSIZE, EXPR_REDUCEDSIZE ou
/// EXPR_TOKENONLYSIZE.
fn expr_struct_size(p: &Expr) -> i32 {
    if expr_has_property(p, EP_TOKENONLY) {
        return EXPR_TOKENONLYSIZE;
    }
    if expr_has_property(p, EP_REDUCED) {
        return EXPR_REDUCEDSIZE;
    }
    EXPR_FULLSIZE
}

/// As rotinas `duped_expr_*_size()` devolvem, cada uma, o número de bytes necessários
/// para guardar uma cópia de uma expressão ou de uma árvore de expressões. Diferem em
/// quanto da árvore é medido.
///
///     duped_expr_struct_size()     Tamanho só da estrutura Expr
///     duped_expr_node_size()       Tamanho de Expr + espaço para o token
///     duped_expr_size()            Expr + token + componentes das subárvores
///
/// A função `duped_expr_struct_size()` devolve dois valores combinados com OR:
/// (1) o espaço necessário para uma cópia só da estrutura Expr e
/// (2) as flags EP_xxx que indicam qual deve ser o tamanho da estrutura.
/// O valor devolvido é sempre um entre:
///
///      EXPR_FULLSIZE
///      EXPR_REDUCEDSIZE   | EP_REDUCED
///      EXPR_TOKENONLYSIZE | EP_TOKENONLY
///
/// O tamanho da estrutura se obtém mascarando o valor devolvido com 0xfff. As flags se
/// obtêm mascarando com EP_REDUCED|EP_TOKENONLY.
///
/// Observe que com `flags==EXPRDUP_REDUCE` esta rotina trabalha com objetos Expr de
/// tamanho completo (não reduzidos), como foram construídos originalmente pelo analisador.
/// Durante a análise da expressão, informação extra é calculada e movida para partes
/// posteriores do objeto Expr, e ela pode ser cortada se a expressão for reduzida.
/// Note também que não funciona fazer uma cópia EXPRDUP_REDUCE de uma expressão já
/// reduzida: só é legal reduzir uma árvore de expressão intocada vinda do analisador.
fn duped_expr_struct_size(p: &Expr, flags: i32) -> i32 {
    debug_assert!(flags == EXPRDUP_REDUCE || flags == 0); // Só um valor de flag permitido
    debug_assert!(EXPR_FULLSIZE <= 0xfff);
    debug_assert!((0xfff & (EP_REDUCED | EP_TOKENONLY) as i32) == 0);
    let n_size: i32;
    if 0 == flags || expr_has_property(p, EP_FULLSIZE) {
        n_size = EXPR_FULLSIZE;
    } else {
        debug_assert!(!expr_has_property(p, EP_TOKENONLY | EP_REDUCED));
        debug_assert!(!expr_has_property(p, EP_OUTERON));
        // x.pList é uma união no C: o teste de ponteiro nulo cobre pList e pSelect.
        if p.p_left.is_some() || p.x.p_list.is_some() || p.x.p_select.is_some() {
            n_size = EXPR_REDUCEDSIZE | (EP_REDUCED as i32);
        } else {
            debug_assert!(p.p_right.is_none());
            n_size = EXPR_TOKENONLYSIZE | (EP_TOKENONLY as i32);
        }
    }
    n_size
}


// ---- part_007.rs ----

/// Callback de `walk_expr()` usado por `expr_is_constant_or_group_by()`.
fn expr_node_is_constant_or_group_by(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    // Verifica se p_expr é idêntico a algum termo do GROUP BY. Se sim,
    // considera-o constante.
    {
        let p_group_by = match &p_walker.u {
            WalkerU::GroupBy(g) => g,
            _ => unreachable!("walker sem GROUP BY"),
        };
        let p_parse = p_walker
            .p_parse
            .as_ref()
            .expect("walker sem Parse");
        for i in 0..p_group_by.n_expr as usize {
            let p = p_group_by.a[i].p_expr.as_ref().expect("termo GROUP BY sem expressão");
            if expr_compare(None, p_expr, p, -1) < 2 {
                let p_coll = expr_nn_coll_seq(&mut p_parse.borrow_mut(), p);
                if is_binary(Some(&p_coll.borrow())) != 0 {
                    return WRC_PRUNE;
                }
            }
        }
    }

    // Verifica se p_expr é uma subconsulta. Se sim, considera-a variável.
    if expr_use_x_select(p_expr) {
        p_walker.e_code = 0;
        return WRC_ABORT;
    }

    expr_node_is_constant(p_walker, p_expr)
}

/// Caminha pela árvore de expressão passada como primeiro argumento. Retorna
/// valor não zero se a expressão consiste inteiramente de constantes ou de
/// cópias de termos de `p_group_by` que ordenam com a colação BINARY.
///
/// Esta rotina é usada para determinar se um termo da cláusula HAVING pode
/// ser promovido para a cláusula WHERE. Para que tal promoção funcione, o
/// valor do termo do HAVING deve ser o mesmo para todos os membros de um
/// "grupo". O requisito de que o termo do GROUP BY seja BINARY assume que
/// nenhuma outra colação tem agrupamento mais fino que o binário. Em outras
/// palavras, (A=B COLLATE binary) implica A=B em qualquer outra colação. O
/// requisito de que o GROUP BY seja BINARY é mais estrito que o necessário.
/// Também funcionaria promover termos do HAVING que usam a mesma colação
/// alternativa do termo do GROUP BY, mas isso é bem mais difícil de checar,
/// colações alternativas são incomuns e isto é só uma otimização, então
/// seguimos o caminho fácil e exigimos que o GROUP BY use a colação BINARY.
///
/// O GROUP BY é emprestado ao walker durante a caminhada (o C guarda um
/// ponteiro) e devolvido a `p_group_by` no fim.
pub fn expr_is_constant_or_group_by(
    p_parse: &ParseRef,
    p: &ExprRef,
    p_group_by: &mut Option<Box<ExprList>>,
) -> i32 {
    let mut w = Walker {
        p_parse: Some(p_parse.clone()),
        x_expr_callback: Some(expr_node_is_constant_or_group_by),
        x_select_callback: None,
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 1,
        m_w_flags: 0,
        u: WalkerU::GroupBy(p_group_by.take().expect("GROUP BY ausente")),
    };
    walk_expr(&mut w, Some(p));
    if let WalkerU::GroupBy(g) = std::mem::replace(&mut w.u, WalkerU::None) {
        *p_group_by = Some(g);
    }
    w.e_code as i32
}

/// Caminha por uma árvore de expressão do campo DEFAULT de uma definição de
/// coluna num CREATE TABLE. Retorna valor não zero se a expressão é aceitável
/// como DEFAULT, isto é, se é constante ou uma chamada de função com
/// argumentos constantes. Retorna 0 se há variáveis.
///
/// `is_init` é verdadeiro ao analisar a partir de sqlite_schema e falso ao
/// processar um novo CREATE TABLE. Quando verdadeiro, parâmetros (como ? ou
/// $abc) na expressão viram NULL. Quando falso, parâmetros geram erro.
/// Parâmetros não deveriam ser permitidos num CREATE TABLE, mas algumas
/// versões antigas do SQLite permitiam, então é preciso suportá-los ao ler
/// sqlite_schema por compatibilidade com o passado.
///
/// Se `is_init` é verdadeiro, liga EP_FromDDL em todo nó TK_FUNCTION.
///
/// Aqui, uma string entre aspas duplas (ex: "abc") é considerada variável,
/// mas uma entre aspas simples (ex: 'abc') é constante.
pub fn expr_is_constant_or_function(p: &ExprRef, is_init: u8) -> i32 {
    debug_assert!(is_init == 0 || is_init == 1);
    expr_is_const(None, p, 4 + is_init as i32)
}

/// Se a expressão `p` codifica um inteiro constante que cabe em 32 bits,
/// retorna 1 e põe o valor em `*p_value`. Se a expressão não é um inteiro ou é
/// grande demais para um inteiro de 32 bits com sinal, retorna 0 e deixa
/// `*p_value` inalterado.
pub fn expr_is_integer(p: &Expr, p_value: &mut i32) -> i32 {
    let mut rc = 0;

    // Se a expressão é um literal inteiro que cabe em 32 bits com sinal, o
    // flag EP_IntValue já foi ligado.
    debug_assert!(
        p.op != TK_INTEGER
            || (p.flags & EP_INT_VALUE) != 0
            || get_int32(p.u.z_token.as_deref().unwrap_or(&[]), &mut 0) == 0
    );

    if (p.flags & EP_INT_VALUE) != 0 {
        *p_value = p.u.i_value;
        return 1;
    }
    match p.op {
        TK_UPLUS => {
            rc = expr_is_integer(p.p_left.as_deref().expect("TK_UPLUS sem operando"), p_value);
        }
        TK_UMINUS => {
            let mut v = 0;
            if expr_is_integer(p.p_left.as_deref().expect("TK_UMINUS sem operando"), &mut v) != 0 {
                debug_assert!((v as u32) != 0x8000_0000);
                *p_value = v.wrapping_neg();
                rc = 1;
            }
        }
        _ => {}
    }
    rc
}

/// Retorna FALSE se não há chance de a expressão ser NULL.
///
/// Se a expressão pode ser NULL ou é complexa demais para saber, retorna
/// TRUE.
///
/// Esta rotina é uma otimização, para pular opcodes OP_IsNull quando se sabe
/// que o valor não pode ser NULL. Um falso positivo (TRUE quando a expressão
/// nunca é NULL) custa um pouco de desempenho e é inofensivo. Um falso
/// negativo (FALSE quando o resultado pode ser NULL) provavelmente gera
/// resposta incorreta. Na dúvida, retorna TRUE.
pub fn expr_can_be_null(p: &Expr) -> i32 {
    let mut p = p;
    while p.op == TK_UPLUS || p.op == TK_UMINUS {
        p = p.p_left.as_deref().expect("operador unário sem operando");
    }
    let mut op = p.op;
    if op == TK_REGISTER {
        op = p.op2;
    }
    match op {
        TK_INTEGER | TK_STRING | TK_FLOAT | TK_BLOB => 0,
        TK_COLUMN => {
            debug_assert!(expr_use_y_tab(p));
            if expr_has_property(p, EP_CAN_BE_NULL) {
                return 1;
            }
            // Referência a coluna de índice sobre expressão
            let p_tab = match &p.y.p_tab {
                None => return 1,
                Some(t) => t.borrow(),
            };
            // `a_col` vazio equivale ao aCol nulo (possível após erro anterior)
            let r = p.i_column >= 0
                && !p_tab.a_col.is_empty()
                && p.i_column < p_tab.n_col as i32
                && p_tab.a_col[p.i_column as usize].not_null == 0;
            r as i32
        }
        _ => 1,
    }
}

/// Retorna TRUE se a expressão dada é uma constante que seria inalterada
/// pelo OP_Affinity com a afinidade dada no segundo argumento.
///
/// Esta rotina decide se a operação OP_Affinity pode ser omitida. Na dúvida,
/// retorna FALSE. Um falso negativo é inofensivo. Um falso positivo, porém,
/// pode gerar resposta errada.
pub fn expr_needs_no_affinity_change(p: &Expr, aff: u8) -> i32 {
    let mut unary_minus = false;
    if aff == SQLITE_AFF_BLOB {
        return 1;
    }
    let mut p = p;
    while p.op == TK_UPLUS || p.op == TK_UMINUS {
        if p.op == TK_UMINUS {
            unary_minus = true;
        }
        p = p.p_left.as_deref().expect("operador unário sem operando");
    }
    let mut op = p.op;
    if op == TK_REGISTER {
        op = p.op2;
    }
    match op {
        TK_INTEGER => (aff >= SQLITE_AFF_NUMERIC) as i32,
        TK_FLOAT => (aff >= SQLITE_AFF_NUMERIC) as i32,
        TK_STRING => (!unary_minus && aff == SQLITE_AFF_TEXT) as i32,
        TK_BLOB => (!unary_minus) as i32,
        TK_COLUMN => {
            debug_assert!(p.i_table >= 0); // p não pode fazer parte de um CHECK
            (aff >= SQLITE_AFF_NUMERIC && p.i_column < 0) as i32
        }
        _ => 0,
    }
}

/// Retorna TRUE se a string dada é um nome de coluna de rowid.
pub fn is_rowid(z: &[u8]) -> i32 {
    if str_i_cmp(z, b"_ROWID_") == 0 {
        return 1;
    }
    if str_i_cmp(z, b"ROWID") == 0 {
        return 1;
    }
    if str_i_cmp(z, b"OID") == 0 {
        return 1;
    }
    0
}

/// Retorna um buffer com um alias de rowid utilizável para a tabela `p_tab`.
/// Um alias é utilizável se não há coluna definida pelo usuário com o mesmo
/// nome.
pub fn rowid_alias(p_tab: &Table) -> Option<&'static [u8]> {
    let az_opt: [&'static [u8]; 3] = [b"_ROWID_", b"ROWID", b"OID"];
    debug_assert!(visible_rowid(p_tab));
    for opt in az_opt.iter() {
        let mut i_col = 0usize;
        while i_col < p_tab.n_col as usize {
            if stricmp(Some(opt), Some(&p_tab.a_col[i_col].z_cn_name)) == 0 {
                break;
            }
            i_col += 1;
        }
        if i_col == p_tab.n_col as usize {
            return Some(opt);
        }
    }
    None
}

/// `p_x` é o lado direito de um operador IN. Se `p_x` é um SELECT que pode ser
/// simplificado para um acesso direto a tabela, retorna o SELECT. Se `p_x` não
/// é um SELECT, ou se o SELECT precisa ser materializado numa tabela
/// transitória, retorna None.
fn is_candidate_for_in_opt(p_x: &Expr) -> Option<&Select> {
    if !expr_use_x_select(p_x) {
        return None; // Não é uma subconsulta
    }
    if expr_has_property(p_x, EP_VAR_SELECT) {
        return None; // Subconsulta correlacionada
    }
    let p = p_x.x.p_select.as_deref().expect("EP_xIsSelect sem SELECT");
    if p.p_prior.is_some() {
        return None; // Não é um SELECT composto
    }
    if (p.sel_flags & (SF_DISTINCT | SF_AGGREGATE)) != 0 {
        return None; // Sem DISTINCT e sem funções de agregação
    }
    debug_assert!(p.p_group_by.is_none()); // Sem cláusula GROUP BY
    if p.p_limit.is_some() {
        return None; // Sem cláusula LIMIT
    }
    if p.p_where.is_some() {
        return None; // Sem cláusula WHERE
    }
    let p_src = p.p_src.as_deref().expect("SELECT sem FROM");
    if p_src.n_src != 1 {
        return None; // Um único termo na cláusula FROM
    }
    if p_src.a[0].p_select.is_some() {
        return None; // FROM não é uma subconsulta nem uma view
    }
    let p_tab = p_src.a[0].p_tab.as_ref().expect("FROM sem tabela").borrow();
    debug_assert!(!is_view(&p_tab)); // FROM não é uma view
    if is_virtual(&p_tab) {
        return None; // FROM não é uma tabela virtual
    }
    let p_e_list = p.p_elist.as_deref().expect("SELECT sem lista de resultados");
    // Todos os resultados do SELECT devem ser colunas.
    for i in 0..p_e_list.n_expr as usize {
        let p_res = p_e_list.a[i].p_expr.as_deref().expect("resultado sem expressão");
        if p_res.op != TK_COLUMN {
            return None;
        }
        debug_assert!(p_res.i_table == p_src.a[0].i_cursor); // Não é correlacionada
    }
    Some(p)
}

/// Gera código que verifica a coluna mais à esquerda da tabela de índice
/// `i_cur` para ver se contém entradas NULL. Faz o registro `reg_has_null`
/// receber um valor não NULL se `i_cur` não contém NULLs, e receber NULL se
/// `i_cur` contém um ou mais valores NULL.
fn sqlite3_set_has_null_flag(v: &mut Vdbe, i_cur: i32, reg_has_null: i32) {
    vdbe_add_op2(v, OP_INTEGER as i32, 0, reg_has_null);
    let addr1 = vdbe_add_op1(v, OP_REWIND as i32, i_cur);
    vdbe_add_op3(v, OP_COLUMN as i32, i_cur, 0, reg_has_null);
    vdbe_change_p5(v, OPFLAG_TYPEOFARG as u16);
    vdbe_jump_here(v, addr1);
}

/// O argumento é um operador IN com uma lista (não uma subconsulta) no lado
/// direito. Retorna TRUE se essa lista é constante.
fn sqlite3_in_rhs_is_constant(p_parse: &ParseRef, p_in: &mut Expr) -> i32 {
    debug_assert!(!expr_has_property(p_in, EP_X_IS_SELECT));
    let p_lhs = p_in.p_left.take();
    let res = expr_is_constant(Some(p_parse), p_in);
    p_in.p_left = p_lhs;
    res
}

/// Esta função é usada pela implementação do operador IN (...). O parâmetro
/// `p_x` é a expressão do lado direito do IN, que pode ser uma lista de
/// expressões ou uma subconsulta.
///
/// O trabalho desta rotina é achar ou criar um objeto b-tree que sirva para
/// testar pertinência ao conjunto do lado direito ou para iterar por todos os
/// seus membros, pulando duplicatas.
///
/// Um cursor é aberto na b-tree do lado direito e `*pi_tab` recebe o índice
/// desse cursor.
///
/// O valor retornado indica o tipo de b-tree:
///
///   IN_INDEX_ROWID      - O cursor foi aberto numa tabela do banco.
///   IN_INDEX_INDEX_ASC  - O cursor foi aberto num índice ascendente.
///   IN_INDEX_INDEX_DESC - O cursor foi aberto num índice descendente.
///   IN_INDEX_EPH        - O cursor foi aberto numa tabela efêmera criada e
///                         populada especialmente.
///   IN_INDEX_NOOP       - Nenhum cursor foi alocado. O IN deve ser
///                         implementado como uma sequência de comparações.
///
/// Uma b-tree existente pode ser usada se `p_x` é uma subconsulta simples
/// como `SELECT <coluna1>, <coluna2>... FROM <tabela>`. Se o lado direito é
/// uma lista ou uma subconsulta mais complexa, pode ser preciso gerar uma
/// tabela efêmera e apontar `p_x.i_table` para ela. Nesse caso a criação e a
/// inicialização da tabela efêmera podem ficar numa sub-rotina, o flag
/// EP_Subrtn é ligado em `p_x` e os campos `p_x.y.sub` mostram onde a
/// sub-rotina foi codificada.
///
/// `in_flags` deve conter pelo menos um dos bits IN_INDEX_MEMBERSHIP ou
/// IN_INDEX_LOOP, mas não os dois. Com IN_INDEX_MEMBERSHIP, a tabela gerada
/// serve para teste rápido de pertinência. Com IN_INDEX_LOOP, o índice serve
/// para percorrer todos os valores do lado direito, e então a b-tree não pode
/// ter duplicatas: uma tabela efêmera é criada a menos que as colunas
/// selecionadas sejam garantidamente únicas (INTEGER PRIMARY KEY, UNIQUE ou
/// índice). Com IN_INDEX_MEMBERSHIP, uma tabela efêmera é usada a menos que
/// `<colunas>` seja uma única coluna INTEGER PRIMARY KEY ou exista um índice
/// com `<colunas>` como prefixo.
///
/// Se IN_INDEX_NOOP_OK e IN_INDEX_MEMBERSHIP estão ligados e o lado direito é
/// uma lista, a rotina pode decidir que criar uma b-tree efêmera é caro demais
/// e retornar IN_INDEX_NOOP. Aí o chamador deve implementar o IN com uma
/// sequência de comparações Eq ou Ne.
///
/// Quando a b-tree serve para pertinência, o chamador pode precisar saber se
/// o lado direito contém NULL. Se `pr_rhs_has_null` é Some e há chance de o
/// (...) conter NULL em tempo de execução, um registro é alocado e seu número
/// vai para `*pr_rhs_has_null`. Se não há chance, `*pr_rhs_has_null` fica
/// inalterado. O registro vale NULL se a b-tree contém um ou mais NULLs e um
/// valor não NULL se não contém nenhum.
///
/// Se `ai_map` é Some, deve ter um elemento por coluna retornada pelo SELECT
/// do lado direito. A i-ésima entrada recebe o deslocamento da coluna do
/// índice que casa com a i-ésima coluna do SELECT. Por exemplo, com
///
///   (?,?,?) IN (SELECT a, b, c FROM t1)
///   CREATE INDEX i1 ON t1(b, c, a);
///
/// `ai_map[]` fica {2, 0, 1}.
pub fn find_in_index(
    p_parse: &ParseRef,
    p_x: &mut Expr,
    in_flags: u32,
    pr_rhs_has_null: Option<&mut i32>,
    ai_map: Option<&mut [i32]>,
    pi_tab: &mut i32,
) -> i32 {
    let mut pr_rhs_has_null = pr_rhs_has_null;
    let mut ai_map = ai_map;
    let mut e_type = 0; // Tipo da tabela do lado direito. IN_INDEX_*
    let v = get_vdbe(p_parse); // Máquina virtual sendo codificada

    debug_assert!(p_x.op == TK_IN);
    let must_be_unique = (in_flags & IN_INDEX_LOOP) != 0; // RHS deve ser único
    let mut i_tab = {
        // Cursor da tabela do lado direito
        let mut parse = p_parse.borrow_mut();
        let t = parse.n_tab;
        parse.n_tab += 1;
        t
    };

    // Se o lado direito deste IN (...) é um SELECT e importa saber se o
    // resultado contém NULLs, verifica se NULL é realmente possível (pode não
    // ser, por exemplo por causa de NOT NULL no esquema). Se nenhum NULL é
    // possível, zera pr_rhs_has_null antes de continuar.
    if pr_rhs_has_null.is_some() && expr_use_x_select(p_x) {
        let p_e_list = p_x
            .x
            .p_select
            .as_deref()
            .and_then(|s| s.p_elist.as_deref())
            .expect("IN com SELECT sem lista de resultados");
        let mut i = 0usize;
        while i < p_e_list.n_expr as usize {
            if expr_can_be_null(p_e_list.a[i].p_expr.as_deref().expect("resultado sem expressão")) != 0 {
                break;
            }
            i += 1;
        }
        if i == p_e_list.n_expr as usize {
            pr_rhs_has_null = None;
        }
    }

    // Verifica se uma tabela ou índice existente serve para a consulta. Isto é
    // preferível a gerar uma nova tabela efêmera.
    let n_err = p_parse.borrow().n_err;
    if n_err == 0 {
        if let Some(p) = is_candidate_for_in_opt(p_x) {
            let p_e_list = p.p_elist.as_deref().expect("SELECT sem lista de resultados");
            let n_expr = p_e_list.n_expr;
            let p_src = p.p_src.as_deref().expect("SELECT sem FROM");
            let p_tab_ref = p_src.a[0].p_tab.clone().expect("FROM sem tabela");
            let p_tab = p_tab_ref.borrow(); // Tabela <tabela>.
            let v = v.as_ref().expect("get_vdbe() já foi chamado antes");

            // Codifica um OP_Transaction e um OP_TableLock para <tabela>.
            let i_db = {
                // Índice do banco de dados de p_tab
                let db = p_parse.borrow().db.upgrade().expect("conexão encerrada");
                let schema = p_tab.p_schema.as_ref().and_then(|w| w.upgrade());
                let schema_b = schema.as_ref().map(|s| s.borrow());
                schema_to_index(&db.borrow(), schema_b.as_deref())
            };
            debug_assert!(i_db >= 0 && i_db < SQLITE_MAX_DB);
            code_verify_schema(&mut p_parse.borrow_mut(), i_db);
            table_lock(&mut p_parse.borrow_mut(), i_db, p_tab.tnum, 0, Some(p_tab.z_name.clone()));

            if n_expr == 1
                && p_e_list.a[0].p_expr.as_deref().expect("resultado sem expressão").i_column < 0
            {
                // O caso "x IN (SELECT rowid FROM tabela)"
                let i_addr = vdbe_add_op0(&mut v.borrow_mut(), OP_ONCE as i32);

                open_table(&mut p_parse.borrow_mut(), i_tab, i_db, &p_tab, OP_OPENREAD as i32);
                e_type = IN_INDEX_ROWID;
                let mut z_msg = b"USING ROWID SEARCH ON TABLE ".to_vec();
                z_msg.extend_from_slice(&p_tab.z_name);
                z_msg.extend_from_slice(b" FOR IN-OPERATOR");
                vdbe_explain(&mut p_parse.borrow_mut(), 0, z_msg);
                vdbe_jump_here(&mut v.borrow_mut(), i_addr);
            } else {
                let mut affinity_ok = true;
                let p_left = p_x.p_left.as_deref().expect("IN sem lado esquerdo");

                // Verifica que a afinidade usada em cada comparação é a mesma
                // da coluna da tabela no lado direito do IN. Se não for, não é
                // possível usar nenhum índice da tabela do lado direito.
                let mut i = 0;
                while i < n_expr && affinity_ok {
                    let p_lhs = vector_field_subexpr(p_left, i);
                    let i_col = p_e_list.a[i as usize]
                        .p_expr
                        .as_deref()
                        .expect("resultado sem expressão")
                        .i_column;
                    let idxaff = table_column_affinity(&p_tab, i_col); // Tabela do RHS
                    let cmpaff = compare_affinity(p_lhs, idxaff);
                    match cmpaff {
                        SQLITE_AFF_BLOB => {}
                        SQLITE_AFF_TEXT => {
                            // compare_affinity() só retorna TEXT se um dos
                            // lados não tem afinidade e o outro é TEXT. Logo,
                            // cmpaff só é TEXT se idxaff é TEXT e o termo do
                            // lado esquerdo do IN não tem afinidade.
                            debug_assert!(idxaff == SQLITE_AFF_TEXT);
                        }
                        _ => {
                            affinity_ok = is_numeric_affinity(idxaff);
                        }
                    }
                    i += 1;
                }

                if affinity_ok {
                    // Procura um índice existente que sirva para este IN
                    let mut p_idx_cur = p_tab.p_index.clone();
                    while let Some(p_idx_ref) = p_idx_cur {
                        if e_type != 0 {
                            break;
                        }
                        let p_idx = p_idx_ref.borrow();
                        p_idx_cur = p_idx.p_next.clone();
                        if (p_idx.n_column as i32) < n_expr {
                            continue;
                        }
                        if p_idx.p_part_idx_where.is_some() {
                            continue;
                        }
                        // O máximo de n_column é BMS-2, não BMS-1, para que se
                        // possa calcular BITMASK(n_expr) sem overflow.
                        if p_idx.n_column as i32 >= BMS - 1 {
                            continue;
                        }
                        if must_be_unique
                            && (p_idx.n_key_col as i32 > n_expr
                                || (p_idx.n_column as i32 > n_expr && !is_unique_index(&p_idx)))
                        {
                            continue; // Este índice não é único sobre as colunas do IN
                        }

                        let mut col_used: Bitmask = 0; // Colunas do índice usadas até agora
                        let mut i = 0;
                        while i < n_expr {
                            let p_lhs = vector_field_subexpr(p_left, i);
                            let p_rhs = p_e_list.a[i as usize]
                                .p_expr
                                .as_deref()
                                .expect("resultado sem expressão");
                            let p_req = binary_compare_coll_seq(p_parse, p_lhs, Some(p_rhs));

                            let mut j = 0;
                            while j < n_expr {
                                if p_idx.ai_column[j as usize] as i32 != p_rhs.i_column {
                                    j += 1;
                                    continue;
                                }
                                debug_assert!(!p_idx.az_coll[j as usize].is_empty());
                                if let Some(req) = &p_req {
                                    if str_i_cmp(&req.borrow().z_name, &p_idx.az_coll[j as usize]) != 0 {
                                        j += 1;
                                        continue;
                                    }
                                }
                                break;
                            }
                            if j == n_expr {
                                break;
                            }
                            let m_col = maskbit(j as u32); // Máscara da coluna atual
                            if (m_col & col_used) != 0 {
                                break; // Cada coluna é usada uma só vez
                            }
                            col_used |= m_col;
                            if let Some(m) = ai_map.as_deref_mut() {
                                m[i as usize] = j;
                            }
                            i += 1;
                        }

                        debug_assert!(i == n_expr || col_used != (maskbit(n_expr as u32) - 1));
                        if col_used == (maskbit(n_expr as u32) - 1) {
                            // Se chegamos aqui, o índice p_idx é utilizável
                            let i_addr = vdbe_add_op0(&mut v.borrow_mut(), OP_ONCE as i32);
                            let mut z_msg = b"USING INDEX ".to_vec();
                            z_msg.extend_from_slice(&p_idx.z_name);
                            z_msg.extend_from_slice(b" FOR IN-OPERATOR");
                            vdbe_explain(&mut p_parse.borrow_mut(), 0, z_msg);
                            vdbe_add_op3(
                                &mut v.borrow_mut(),
                                OP_OPENREAD as i32,
                                i_tab,
                                p_idx.tnum as i32,
                                i_db,
                            );
                            vdbe_set_p4_key_info(&mut p_parse.borrow_mut(), &p_idx_ref);
                            debug_assert!(IN_INDEX_INDEX_DESC == IN_INDEX_INDEX_ASC + 1);
                            e_type = IN_INDEX_INDEX_ASC + p_idx.a_sort_order[0] as i32;

                            if let Some(r) = pr_rhs_has_null.as_deref_mut() {
                                let reg = {
                                    let mut parse = p_parse.borrow_mut();
                                    parse.n_mem += 1;
                                    parse.n_mem
                                };
                                *r = reg;
                                if n_expr == 1 {
                                    sqlite3_set_has_null_flag(&mut v.borrow_mut(), i_tab, reg);
                                }
                            }
                            vdbe_jump_here(&mut v.borrow_mut(), i_addr);
                        }
                    } // Fim do laço sobre os índices
                } // Fim de if affinity_ok
            } // Fim do caso que não é índice de rowid
        } // Fim da tentativa de otimizar com um índice
    }

    // Se não há índice pré-existente para a cláusula IN, e IN_INDEX_NOOP é
    // uma resposta permitida, e o lado direito do IN é uma lista e não uma
    // subconsulta, e o lado direito não é constante ou tem dois termos ou
    // menos, então não vale a pena criar uma tabela efêmera para avaliar o IN
    // e retorna IN_INDEX_NOOP.
    if e_type == 0
        && (in_flags & IN_INDEX_NOOP_OK) != 0
        && expr_use_x_list(p_x)
        && (sqlite3_in_rhs_is_constant(p_parse, p_x) == 0
            || p_x.x.p_list.as_deref().expect("IN sem lista").n_expr <= 2)
    {
        p_parse.borrow_mut().n_tab -= 1; // Desfaz a alocação do cursor não usado
        i_tab = -1; // Cursor não alocado
        e_type = IN_INDEX_NOOP;
    }

    if e_type == 0 {
        // Não achamos tabela nem índice existente para usar como b-tree do
        // lado direito. É preciso gerar uma tabela efêmera para o trabalho.
        let saved_n_query_loop = p_parse.borrow().n_query_loop;
        let mut r_may_have_null = 0;
        e_type = IN_INDEX_EPH;
        if (in_flags & IN_INDEX_LOOP) != 0 {
            p_parse.borrow_mut().n_query_loop = 0;
        } else if let Some(r) = pr_rhs_has_null.as_deref_mut() {
            let mut parse = p_parse.borrow_mut();
            parse.n_mem += 1;
            r_may_have_null = parse.n_mem;
            *r = r_may_have_null;
        }
        debug_assert!(p_x.op == TK_IN);
        code_rhs_of_in(&mut p_parse.borrow_mut(), p_x, i_tab);
        if r_may_have_null != 0 {
            let v = v.as_ref().expect("get_vdbe() já foi chamado antes");
            sqlite3_set_has_null_flag(&mut v.borrow_mut(), i_tab, r_may_have_null);
        }
        p_parse.borrow_mut().n_query_loop = saved_n_query_loop;
    }

    if let Some(m) = ai_map.as_deref_mut() {
        if e_type != IN_INDEX_INDEX_ASC && e_type != IN_INDEX_INDEX_DESC {
            let n = expr_vector_size(p_x.p_left.as_deref().expect("IN sem lado esquerdo"));
            for i in 0..n {
                m[i as usize] = i;
            }
        }
    }
    *pi_tab = i_tab;
    e_type
}


// ---- part_008.rs ----

// Notas desta parte (expr.c, parte 8):
//  - Convenção de empréstimo, igual à da parte 9: `Parse*` vira `&ParseRef`, `Vdbe*` vira `&VdbeRef`,
//    e nenhum `borrow()` é mantido durante a chamada a outra rotina.
//  - Parâmetro de expressão que o C aceita nulo vira `Option<&Expr>`; o que o C nunca recebe nulo é `&Expr`.
//  - Os `assert()`, `testcase`, `VdbeCoverage*`, `VdbeComment` e `ExprSetVVAProperty` não existem no
//    Debian (sem SQLITE_DEBUG, SQLITE_COVERAGE_TEST nem ENABLE_EXPLAIN_COMMENTS) e não são traduzidos.
//    `ExplainQueryPlan` existe (SQLITE_OMIT_EXPLAIN não está definido) e vira `vdbe_explain`.
//  - SQLITE_ENABLE_STMT_SCANSTATUS não está entre as opções do Debian: `sqlite3VdbeScanStatus*` some.
//  - SQLITE_OMIT_SUBQUERY não está definido: os trechos sob `#ifndef` valem sempre.
//  - Falha de alocação não existe em Rust: os ramos `if( zRet )` e afins ficam só com o caminho normal,
//    mas o teste de `db.malloc_failed` do C é mantido onde ele decide o fluxo.
//  - Os macros de `sqliteInt.h` (`ExprUseXSelect`, `ExprHasProperty`, ...) e `sqlite3SelectDestInit`
//    não pertencem a este trecho; vêm do módulo do cabeçalho e de select.c.

/// O argumento `p_expr` é uma expressão `(?, ?...) IN(...)`. Esta função aloca e devolve a cadeia
/// terminada em zero com as afinidades a usar em cada coluna da comparação.
pub fn expr_in_affinity(p_parse: &ParseRef, p_expr: &Expr) -> Vec<u8> {
    let _ = p_parse;
    let p_left: &Expr = p_expr.p_left.as_deref().expect("IN sem lado esquerdo");
    let n_val = expr_vector_size(p_left) as usize;
    let p_select: Option<&Select> = if expr_use_x_select(p_expr) {
        p_expr.x.p_select.as_deref()
    } else {
        None
    };
    let mut z_ret: Vec<u8> = vec![0u8; n_val + 1];
    for i in 0..n_val {
        let p_a: &Expr = vector_field_subexpr(p_left, i as i32);
        let a: u8 = expr_affinity(Some(p_a));
        if let Some(p_sel) = p_select {
            let p_e_list: &ExprList = p_sel.p_e_list.as_deref().expect("select sem lista de colunas");
            z_ret[i] = compare_affinity(p_e_list.a[i].p_expr.as_deref().expect("coluna sem expressão"), a);
        } else {
            z_ret[i] = a;
        }
    }
    z_ret[n_val] = 0;
    z_ret
}

/// Carrega o `Parse` com uma mensagem de erro da forma "sub-select returns N columns - expected M".
pub fn subselect_error(p_parse: &ParseRef, n_actual: i32, n_expect: i32) {
    if p_parse.borrow().n_err == 0 {
        error_msg(
            p_parse,
            b"sub-select returns %d columns - expected %d",
            &[Value::Int(n_actual as i64), Value::Int(n_expect as i64)],
        );
    }
}

/// A expressão `p_expr` é um vetor usado num contexto onde não é permitido. Se for um vetor de
/// sub-seleção, carrega o `Parse` com "sub-select returns N columns - expected 1". Se for um vetor
/// escalar comum, carrega com "row value misused".
pub fn vector_error_msg(p_parse: &ParseRef, p_expr: &Expr) {
    if expr_use_x_select(p_expr) {
        let p_select: &Select = p_expr.x.p_select.as_deref().expect("expr sem select");
        let n_expr = p_select.p_e_list.as_deref().expect("select sem lista de colunas").n_expr;
        subselect_error(p_parse, n_expr, 1);
    } else {
        error_msg(p_parse, b"row value misused", &[]);
    }
}

/// Gera código que constrói uma tabela efêmera com todos os termos do lado direito de um operador
/// IN. O IN pode ter duas formas:
///
///     x IN (4,5,11)              -- lista no lado direito
///     x IN (SELECT a FROM b)     -- subconsulta no lado direito
///
/// O parâmetro `p_expr` é o operador IN e `i_tab` é o número do cursor da tabela efêmera. Na
/// primeira vez que a tabela é calculada, o cursor também é guardado em `p_expr.i_table`, mas o
/// cursor usado pode ser outro, pois pode ter sido duplicado com `OP_OpenDup`.
///
/// Se o lado esquerdo for uma coluna, ou o SELECT devolver uma coluna, a afinidade dela é usada para
/// montar as chaves do índice. Se ambos forem colunas, vale a afinidade numérica quando uma delas é
/// NUMERIC ou INTEGER. Se nenhum dos dois for coluna, vale a afinidade numérica.
pub fn code_rhs_of_in(p_parse: &ParseRef, p_expr: &mut Expr, i_tab: i32) {
    let mut addr_once: i32 = 0; // Endereço do OP_Once no topo
    let addr: i32; // Endereço do OP_OpenEphemeral
    let mut p_key_info: Option<KeyInfoRef>;
    let n_val: i32;
    let v: VdbeRef = p_parse
        .borrow()
        .p_vdbe
        .clone()
        .expect("code_rhs_of_in: Parse sem Vdbe");

    // A avaliação do IN precisa ser repetida a cada vez que ele é encontrado se qualquer um dos
    // casos abaixo for verdadeiro:
    //
    //    *  O lado direito é uma subconsulta correlacionada
    //    *  O lado direito é uma lista de expressões com variáveis
    //    *  Estamos dentro de um trigger
    //
    // Se todos forem falsos, o lado direito é calculado uma só vez e reaproveitado várias vezes.
    if !expr_has_property(p_expr, EP_VARSELECT) && p_parse.borrow().i_self_tab == 0 {
        // O reaproveitamento do lado direito é permitido.
        // Se esta rotina já foi codificada, mas o código anterior pode não ter sido executado
        // ainda, invoca-o agora como sub-rotina.
        if expr_has_property(p_expr, EP_SUBRTN) {
            addr_once = vdbe_add_op0(&v, OP_ONCE as i32);
            if expr_use_x_select(p_expr) {
                let sel_id: u32 = p_expr.x.p_select.as_deref().expect("expr sem select").sel_id;
                vdbe_explain(p_parse, 0, b"REUSE LIST SUBQUERY %d", &[Value::Int(sel_id as i64)]);
            }
            vdbe_add_op2(&v, OP_GOSUB as i32, p_expr.y.sub.reg_return, p_expr.y.sub.i_addr);
            vdbe_add_op2(&v, OP_OPENDUP as i32, i_tab, p_expr.i_table);
            vdbe_jump_here(&v, addr_once);
            return;
        }

        // Começa a codificar a sub-rotina
        expr_set_property(p_expr, EP_SUBRTN);
        let reg_return: i32 = {
            let mut p = p_parse.borrow_mut();
            p.n_mem += 1;
            p.n_mem
        };
        p_expr.y.sub.reg_return = reg_return;
        p_expr.y.sub.i_addr = vdbe_add_op2(&v, OP_BEGINSUBRTN as i32, 0, reg_return) + 1;

        addr_once = vdbe_add_op0(&v, OP_ONCE as i32);
    }

    // Confere se este é um operador IN de vetor
    n_val = expr_vector_size(p_expr.p_left.as_deref().expect("IN sem lado esquerdo"));

    // Constrói a tabela efêmera que vai conter o lado direito do operador IN.
    p_expr.i_table = i_tab;
    addr = vdbe_add_op2(&v, OP_OPENEPHEMERAL as i32, p_expr.i_table, n_val);
    let db: DbRef = p_parse.borrow().db.upgrade().expect("Parse sem db");
    p_key_info = key_info_alloc(&db, n_val, 1);

    if expr_use_x_select(p_expr) {
        // Caso 1:     expr IN (SELECT ...)
        //
        // Gera código que escreve os resultados do select na tabela temporária alocada e aberta
        // acima.
        let sel_id: u32 = p_expr.x.p_select.as_deref().expect("expr sem select").sel_id;
        let n_expr: i32 = p_expr
            .x
            .p_select
            .as_deref()
            .expect("expr sem select")
            .p_e_list
            .as_deref()
            .expect("select sem lista de colunas")
            .n_expr;

        vdbe_explain(
            p_parse,
            1,
            b"%sLIST SUBQUERY %d",
            &[
                Value::Text(if addr_once != 0 { Vec::new() } else { b"CORRELATED ".to_vec() }),
                Value::Int(sel_id as i64),
            ],
        );
        // Se os lados esquerdo e direito do IN não casam, esse erro já foi pego muito antes de
        // chegar aqui.
        if n_expr == n_val {
            let mut dest = SelectDest::default();
            select_dest_init(&mut dest, SRT_SET, i_tab);
            dest.z_aff_sdst = Some(expr_in_affinity(p_parse, p_expr));
            let rc: i32;
            {
                let p_select: &mut Select = p_expr.x.p_select.as_deref_mut().expect("expr sem select");
                p_select.i_limit = 0;
                let mut p_copy: Box<Select> = select_dup(&db, p_select, 0);
                rc = if db.borrow().malloc_failed != 0 {
                    1
                } else {
                    select(p_parse, &mut p_copy, &mut dest)
                };
                // `p_copy` e `dest.z_aff_sdst` são liberados pelo Drop, como sqlite3SelectDelete
                // e sqlite3DbFree no C.
            }
            if rc != 0 {
                key_info_unref(p_key_info);
                return;
            }
            let p_left: &Expr = p_expr.p_left.as_deref().expect("IN sem lado esquerdo");
            let p_e_list: &ExprList = p_expr
                .x
                .p_select
                .as_deref()
                .expect("expr sem select")
                .p_e_list
                .as_deref()
                .expect("select sem lista de colunas");
            let k_info: &KeyInfoRef = p_key_info.as_ref().expect("key_info_alloc falhou");
            for i in 0..(n_val as usize) {
                let p: &Expr = vector_field_subexpr(p_left, i as i32);
                let p_coll = binary_compare_coll_seq(p_parse, p, p_e_list.a[i].p_expr.as_deref());
                k_info.borrow_mut().a_coll[i] = p_coll;
            }
        }
    } else if p_expr.x.p_list.is_some() {
        // Caso 2:     expr IN (exprlist)
        //
        // Para cada expressão, monta uma chave de índice a partir da avaliação e a guarda na tabela
        // temporária. Se <expr> é uma coluna, usa a afinidade dela ao montar as chaves. Se não é
        // uma coluna, usa a afinidade numérica.
        let mut affinity: u8 = expr_affinity(p_expr.p_left.as_deref());
        if affinity <= SQLITE_AFF_NONE {
            affinity = SQLITE_AFF_BLOB;
        } else if affinity == SQLITE_AFF_REAL {
            affinity = SQLITE_AFF_NUMERIC;
        }
        if let Some(k_info) = p_key_info.as_ref() {
            let p_coll = expr_coll_seq(p_parse, p_expr.p_left.as_deref());
            k_info.borrow_mut().a_coll[0] = p_coll;
        }

        // Percorre cada expressão de <exprlist>.
        let r1: i32 = get_temp_reg(p_parse);
        let r2: i32 = get_temp_reg(p_parse);
        let n_list: usize = p_expr.x.p_list.as_deref().expect("IN sem lista").a.len();
        for ii in 0..n_list {
            // Se a expressão não é constante, é preciso desligar o teste gerado acima que garante
            // que este código execute uma vez só. Para uma expressão não constante, este código
            // precisa rodar de novo a cada vez.
            let is_const: bool = {
                let p_e2: &Expr = p_expr.x.p_list.as_deref().expect("IN sem lista").a[ii]
                    .p_expr
                    .as_deref()
                    .expect("item sem expressão");
                addr_once == 0 || expr_is_constant(p_parse, p_e2) != 0
            };
            if !is_const {
                vdbe_change_to_noop(&v, addr_once - 1);
                vdbe_change_to_noop(&v, addr_once);
                expr_clear_property(p_expr, EP_SUBRTN);
                addr_once = 0;
            }

            // Avalia a expressão e a insere na tabela temporária
            let p_e2: &Expr = p_expr.x.p_list.as_deref().expect("IN sem lista").a[ii]
                .p_expr
                .as_deref()
                .expect("item sem expressão");
            expr_code(p_parse, Some(p_e2), r1);
            vdbe_add_op4(&v, OP_MAKERECORD as i32, r1, 1, r2, P4Value::Static(vec![affinity]), 1);
            vdbe_add_op4_int(&v, OP_IDXINSERT as i32, i_tab, r2, r1, 1);
        }
        release_temp_reg(p_parse, r1);
        release_temp_reg(p_parse, r2);
    }
    if let Some(k_info) = p_key_info.take() {
        vdbe_change_p4(&v, addr, P4Value::KeyInfo(k_info), P4_KEYINFO);
    }
    if addr_once != 0 {
        vdbe_add_op1(&v, OP_NULLROW as i32, i_tab);
        vdbe_jump_here(&v, addr_once);
        // Retorno da sub-rotina
        vdbe_add_op3(&v, OP_RETURN as i32, p_expr.y.sub.reg_return, p_expr.y.sub.i_addr, 1);
        clear_temp_reg_cache(p_parse);
    }
}

/// Gera código para subconsultas escalares usadas como expressão de subconsulta ou operador EXISTS:
///
///     (SELECT a FROM b)          -- subconsulta
///     EXISTS (SELECT a FROM b)   -- subconsulta EXISTS
///
/// O parâmetro `p_expr` é o SELECT ou EXISTS a codificar. Devolve o registro que guarda o resultado.
/// Num SELECT de várias colunas, o resultado fica numa sequência contígua de registros e o valor
/// devolvido é o registro da coluna mais à esquerda. Devolve 0 se ocorrer um erro.
pub fn code_subselect(p_parse: &ParseRef, p_expr: &mut Expr) -> i32 {
    let mut addr_once: i32 = 0; // Endereço do OP_Once no topo da sub-rotina
    let r_reg: i32; // Registro com o resultado
    let mut dest = SelectDest::default(); // O que fazer com o resultado do SELECT
    let n_reg: i32; // Registros a alocar

    let v: VdbeRef = p_parse
        .borrow()
        .p_vdbe
        .clone()
        .expect("code_subselect: Parse sem Vdbe");
    if p_parse.borrow().n_err != 0 {
        return 0;
    }

    // Se esta rotina já foi codificada, invoca-a como sub-rotina.
    if expr_has_property(p_expr, EP_SUBRTN) {
        let sel_id: u32 = p_expr.x.p_select.as_deref().expect("expr sem select").sel_id;
        vdbe_explain(p_parse, 0, b"REUSE SUBQUERY %d", &[Value::Int(sel_id as i64)]);
        vdbe_add_op2(&v, OP_GOSUB as i32, p_expr.y.sub.reg_return, p_expr.y.sub.i_addr);
        return p_expr.i_table;
    }

    // Começa a codificar a sub-rotina
    expr_set_property(p_expr, EP_SUBRTN);
    let reg_return: i32 = {
        let mut p = p_parse.borrow_mut();
        p.n_mem += 1;
        p.n_mem
    };
    p_expr.y.sub.reg_return = reg_return;
    p_expr.y.sub.i_addr = vdbe_add_op2(&v, OP_BEGINSUBRTN as i32, 0, reg_return) + 1;

    // A avaliação do EXISTS/SELECT precisa ser repetida a cada vez que ele é encontrado se qualquer
    // um dos casos abaixo for verdadeiro:
    //
    //    *  O lado direito é uma subconsulta correlacionada
    //    *  O lado direito é uma lista de expressões com variáveis
    //    *  Estamos dentro de um trigger
    //
    // Se todos forem falsos, o código roda uma vez só, o resultado é guardado e reaproveitado nas
    // invocações seguintes.
    if !expr_has_property(p_expr, EP_VARSELECT) {
        addr_once = vdbe_add_op0(&v, OP_ONCE as i32);
    }

    // Num SELECT, gera código que põe os valores de todas as colunas da primeira linha numa sequência
    // de registros e devolve o índice do primeiro.
    //
    // Num EXISTS, escreve o inteiro 0 (não existe) ou 1 (existe) num registro e devolve esse
    // registro.
    //
    // Nos dois casos a consulta ganha "LIMIT 1". Qualquer limite pré-existente é descartado em favor
    // do novo LIMIT 1.
    let mut p_sel: Box<Select> = p_expr.x.p_select.take().expect("expr sem select");
    vdbe_explain(
        p_parse,
        1,
        b"%sSCALAR SUBQUERY %d",
        &[
            Value::Text(if addr_once != 0 { Vec::new() } else { b"CORRELATED ".to_vec() }),
            Value::Int(p_sel.sel_id as i64),
        ],
    );
    n_reg = if p_expr.op == TK_SELECT {
        p_sel.p_e_list.as_deref().expect("select sem lista de colunas").n_expr
    } else {
        1
    };
    let n_mem_next: i32 = p_parse.borrow().n_mem + 1;
    select_dest_init(&mut dest, 0, n_mem_next);
    p_parse.borrow_mut().n_mem += n_reg;
    if p_expr.op == TK_SELECT {
        dest.e_dest = SRT_MEM;
        dest.i_sdst = dest.i_sd_parm;
        dest.n_sdst = n_reg;
        vdbe_add_op3(&v, OP_NULL as i32, 0, dest.i_sd_parm, dest.i_sd_parm + n_reg - 1);
    } else {
        dest.e_dest = SRT_EXISTS;
        vdbe_add_op2(&v, OP_INTEGER as i32, 0, dest.i_sd_parm);
    }
    let db: DbRef = p_parse.borrow().db.upgrade().expect("Parse sem db");
    if p_sel.p_limit.is_some() {
        // A subconsulta já tem um limite. Se o limite pré-existente é X, o novo limite passa a ser
        // X<>0, de modo que o novo limite seja 1 ou 0.
        let mut p_limit: Option<Box<Expr>> = expr(&db, TK_INTEGER, Some(b"0".as_slice()));
        if let Some(l) = p_limit.as_mut() {
            l.aff_expr = SQLITE_AFF_NUMERIC;
        }
        if p_limit.is_some() {
            let p_dup: Option<Box<Expr>> = expr_dup(
                &db,
                p_sel.p_limit.as_deref().expect("limite sumiu").p_left.as_deref(),
                0,
            );
            p_limit = super::p_expr(p_parse, TK_NE, p_dup, p_limit);
        }
        let p_old: Option<Box<Expr>> = p_sel.p_limit.as_deref_mut().expect("limite sumiu").p_left.take();
        expr_deferred_delete(p_parse, p_old);
        p_sel.p_limit.as_deref_mut().expect("limite sumiu").p_left = p_limit;
    } else {
        // Se não há limite pré-existente, acrescenta um limite de 1
        let p_limit: Option<Box<Expr>> = expr(&db, TK_INTEGER, Some(b"1".as_slice()));
        p_sel.p_limit = super::p_expr(p_parse, TK_LIMIT, p_limit, None);
    }
    p_sel.i_limit = 0;
    if select(p_parse, &mut p_sel, &mut dest) != 0 {
        p_expr.x.p_select = Some(p_sel);
        p_expr.op2 = p_expr.op;
        p_expr.op = TK_ERROR;
        return 0;
    }
    p_expr.x.p_select = Some(p_sel);
    r_reg = dest.i_sd_parm;
    p_expr.i_table = r_reg;
    if addr_once != 0 {
        vdbe_jump_here(&v, addr_once);
    }

    // Retorno da sub-rotina
    vdbe_add_op3(&v, OP_RETURN as i32, p_expr.y.sub.reg_return, p_expr.y.sub.i_addr, 1);
    clear_temp_reg_cache(p_parse);
    r_reg
}


// ---- part_009.rs ----

// Notas desta parte (expr.c, parte 9):
//  - Convenção de empréstimo: as rotinas que recebem `Vdbe*` recebem `&VdbeRef` e as que recebem
//    `Parse*` recebem `&ParseRef`. Nenhum `borrow()` é mantido durante a chamada a outra rotina.
//  - Os `assert()` do C, `VdbeCoverage*`, `VdbeComment` e `VdbeNoopComment` não existem no Debian
//    (sem SQLITE_DEBUG, SQLITE_COVERAGE_TEST nem EXPLAIN_COMMENTS) e por isso não são traduzidos.
//  - O bloco `#ifdef SQLITE_DEBUG` que confere o `aiMap[]` também some.
//  - SQLITE_OMIT_SUBQUERY, SQLITE_OMIT_FLOATING_POINT e SQLITE_OMIT_HEX_INTEGER não estão
//    definidos no Debian: os ramos que valem são os do caso "não omitido".
//  - Falha de alocação não existe em Rust: `aiMap` e `zAff` são liberados pelo `Drop`, então os
//    rótulos `sqlite3ExprCodeIN_oom_error` viram `return` e `sqlite3ExprCodeIN_finished` vira o
//    bloco rotulado `'finished`.

/// Expr pIn é uma expressão IN(...). Esta função confere se o sub-select do lado direito do
/// operador IN() tem o mesmo número de colunas que o vetor do lado esquerdo. Ou, se o lado direito
/// do IN() não é uma subconsulta, que o lado esquerdo é um vetor de tamanho 1.
pub fn expr_check_in(p_parse: &ParseRef, p_in: &Expr) -> i32 {
    let n_vector = expr_vector_size(p_in.p_left.as_deref().expect("IN sem lado esquerdo"));
    let malloc_failed = p_parse
        .borrow()
        .db
        .upgrade()
        .map_or(false, |db| db.borrow().malloc_failed != 0);
    if expr_use_x_select(p_in) && !malloc_failed {
        let n_expr = p_in
            .x
            .p_select
            .as_ref()
            .and_then(|s| s.p_e_list.as_ref())
            .map_or(0, |l| l.n_expr);
        if n_vector != n_expr {
            subselect_error(p_parse, n_expr, n_vector);
            return 1;
        }
    } else if n_vector != 1 {
        vector_error_msg(p_parse, p_in.p_left.as_deref().expect("IN sem lado esquerdo"));
        return 1;
    }
    0
}

/// Gera código para uma expressão IN.
///
///      x IN (SELECT ...)
///      x IN (value, value, ...)
///
/// O lado esquerdo (LHS) é uma expressão escalar ou vetorial. O lado direito (RHS) é um array de
/// zero ou mais valores escalares, ou uma subconsulta. Se o RHS é uma subconsulta, o número de
/// colunas do resultado precisa casar com o número de colunas do vetor do LHS. Se o RHS é uma
/// lista de valores, o LHS precisa ser escalar.
///
/// O operador IN é verdadeiro se o valor do LHS está contido no RHS. O resultado é falso se o LHS
/// definitivamente não está no RHS. O resultado é NULL se a presença do LHS no RHS não pode ser
/// determinada por causa de NULLs.
///
/// Esta rotina gera código que salta para `dest_if_false` se o LHS não está contido no RHS. Se por
/// causa de NULLs não dá para saber se o LHS está contido no RHS, salta para `dest_if_null`. Se o
/// LHS está contido no RHS, o código segue em frente.
///
/// Veja o arquivo in-operator.md da árvore canônica do SQLite para mais informações.
pub fn expr_code_in(p_parse: &ParseRef, p_expr: &mut Expr, dest_if_false: i32, dest_if_null: i32) {
    let mut r_rhs_has_null: i32 = 0; // Registro que é verdadeiro se o RHS contém NULLs
    let mut i_dummy: i32 = 0; // Parâmetro descartável de expr_code_vector()
    let mut dest_step6: i32 = 0; // Início do código do passo 6
    let mut i_tab: i32 = 0; // Índice a usar
    let ok_const_factor: u8 = p_parse.borrow().ok_const_factor;

    if expr_check_in(p_parse, p_expr) != 0 {
        return;
    }
    let z_aff: Vec<u8> = expr_in_affinity(p_parse, p_expr); // Afinidades das comparações
    let n_vector: i32 = expr_vector_size(p_expr.p_left.as_deref().expect("IN sem lado esquerdo"));
    // Mapa do campo do vetor para a coluna do índice
    let mut ai_map: Vec<i32> = vec![0; n_vector as usize];
    let malloc_failed = p_parse
        .borrow()
        .db
        .upgrade()
        .map_or(false, |db| db.borrow().malloc_failed != 0);
    if malloc_failed {
        return;
    }

    // Tenta calcular o RHS. Depois deste passo, se algo diferente de IN_INDEX_NOOP é devolvido, a
    // tabela aberta com o cursor i_tab contém os valores que formam o RHS. Se IN_INDEX_NOOP é
    // devolvido, o RHS ainda não foi codificado.
    let v: VdbeRef = p_parse
        .borrow()
        .p_vdbe
        .clone()
        .expect("expr_code_in: Parse sem Vdbe");
    let e_type: i32 = find_in_index(
        p_parse,
        p_expr,
        IN_INDEX_MEMBERSHIP | IN_INDEX_NOOP_OK,
        if dest_if_false == dest_if_null {
            None
        } else {
            Some(&mut r_rhs_has_null)
        },
        Some(&mut ai_map[..]),
        &mut i_tab,
    );

    // Codifica o LHS, o <expr> de "<expr> IN (...)". Se o LHS é um vetor, ele é guardado num array
    // de n_vector registros.
    //
    // find_in_index() pode ter reordenado os campos do vetor do LHS para ficarem na mesma ordem de
    // um índice existente. O array ai_map[] guarda o mapa da ordem original dos campos do LHS para
    // a ordem que casa com o índice do RHS.
    //
    // Evita tirar o LHS do IN(...) para fora do laço, mesmo constante, pois o OP_Affinity pode ser
    // usado sobre o registro pelo código gerado abaixo.
    p_parse.borrow_mut().ok_const_factor = 0;
    let r_lhs_orig: i32 = expr_code_vector(
        p_parse,
        p_expr.p_left.as_deref_mut().expect("IN sem lado esquerdo"),
        &mut i_dummy,
    );
    p_parse.borrow_mut().ok_const_factor = ok_const_factor;
    // Os campos do LHS foram reordenados?
    let mut i: i32 = 0;
    while i < n_vector && ai_map[i as usize] == i {
        i += 1;
    }
    let r_lhs: i32;
    if i == n_vector {
        // Os campos do LHS não foram reordenados
        r_lhs = r_lhs_orig;
    } else {
        // Precisa reordenar os campos do LHS conforme ai_map
        r_lhs = get_temp_range(p_parse, n_vector);
        for i in 0..n_vector {
            vdbe_add_op3(&v, OP_COPY as i32, r_lhs_orig + i, r_lhs + ai_map[i as usize], 0);
        }
    }

    'finished: {
        // Se find_in_index() não achou nem criou um índice adequado para avaliar o operador IN,
        // avalia com uma sequência de comparações.
        //
        // Este é o passo (1) do algoritmo otimizado de in-operator.md.
        if e_type == IN_INDEX_NOOP {
            let label_ok: i32 = vdbe_make_label(p_parse);
            let mut reg_ck_null: i32 = 0;
            let p_list: &ExprList = p_expr
                .x
                .p_list
                .as_deref()
                .expect("expr_code_in: IN sem lista");
            let p_coll = expr_coll_seq(p_parse, p_expr.p_left.as_deref());
            if dest_if_null != dest_if_false {
                reg_ck_null = get_temp_reg(p_parse);
                vdbe_add_op3(&v, OP_BITAND as i32, r_lhs, r_lhs, reg_ck_null);
            }
            for ii in 0..p_list.n_expr {
                let p_item: &Expr = p_list.a[ii as usize]
                    .p_expr
                    .as_deref()
                    .expect("expr_code_in: item sem expressão");
                let mut reg_to_free: i32 = 0;
                let r2: i32 = expr_code_temp(p_parse, p_item, &mut reg_to_free);
                if reg_ck_null != 0 && expr_can_be_null(p_item) != 0 {
                    vdbe_add_op3(&v, OP_BITAND as i32, reg_ck_null, r2, reg_ck_null);
                }
                release_temp_reg(p_parse, reg_to_free);
                if ii < p_list.n_expr - 1 || dest_if_null != dest_if_false {
                    let op: u8 = if r_lhs != r2 { OP_EQ } else { OP_NOTNULL };
                    // pColl nulo deixa o P4 sem uso, como o vdbe_change_p4() do C faz com zP4==0.
                    vdbe_add_op4(
                        &v,
                        op as i32,
                        r_lhs,
                        label_ok,
                        r2,
                        p_coll.clone().map_or(P4Value::NotUsed, P4Value::CollSeq),
                        P4_COLLSEQ,
                    );
                    vdbe_change_p5(&v, z_aff[0] as u16);
                } else {
                    let op: u8 = if r_lhs != r2 { OP_NE } else { OP_ISNULL };
                    vdbe_add_op4(
                        &v,
                        op as i32,
                        r_lhs,
                        dest_if_false,
                        r2,
                        p_coll.clone().map_or(P4Value::NotUsed, P4Value::CollSeq),
                        P4_COLLSEQ,
                    );
                    vdbe_change_p5(&v, (z_aff[0] as u16) | (SQLITE_JUMPIFNULL as u16));
                }
            }
            if reg_ck_null != 0 {
                vdbe_add_op2(&v, OP_ISNULL as i32, reg_ck_null, dest_if_null);
                vdbe_goto(&v, dest_if_false);
            }
            vdbe_resolve_label(&v, label_ok);
            release_temp_reg(p_parse, reg_ck_null);
            break 'finished;
        }

        // Passo 2: confere se o LHS tem alguma coluna NULL. Se tem, o resultado só pode ser FALSE
        // ou NULL, e o RHS não é pesquisado.
        let dest_step2: i32;
        if dest_if_null == dest_if_false {
            dest_step2 = dest_if_false;
        } else {
            dest_step6 = vdbe_make_label(p_parse);
            dest_step2 = dest_step6;
        }
        for i in 0..n_vector {
            let p: &Expr = vector_field_subexpr(p_expr.p_left.as_deref().expect("IN sem lado esquerdo"), i);
            if p_parse.borrow().n_err != 0 {
                return;
            }
            if expr_can_be_null(p) != 0 {
                vdbe_add_op2(&v, OP_ISNULL as i32, r_lhs + i, dest_step2);
            }
        }

        // Passo 3: o LHS agora é sabidamente não NULL. Faz a busca binária no RHS usando o LHS como
        // sonda. Se achou, o resultado é verdadeiro.
        let addr_truth_op: i32;
        if e_type == IN_INDEX_ROWID {
            // Neste caso o RHS é o ROWID de uma b-tree de tabela, então também se sabe que o RHS
            // não é NULL. Por isso os passos 3 e 4 se juntam num só opcode.
            vdbe_add_op3(&v, OP_SEEKROWID as i32, i_tab, dest_if_false, r_lhs);
            addr_truth_op = vdbe_add_op0(&v, OP_GOTO as i32); // Retorna verdadeiro
        } else {
            vdbe_add_op4(
                &v,
                OP_AFFINITY as i32,
                r_lhs,
                n_vector,
                0,
                P4Value::Static(z_aff[..n_vector as usize].to_vec()),
                n_vector as i8,
            );
            if dest_if_false == dest_if_null {
                // Junta o passo 3 e o passo 5 num só opcode
                vdbe_add_op4_int(&v, OP_NOTFOUND as i32, i_tab, dest_if_false, r_lhs, n_vector);
                break 'finished;
            }
            // Passo 3 comum, para o caso em que FALSE e NULL são distintos
            addr_truth_op = vdbe_add_op4_int(&v, OP_FOUND as i32, i_tab, 0, r_lhs, n_vector);
        }

        // Passo 4: se o RHS é sabidamente não NULL e a busca acima não achou casamento, o resultado
        // só pode ser FALSE.
        if r_rhs_has_null != 0 && n_vector == 1 {
            vdbe_add_op2(&v, OP_NOTNULL as i32, r_rhs_has_null, dest_if_false);
        }

        // Passo 5: se a diferença entre NULL e FALSE não importa, devolve falso.
        if dest_if_false == dest_if_null {
            vdbe_goto(&v, dest_if_false);
        }

        // Passo 6: percorre as linhas do RHS. Compara cada linha com o LHS. Se alguma comparação é
        // NULL, o resultado é NULL. Se todas as comparações são FALSE, o resultado final é FALSE.
        //
        // Para um LHS escalar, basta conferir a primeira linha do RHS.
        if dest_step6 != 0 {
            vdbe_resolve_label(&v, dest_step6);
        }
        let addr_top: i32 = vdbe_add_op2(&v, OP_REWIND as i32, i_tab, dest_if_false);
        let dest_not_null: i32 = if n_vector > 1 {
            vdbe_make_label(p_parse)
        } else {
            // Para n_vector==1, junta os passos 6 e 7 devolvendo FALSE logo se a primeira
            // comparação não é NULL
            dest_if_false
        };
        for i in 0..n_vector {
            let r3: i32 = get_temp_reg(p_parse);
            let p: &Expr = vector_field_subexpr(p_expr.p_left.as_deref().expect("IN sem lado esquerdo"), i);
            let p_coll = expr_coll_seq(p_parse, Some(p));
            vdbe_add_op3(&v, OP_COLUMN as i32, i_tab, i, r3);
            vdbe_add_op4(
                &v,
                OP_NE as i32,
                r_lhs + i,
                dest_not_null,
                r3,
                p_coll.map_or(P4Value::NotUsed, P4Value::CollSeq),
                P4_COLLSEQ,
            );
            release_temp_reg(p_parse, r3);
        }
        vdbe_add_op2(&v, OP_GOTO as i32, 0, dest_if_null);
        if n_vector > 1 {
            vdbe_resolve_label(&v, dest_not_null);
            vdbe_add_op2(&v, OP_NEXT as i32, i_tab, addr_top + 1);

            // Passo 7: se chegou aqui, o resultado só pode ser falso.
            vdbe_add_op2(&v, OP_GOTO as i32, 0, dest_if_false);
        }

        // Salta para cá para devolver verdadeiro.
        vdbe_jump_here(&v, addr_truth_op);
    }

    // sqlite3ExprCodeIN_finished:
    if r_lhs != r_lhs_orig {
        release_temp_reg(p_parse, r_lhs);
    }
}

/// Gera uma instrução que coloca no registro `i_mem` o valor de ponto flutuante descrito por
/// `z[0..n-1]`.
///
/// A string z[] provavelmente não termina em zero. Mas o caractere z[n] é garantidamente algo que
/// não parece continuação do número.
fn code_real(v: &VdbeRef, z: &[u8], negate_flag: i32, i_mem: i32) {
    let mut value: f64 = 0.0;
    ato_f(z, &mut value, strlen30(z), SQLITE_UTF8);
    if negate_flag != 0 {
        value = -value;
    }
    vdbe_add_op4_dup8(v, OP_REAL as i32, 0, i_mem, 0, &value.to_ne_bytes(), P4_REAL);
}

/// Gera uma instrução que coloca no registro `i_mem` o inteiro descrito pelo texto `z[0..n-1]`.
///
/// Expr.u.zToken é sempre UTF8 e terminado em zero.
fn code_integer(p_parse: &ParseRef, p_expr: &Expr, neg_flag: i32, i_mem: i32) {
    let v: VdbeRef = p_parse
        .borrow()
        .p_vdbe
        .clone()
        .expect("code_integer: Parse sem Vdbe");
    if (p_expr.flags & EP_INT_VALUE) != 0 {
        let mut i: i32 = p_expr.u.i_value;
        if neg_flag != 0 {
            i = -i;
        }
        vdbe_add_op2(&v, OP_INTEGER as i32, i, i_mem);
    } else {
        let mut value: i64 = 0;
        let z: &[u8] = p_expr.u.z_token.as_deref().expect("code_integer: sem zToken");
        let c: i32 = dec_or_hex_to_i64(z, &mut value);
        if (c == 3 && neg_flag == 0) || c == 2 || (neg_flag != 0 && value == SMALLEST_INT64) {
            if strnicmp(z, b"0x", 2) == 0 {
                error_msg(
                    p_parse,
                    b"hex literal too big: %s%#T",
                    &[
                        Value::Text(if neg_flag != 0 { b"-".to_vec() } else { Vec::new() }),
                        Value::Expr(p_expr),
                    ],
                );
            } else {
                code_real(&v, z, neg_flag, i_mem);
            }
        } else {
            if neg_flag != 0 {
                value = if c == 3 { SMALLEST_INT64 } else { -value };
            }
            vdbe_add_op4_dup8(&v, OP_INT64 as i32, 0, i_mem, 0, &value.to_ne_bytes(), P4_INT64);
        }
    }
}


// ---- part_010.rs ----

// Notas desta parte (expr.c, parte 10):
//  - Mesma convenção de empréstimo da parte 9: `Parse*` vira `&ParseRef` e `Vdbe*` vira `&VdbeRef`.
//  - Rotinas que podem chegar a `code_subselect()` / `code_rhs_of_in()` (que mexem em `y.sub`, `i_table`
//    e `flags` do nó) recebem `&mut Expr`. O integrador precisa unificar isso com `expr_code_temp`,
//    `expr_code` e `expr_code_target`, ou dar mutabilidade interior a esses campos do `Expr`.
//  - Os `assert()`, `testcase`, `VdbeCoverage*` e `VdbeComment` não existem no Debian e não são traduzidos.
//  - SQLITE_OMIT_GENERATED_COLUMNS, SQLITE_OMIT_SUBQUERY e SQLITE_UNTESTABLE não estão definidos: os
//    ramos que valem são os do caso "não omitido", e os casos de teste de `INLINEFUNC_*` ficam.
//    SQLITE_ENABLE_OFFSET_SQL_FUNC não está entre as opções do Debian: o caso `sqlite_offset` some.
//  - `sqlite3ColumnExpr` vira `column_expr(&TableRef, &Column) -> Option<Box<Expr>>` (devolve cópia,
//    já que a tabela não pode ficar emprestada durante a geração de código).
//  - `Column` é passado por cópia (`&Column`) nas rotinas de coluna gerada, pelo mesmo motivo.
//  - `IndexedExpr` é compartilhado: `type IndexedExprRef = Rc<IndexedExpr>`, `p_ie_next` é
//    `Option<IndexedExprRef>` e `Parse.p_idx_epr` / `Parse.p_idx_part_expr` guardam a cabeça da lista.

/// Gera código que carrega no registro `reg_out` um valor apropriado para a coluna `i_idx_col` do
/// índice `p_idx`.
pub fn expr_code_load_index_column(
    p_parse: &ParseRef,
    p_idx: &IndexRef,
    i_tab_cur: i32,
    i_idx_col: i32,
    reg_out: i32,
) {
    let i_tab_col: i16 = p_idx.borrow().ai_column[i_idx_col as usize];
    if i_tab_col as i32 == XN_EXPR {
        p_parse.borrow_mut().i_self_tab = i_tab_cur + 1;
        {
            let idx = p_idx.borrow();
            let p_e: &Expr = idx
                .a_col_expr
                .as_deref()
                .expect("índice de expressão sem a_col_expr")
                .a[i_idx_col as usize]
                .p_expr
                .as_deref()
                .expect("coluna de índice sem expressão");
            expr_code_copy(p_parse, Some(p_e), reg_out);
        }
        p_parse.borrow_mut().i_self_tab = 0;
    } else {
        let p_table: TableRef = p_idx.borrow().p_table.upgrade().expect("índice sem tabela");
        let v: VdbeRef = p_parse.borrow().p_vdbe.clone().expect("Parse sem Vdbe");
        expr_code_get_column_of_table(&v, &p_table, i_tab_cur, i_tab_col as i32, reg_out);
    }
}

/// Gera código que calcula o valor da coluna gerada `p_col` e guarda o resultado em `reg_out`.
pub fn expr_code_generated_column(p_parse: &ParseRef, p_tab: &TableRef, p_col: &Column, reg_out: i32) {
    let v: VdbeRef = p_parse.borrow().p_vdbe.clone().expect("Parse sem Vdbe");
    let n_err: i32 = p_parse.borrow().n_err;
    let i_self_tab: i32 = p_parse.borrow().i_self_tab;
    let i_addr: i32 = if i_self_tab > 0 {
        vdbe_add_op3(&v, OP_IFNULLROW as i32, i_self_tab - 1, 0, reg_out)
    } else {
        0
    };
    let p_dflt: Option<Box<Expr>> = column_expr(p_tab, p_col);
    expr_code_copy(p_parse, p_dflt.as_deref(), reg_out);
    if p_col.affinity >= SQLITE_AFF_TEXT {
        vdbe_add_op4(&v, OP_AFFINITY as i32, reg_out, 1, 0, P4Value::Static(vec![p_col.affinity]), 1);
    }
    if i_addr != 0 {
        vdbe_jump_here(&v, i_addr);
    }
    if p_parse.borrow().n_err > n_err {
        let db: DbRef = p_parse.borrow().db.upgrade().expect("Parse sem db");
        db.borrow_mut().err_byte_offset = -1;
    }
}

/// Gera código que extrai o valor da coluna `i_col` de uma tabela.
pub fn expr_code_get_column_of_table(v: &VdbeRef, p_tab: &TableRef, i_tab_cur: i32, i_col: i32, reg_out: i32) {
    let i_p_key: i32 = p_tab.borrow().i_p_key as i32;
    if i_col < 0 || i_col == i_p_key {
        vdbe_add_op2(v, OP_ROWID as i32, i_tab_cur, reg_out);
    } else {
        let op: i32;
        let x: i32;
        if is_virtual(&p_tab.borrow()) {
            op = OP_VCOLUMN as i32;
            x = i_col;
        } else if (p_tab.borrow().a_col[i_col as usize].col_flags & COLFLAG_VIRTUAL) != 0 {
            let p_parse: ParseRef = vdbe_parser(v);
            let col_flags = p_tab.borrow().a_col[i_col as usize].col_flags;
            if (col_flags & COLFLAG_BUSY) != 0 {
                let z_cn_name: Vec<u8> = p_tab.borrow().a_col[i_col as usize].z_cn_name.clone();
                error_msg(&p_parse, b"generated column loop on \"%s\"", &[Value::Text(z_cn_name)]);
            } else {
                let saved_self_tab: i32 = p_parse.borrow().i_self_tab;
                p_tab.borrow_mut().a_col[i_col as usize].col_flags |= COLFLAG_BUSY;
                p_parse.borrow_mut().i_self_tab = i_tab_cur + 1;
                let p_col: Column = p_tab.borrow().a_col[i_col as usize].clone();
                expr_code_generated_column(&p_parse, p_tab, &p_col, reg_out);
                p_parse.borrow_mut().i_self_tab = saved_self_tab;
                p_tab.borrow_mut().a_col[i_col as usize].col_flags &= !COLFLAG_BUSY;
            }
            return;
        } else if !has_rowid(&p_tab.borrow()) {
            let p_pk: IndexRef = primary_key_index(p_tab);
            x = table_column_to_index(&p_pk.borrow(), i_col as i16) as i32;
            op = OP_COLUMN as i32;
        } else {
            x = table_column_to_storage(&p_tab.borrow(), i_col as i16) as i32;
            op = OP_COLUMN as i32;
        }
        vdbe_add_op3(v, op, i_tab_cur, x, reg_out);
        column_default(v, p_tab, i_col, reg_out);
    }
}

/// Gera código que extrai a coluna `i_column` da tabela `p_tab` e guarda o valor no registro `i_reg`.
///
/// Precisa haver um cursor aberto para `p_tab` em `i_table` quando esta rotina é chamada. Se
/// `i_column<0`, o código gerado extrai o rowid.
pub fn expr_code_get_column(
    p_parse: &ParseRef,
    p_tab: &TableRef,
    i_column: i32,
    i_table: i32,
    i_reg: i32,
    p5: u8,
) -> i32 {
    let v: VdbeRef = p_parse.borrow().p_vdbe.clone().expect("Parse sem Vdbe");
    expr_code_get_column_of_table(&v, p_tab, i_table, i_column, i_reg);
    if p5 != 0 {
        let mut vb = v.borrow_mut();
        let i_last: usize = (vb.n_op - 1) as usize;
        let p_op = &mut vb.a_op[i_last];
        if p_op.opcode == OP_COLUMN {
            p_op.p5 = p5 as u16;
        }
        if p_op.opcode == OP_VCOLUMN {
            p_op.p5 = (p5 & OPFLAG_NOCHNG) as u16;
        }
    }
    i_reg
}

/// Gera código que move o conteúdo dos registros `i_from..i_from+n_reg-1` para
/// `i_to..i_to+n_reg-1`.
pub fn expr_code_move(p_parse: &ParseRef, i_from: i32, i_to: i32, n_reg: i32) {
    let v: VdbeRef = p_parse.borrow().p_vdbe.clone().expect("Parse sem Vdbe");
    vdbe_add_op3(&v, OP_MOVE as i32, i_from, i_to, n_reg);
}

/// Converte um nó de expressão escalar num TK_REGISTER que referencia o registro `i_reg`. O chamador
/// garante que `i_reg` já contém o valor correto da expressão.
fn expr_to_register(p_expr: &mut Expr, i_reg: i32) {
    let p: &mut Expr = match expr_skip_collate_and_likely_mut(p_expr) {
        Some(p) => p,
        None => return,
    };
    p.op2 = p.op;
    p.op = TK_REGISTER;
    p.i_table = i_reg;
    expr_clear_property(p, EP_SKIP);
}

/// Avalia uma expressão (vetor ou escalar) e guarda o resultado em registros temporários contíguos.
/// Devolve o índice do primeiro registro usado.
///
/// Se o registro devolvido é um escalar temporário, escreve também esse número em `*pi_freeable`.
/// Se não é temporário, ou se a expressão é um vetor, põe 0 em `*pi_freeable`.
fn expr_code_vector(p_parse: &ParseRef, p: &mut Expr, pi_freeable: &mut i32) -> i32 {
    let i_result: i32;
    let n_result: i32 = expr_vector_size(p);
    if n_result == 1 {
        i_result = expr_code_temp(p_parse, p, pi_freeable);
    } else {
        *pi_freeable = 0;
        if p.op == TK_SELECT {
            i_result = code_subselect(p_parse, p);
        } else {
            i_result = p_parse.borrow().n_mem + 1;
            p_parse.borrow_mut().n_mem += n_result;
            let p_list: &mut ExprList = p.x.p_list.as_deref_mut().expect("vetor sem lista");
            for i in 0..n_result {
                let p_item: &mut Expr = p_list.a[i as usize].p_expr.as_deref_mut().expect("item sem expressão");
                expr_code_factorable(p_parse, p_item, i + i_result);
            }
        }
    }
    i_result
}

/// Se o último opcode é um OP_Copy, liga a flag "não mesclar" (p5) para que uma cópia seguinte não
/// seja mesclada com ele.
fn set_do_not_merge_flag_on_copy(v: &VdbeRef) {
    let is_copy: bool = {
        let vb = v.borrow();
        vb.a_op[(vb.n_op - 1) as usize].opcode == OP_COPY
    };
    if is_copy {
        vdbe_change_p5(v, 1); // Marca o OP_Copy final como não mesclável
    }
}

/// Gera código para as funções SQL especiais que são implementadas em linha, e não pelos callbacks
/// de costume.
fn expr_code_inline_function(p_parse: &ParseRef, p_farg: &mut ExprList, i_func_id: i32, target: i32) -> i32 {
    let v: VdbeRef = p_parse.borrow().p_vdbe.clone().expect("Parse sem Vdbe");
    let n_farg: i32 = p_farg.n_expr; // Toda função em linha tem pelo menos um argumento
    let mut target: i32 = target;
    match i_func_id {
        INLINEFUNC_COALESCE => {
            // Tenta uma implementação direta das funções embutidas COALESCE() e IFNULL(). Isso evita
            // avaliar à toa os argumentos depois do primeiro que não é NULL.
            let end_coalesce: i32 = vdbe_make_label(p_parse);
            expr_code(p_parse, p_farg.a[0].p_expr.as_deref_mut(), target);
            for i in 1..n_farg {
                vdbe_add_op2(&v, OP_NOTNULL as i32, target, end_coalesce);
                expr_code(p_parse, p_farg.a[i as usize].p_expr.as_deref_mut(), target);
            }
            set_do_not_merge_flag_on_copy(&v);
            vdbe_resolve_label(&v, end_coalesce);
        }
        INLINEFUNC_IIF => {
            // A lista de argumentos emprestada vai para dentro do nó CASE e volta depois, porque
            // o C apenas aponta `caseExpr.x.pList` para ela, sem tomar posse.
            let mut case_expr = Expr::default();
            case_expr.op = TK_CASE;
            case_expr.x.p_list = Some(Box::new(std::mem::take(p_farg)));
            let r: i32 = expr_code_target(p_parse, Some(&mut case_expr), target);
            *p_farg = *case_expr.x.p_list.take().expect("lista do CASE sumiu");
            return r;
        }
        INLINEFUNC_EXPR_COMPARE => {
            // Compara duas expressões com expr_compare()
            let r: i32 = expr_compare(
                None,
                p_farg.a[0].p_expr.as_deref(),
                p_farg.a[1].p_expr.as_deref(),
                -1,
            );
            vdbe_add_op2(&v, OP_INTEGER as i32, r, target);
        }
        INLINEFUNC_EXPR_IMPLIES_EXPR => {
            // Compara duas expressões com expr_implies_expr()
            let r: i32 = expr_implies_expr(
                p_parse,
                p_farg.a[0].p_expr.as_deref(),
                p_farg.a[1].p_expr.as_deref(),
                -1,
            );
            vdbe_add_op2(&v, OP_INTEGER as i32, r, target);
        }
        INLINEFUNC_IMPLIES_NONNULL_ROW => {
            // Resultado de expr_implies_non_null_row()
            let p_a1: &Expr = p_farg.a[1].p_expr.as_deref().expect("argumento sem expressão");
            if p_a1.op == TK_COLUMN {
                let i_table: i32 = p_a1.i_table;
                let r: i32 = expr_implies_non_null_row(
                    p_farg.a[0].p_expr.as_deref().expect("argumento sem expressão"),
                    i_table,
                    1,
                );
                vdbe_add_op2(&v, OP_INTEGER as i32, r, target);
            } else {
                vdbe_add_op2(&v, OP_NULL as i32, 0, target);
            }
        }
        INLINEFUNC_AFFINITY => {
            // A função AFFINITY() vale uma string que descreve a afinidade de tipo do argumento. É
            // usada para testar a lógica de tipos do SQLite.
            const AZ_AFF: [&[u8]; 6] = [b"blob", b"text", b"numeric", b"integer", b"real", b"flexnum"];
            let aff: u8 = expr_affinity(p_farg.a[0].p_expr.as_deref());
            let z: &[u8] = if aff <= SQLITE_AFF_NONE {
                b"none"
            } else {
                AZ_AFF[(aff - SQLITE_AFF_BLOB) as usize]
            };
            vdbe_load_string(&v, target, z);
        }
        _ => {
            // A função UNLIKELY() não faz nada. O resultado é o valor do primeiro argumento.
            target = expr_code_target(p_parse, p_farg.a[0].p_expr.as_deref_mut(), target);
        }
    }
    target
}

/// Confere se `p_expr` é uma das expressões indexadas em `p_parse.p_idx_epr`. Se for, resolve a
/// expressão lendo do índice e devolve o registro em que o valor foi lido. Se não for uma expressão
/// indexada, devolve um valor negativo.
fn indexed_expr_lookup(p_parse: &ParseRef, p_expr: &mut Expr, target: i32) -> i32 {
    let mut p: Option<IndexedExprRef> = p_parse.borrow().p_idx_epr.clone();
    while let Some(node) = p {
        p = node.p_ie_next.clone();
        let mut i_data_cur: i32 = node.i_data_cur;
        if i_data_cur < 0 {
            continue;
        }
        let i_self_tab: i32 = p_parse.borrow().i_self_tab;
        if i_self_tab != 0 {
            if node.i_data_cur != i_self_tab - 1 {
                continue;
            }
            i_data_cur = -1;
        }
        if expr_compare(None, Some(&*p_expr), node.p_expr.as_deref(), i_data_cur) != 0 {
            continue;
        }
        let expr_aff: u8 = expr_affinity(Some(&*p_expr));
        if (expr_aff <= SQLITE_AFF_BLOB && node.aff != SQLITE_AFF_BLOB)
            || (expr_aff == SQLITE_AFF_TEXT && node.aff != SQLITE_AFF_TEXT)
            || (expr_aff >= SQLITE_AFF_NUMERIC && node.aff != SQLITE_AFF_NUMERIC)
        {
            // Afinidade incompatível numa coluna gerada
            continue;
        }

        let v: VdbeRef = p_parse.borrow().p_vdbe.clone().expect("Parse sem Vdbe");
        if node.b_maybe_null_row != 0 {
            // Se o índice está numa linha NULL por causa de um outer join, o valor não pode ser
            // extraído do índice. Ele precisa ser calculado com a expressão original.
            let addr: i32 = vdbe_current_addr(&v);
            vdbe_add_op3(&v, OP_IFNULLROW as i32, node.i_idx_cur, addr + 3, target);
            vdbe_add_op3(&v, OP_COLUMN as i32, node.i_idx_cur, node.i_idx_col, target);
            vdbe_goto(&v, 0);
            let p_saved: Option<IndexedExprRef> = p_parse.borrow_mut().p_idx_epr.take();
            expr_code(p_parse, Some(p_expr), target);
            p_parse.borrow_mut().p_idx_epr = p_saved;
            vdbe_jump_here(&v, addr + 2);
        } else {
            vdbe_add_op3(&v, OP_COLUMN as i32, node.i_idx_cur, node.i_idx_col, target);
        }
        return target;
    }
    -1 // Não achou
}


// ---- part_011.rs ----

// Notas desta parte (expr.c, parte 11):
//  - O trecho C 011 termina no meio de `sqlite3ExprCodeTarget()` (dentro do `case TK_UMINUS`) e a função
//    continua no trecho 012. Uma função Rust não pode ser dividida entre arquivos, então a primeira
//    metade vira `expr_code_target_head()`: ela trata o topo do `switch` até o início de TK_UMINUS e
//    devolve um `CodeStep`. O trecho 012 traduz o resto do `switch` (inclusive o `default:`, que é
//    o ramo que gera OP_Null para operador ilegal) como `expr_code_target_tail()`, e o integrador
//    monta `expr_code_target()` com este laço:
//
//        let mut c = ExprCodeCtx::new(target);
//        loop {
//            match expr_code_target_head(p_parse, p_expr, target, &mut c) {
//                CodeStep::Return(r) => return r,
//                CodeStep::Break => break,
//                CodeStep::Next => match expr_code_target_tail(...) {   // pode pedir `goto expr_code_doover`
//                    ...
//                },
//            }
//        }
//        // epílogo (libera reg_free1 e reg_free2) e `return c.in_reg`
//
//    `CodeStep::Break` significa "saiu do switch com `break`": segue para o epílogo da função.
//    `CodeStep::Next` significa "esse operador não é tratado aqui": o resto do switch decide.
//    O `goto expr_code_doover` do C vira uma nova chamada de `expr_code_target_head()` com o mesmo
//    `ExprCodeCtx` (as variáveis locais do C não são reinicializadas no doover).
//  - Os `assert()`, `testcase`, `VdbeCoverage*` e `VdbeComment` não existem no Debian. SQLITE_VDBE_COVERAGE
//    não está definido, então o OP_NotNull de verificação sob esse #ifdef some.
//  - SQLITE_OMIT_FLOATING_POINT, OMIT_BLOB_LITERAL, OMIT_CAST e OMIT_GENERATED_COLUMNS não estão definidos.
//  - `AggInfo` é compartilhado (`AggInfoRef = Rc<RefCell<AggInfo>>`) e `Expr.p_agg_info` é
//    `Option<AggInfoRef>`; os campos de bit do C (`directMode`, `useSortingIdx`) são `u8`.
//  - `Value::Text`, `P4Value::Dynamic` e `P4Value::Static` seguem o que as partes 8 a 10 já usam.

/// Estado local de `sqlite3ExprCodeTarget()` que atravessa a divisão em duas metades.
pub struct ExprCodeCtx {
    pub op: i32,        // O opcode sendo codificado
    pub in_reg: i32,    // Resultados guardados no registro in_reg
    pub reg_free1: i32, // Se diferente de zero, libera este registro temporário
    pub reg_free2: i32, // Se diferente de zero, libera este registro temporário
    pub r1: i32,        // Números de registro diversos
    pub r2: i32,
    pub p5: i32,
}

impl ExprCodeCtx {
    pub fn new(target: i32) -> ExprCodeCtx {
        ExprCodeCtx { op: 0, in_reg: target, reg_free1: 0, reg_free2: 0, r1: 0, r2: 0, p5: 0 }
    }
}

/// O que a primeira metade de `expr_code_target()` decidiu.
pub enum CodeStep {
    /// `return n;` do C
    Return(i32),
    /// `break;` do `switch`: segue para o epílogo da função
    Break,
    /// Operador que o resto do `switch` (trecho 012) trata
    Next,
}

/// Expressão `p_expr` é garantidamente um TK_COLUMN ou equivalente. Esta função consulta a lista
/// `Parse.p_idx_part_expr` para ver se a coluna pode ser trocada por um valor constante. Se puder,
/// gera código que põe o valor constante num registro (idealmente, mas não necessariamente,
/// `i_target`) e devolve o número do registro.
///
/// Se o TK_COLUMN não pode ser trocado por uma constante, devolve zero.
fn expr_partidx_expr_lookup(p_parse: &ParseRef, p_expr: &Expr, i_target: i32) -> i32 {
    let mut p: Option<IndexedExprRef> = p_parse.borrow().p_idx_part_expr.clone();
    while let Some(node) = p {
        if p_expr.i_column as i32 == node.i_idx_col && p_expr.i_table == node.i_data_cur {
            let v: VdbeRef = p_parse.borrow().p_vdbe.clone().expect("Parse sem Vdbe");
            let mut addr: i32 = 0;
            if node.b_maybe_null_row != 0 {
                addr = vdbe_add_op1(&v, OP_IFNULLROW as i32, node.i_idx_cur);
            }
            let ret: i32 = expr_code_target(p_parse, node.p_expr.clone().as_deref_mut(), i_target);
            vdbe_add_op4(&v, OP_AFFINITY as i32, ret, 1, 0, P4Value::Static(vec![node.aff]), 1);
            if addr != 0 {
                vdbe_jump_here(&v, addr);
                vdbe_change_p3(&v, addr, ret);
            }
            return ret;
        }
        p = node.p_ie_next.clone();
    }
    0
}

/// Primeira metade de `sqlite3ExprCodeTarget()`: gera código na Vdbe atual para avaliar a expressão
/// dada, tentando guardar o resultado no registro `target`. Não há garantia de que o resultado fique
/// em `target`; a função chamadora precisa conferir o registro devolvido e mover o resultado para o
/// registro desejado. Veja as notas no topo do arquivo para o protocolo de `CodeStep`.
pub fn expr_code_target_head(
    p_parse: &ParseRef,
    p_expr: Option<&mut Expr>,
    target: i32,
    c: &mut ExprCodeCtx,
) -> CodeStep {
    let v: VdbeRef = p_parse.borrow().p_vdbe.clone().expect("Parse sem Vdbe");

    let e: &mut Expr = match p_expr {
        None => {
            // op = TK_NULL: cai no `default:` do C, que gera OP_Null
            c.op = TK_NULL;
            vdbe_add_op2(&v, OP_NULL as i32, 0, target);
            return CodeStep::Return(target);
        }
        Some(e) => e,
    };
    if p_parse.borrow().p_idx_epr.is_some() && !expr_has_property(e, EP_LEAF) {
        c.r1 = indexed_expr_lookup(p_parse, e, target);
        if c.r1 >= 0 {
            return CodeStep::Return(c.r1);
        }
    }
    c.op = e.op;

    match c.op {
        TK_AGG_COLUMN | TK_COLUMN => {
            if c.op == TK_AGG_COLUMN {
                let p_agg_info: AggInfoRef = e.p_agg_info.clone().expect("TK_AGG_COLUMN sem AggInfo");
                let i_agg: i32 = e.i_agg as i32;
                let a = p_agg_info.borrow();
                if i_agg >= a.n_column {
                    // Acontece quando a tabela da esquerda de um RIGHT JOIN é nula e usa um índice
                    // de expressão
                    vdbe_add_op2(&v, OP_NULL as i32, 0, target);
                    return CodeStep::Break;
                }
                let p_col = &a.a_col[i_agg as usize];
                if a.direct_mode == 0 {
                    return CodeStep::Return(agg_info_column_reg(&a, i_agg));
                } else if a.use_sorting_idx != 0 {
                    vdbe_add_op3(&v, OP_COLUMN as i32, a.sorting_idx_p_tab, p_col.i_sorter_column, target);
                    if let Some(p_tab) = p_col.p_tab.as_ref() {
                        if p_col.i_column >= 0
                            && p_tab.borrow().a_col[p_col.i_column as usize].affinity == SQLITE_AFF_REAL
                        {
                            vdbe_add_op1(&v, OP_REALAFFINITY as i32, target);
                        }
                    }
                    return CodeStep::Return(target);
                } else if e.y.p_tab.is_none() {
                    // Este caso acontece quando o argumento de uma função agregada é reescrito
                    // por aggregate_convert_indexed_expr_ref_to_column()
                    vdbe_add_op3(&v, OP_COLUMN as i32, e.i_table, e.i_column as i32, target);
                    return CodeStep::Return(target);
                }
                // Senão, cai no caso TK_COLUMN
            }

            // case TK_COLUMN:
            let mut i_tab: i32 = e.i_table;
            if expr_has_property(e, EP_FIXEDCOL) {
                // Esta expressão COLUMN é na verdade uma constante por causa das restrições da
                // cláusula WHERE, e essa constante é codificada pela expressão `p_left`. Mesmo
                // assim, garante que a constante tenha o tipo certo aplicando a ela a afinidade
                // da coluna da tabela.
                let i_reg: i32 = expr_code_target(p_parse, e.p_left.as_deref_mut(), target);
                let p_tab: TableRef = e.y.p_tab.clone().expect("FIXEDCOL sem tabela");
                let aff: u8 = table_column_affinity(&p_tab.borrow(), e.i_column);
                if aff > SQLITE_AFF_BLOB {
                    // O C aponta para zAff[(aff-'B')*2], que é a string de um caractere igual a `aff`.
                    vdbe_add_op4(&v, OP_AFFINITY as i32, i_reg, 1, 0, P4Value::Static(vec![aff]), P4_STATIC);
                }
                return CodeStep::Return(i_reg);
            }
            if i_tab < 0 {
                let i_self_tab: i32 = p_parse.borrow().i_self_tab;
                if i_self_tab < 0 {
                    // Outras colunas da mesma linha, para restrições CHECK, colunas geradas ou
                    // inserção num índice parcial. A linha é desempacotada em registros a partir
                    // de 0-(p_parse.i_self_tab). O rowid (se houver) fica num registro logo antes
                    // da primeira coluna.
                    let i_col: i32 = e.i_column as i32;
                    let p_tab: TableRef = e.y.p_tab.clone().expect("coluna sem tabela");
                    if i_col < 0 {
                        return CodeStep::Return(-1 - i_self_tab);
                    }
                    let i_src: i32 = table_column_to_storage(&p_tab.borrow(), i_col as i16) as i32 - i_self_tab;
                    let col_flags = p_tab.borrow().a_col[i_col as usize].col_flags;
                    if (col_flags & COLFLAG_GENERATED) != 0 {
                        if (col_flags & COLFLAG_BUSY) != 0 {
                            let z_cn_name: Vec<u8> = p_tab.borrow().a_col[i_col as usize].z_cn_name.clone();
                            error_msg(p_parse, b"generated column loop on \"%s\"", &[Value::Text(z_cn_name)]);
                            return CodeStep::Return(0);
                        }
                        p_tab.borrow_mut().a_col[i_col as usize].col_flags |= COLFLAG_BUSY;
                        if (col_flags & COLFLAG_NOTAVAIL) != 0 {
                            let p_col: Column = p_tab.borrow().a_col[i_col as usize].clone();
                            expr_code_generated_column(p_parse, &p_tab, &p_col, i_src);
                        }
                        p_tab.borrow_mut().a_col[i_col as usize].col_flags &= !(COLFLAG_BUSY | COLFLAG_NOTAVAIL);
                        return CodeStep::Return(i_src);
                    } else if p_tab.borrow().a_col[i_col as usize].affinity == SQLITE_AFF_REAL {
                        vdbe_add_op2(&v, OP_SCOPY as i32, i_src, target);
                        vdbe_add_op1(&v, OP_REALAFFINITY as i32, target);
                        return CodeStep::Return(target);
                    } else {
                        return CodeStep::Return(i_src);
                    }
                } else {
                    // Codificando uma expressão que é parte de um índice, em que os nomes de coluna
                    // do índice se referem à tabela a que o índice pertence
                    i_tab = i_self_tab - 1;
                }
            } else if p_parse.borrow().p_idx_part_expr.is_some() {
                c.r1 = expr_partidx_expr_lookup(p_parse, e, target);
                if c.r1 != 0 {
                    return CodeStep::Return(c.r1);
                }
            }
            let p_tab: TableRef = e.y.p_tab.clone().expect("coluna sem tabela");
            let i_reg: i32 = expr_code_get_column(p_parse, &p_tab, e.i_column as i32, i_tab, target, e.op2);
            CodeStep::Return(i_reg)
        }
        TK_INTEGER => {
            code_integer(p_parse, e, 0, target);
            CodeStep::Return(target)
        }
        TK_TRUEFALSE => {
            vdbe_add_op2(&v, OP_INTEGER as i32, expr_truth_value(e), target);
            CodeStep::Return(target)
        }
        TK_FLOAT => {
            code_real(&v, e.u.z_token.as_deref().expect("TK_FLOAT sem zToken"), 0, target);
            CodeStep::Return(target)
        }
        TK_STRING => {
            vdbe_load_string(&v, target, e.u.z_token.as_deref().expect("TK_STRING sem zToken"));
            CodeStep::Return(target)
        }
        TK_BLOB => {
            let z_token: &[u8] = e.u.z_token.as_deref().expect("TK_BLOB sem zToken");
            let z: &[u8] = &z_token[2..];
            let n: i32 = strlen30(z) as i32 - 1;
            let z_blob: Vec<u8> = hex_to_blob(&vdbe_db(&v), z, n);
            vdbe_add_op4(&v, OP_BLOB as i32, n / 2, target, 0, P4Value::Dynamic(z_blob), P4_DYNAMIC);
            CodeStep::Return(target)
        }
        TK_VARIABLE => {
            vdbe_add_op2(&v, OP_VARIABLE as i32, e.i_column as i32, target);
            CodeStep::Return(target)
        }
        TK_REGISTER => CodeStep::Return(e.i_table),
        TK_CAST => {
            // Expressões da forma:   CAST(p_left AS token)
            expr_code(p_parse, e.p_left.as_deref_mut(), target);
            let aff: i32 = affinity_type(e.u.z_token.as_deref().expect("TK_CAST sem zToken"), None) as i32;
            vdbe_add_op2(&v, OP_CAST as i32, target, aff);
            CodeStep::Return(c.in_reg)
        }
        TK_IS | TK_ISNOT | TK_LT | TK_LE | TK_GT | TK_GE | TK_NE | TK_EQ => {
            if c.op == TK_IS || c.op == TK_ISNOT {
                c.op = if c.op == TK_IS { TK_EQ } else { TK_NE };
                c.p5 = SQLITE_NULLEQ;
            }
            if expr_is_vector(e.p_left.as_deref().expect("comparação sem lado esquerdo")) {
                code_vector_compare(p_parse, e, target, c.op, c.p5);
            } else {
                c.r1 = expr_code_temp(
                    p_parse,
                    e.p_left.as_deref_mut().expect("comparação sem lado esquerdo"),
                    &mut c.reg_free1,
                );
                c.r2 = expr_code_temp(
                    p_parse,
                    e.p_right.as_deref_mut().expect("comparação sem lado direito"),
                    &mut c.reg_free2,
                );
                vdbe_add_op2(&v, OP_INTEGER as i32, 1, c.in_reg);
                let is_commuted: bool = expr_has_property(e, EP_COMMUTED);
                code_compare(
                    p_parse,
                    e.p_left.as_deref().expect("comparação sem lado esquerdo"),
                    e.p_right.as_deref().expect("comparação sem lado direito"),
                    c.op,
                    c.r1,
                    c.r2,
                    vdbe_current_addr(&v) + 2,
                    c.p5,
                    is_commuted,
                );
                // TK_LT==OP_Lt, TK_LE==OP_Le, TK_GT==OP_Gt, TK_GE==OP_Ge, TK_EQ==OP_Eq, TK_NE==OP_Ne
                if c.p5 == SQLITE_NULLEQ {
                    vdbe_add_op2(&v, OP_INTEGER as i32, 0, c.in_reg);
                } else {
                    vdbe_add_op3(&v, OP_ZEROORNULL as i32, c.r1, c.in_reg, c.r2);
                }
            }
            CodeStep::Break
        }
        TK_AND | TK_OR | TK_PLUS | TK_STAR | TK_MINUS | TK_REM | TK_BITAND | TK_BITOR | TK_SLASH
        | TK_LSHIFT | TK_RSHIFT | TK_CONCAT => {
            // TK_AND==OP_And, TK_OR==OP_Or, TK_PLUS==OP_Add, TK_MINUS==OP_Subtract,
            // TK_REM==OP_Remainder, TK_BITAND==OP_BitAnd, TK_BITOR==OP_BitOr, TK_SLASH==OP_Divide,
            // TK_LSHIFT==OP_ShiftLeft, TK_RSHIFT==OP_ShiftRight, TK_CONCAT==OP_Concat
            c.r1 = expr_code_temp(
                p_parse,
                e.p_left.as_deref_mut().expect("operador sem lado esquerdo"),
                &mut c.reg_free1,
            );
            c.r2 = expr_code_temp(
                p_parse,
                e.p_right.as_deref_mut().expect("operador sem lado direito"),
                &mut c.reg_free2,
            );
            vdbe_add_op3(&v, c.op, c.r2, c.r1, target);
            CodeStep::Break
        }
        TK_UMINUS => {
            let p_left: &Expr = e.p_left.as_deref().expect("TK_UMINUS sem operando");
            if p_left.op == TK_INTEGER {
                code_integer(p_parse, p_left, 1, target);
                CodeStep::Return(target)
            } else if p_left.op == TK_FLOAT {
                code_real(&v, p_left.u.z_token.as_deref().expect("TK_FLOAT sem zToken"), 1, target);
                CodeStep::Return(target)
            } else {
                // O `else` (monta tempX com o literal e chama expr_code_target) está no trecho 012
                CodeStep::Next
            }
        }
        _ => CodeStep::Next,
    }
}


// ---- part_013.rs ----

/// Gera código que avaliará a expressão pExpr apenas uma vez por execução da
/// declaração preparada.
///
/// Se a expressão usa funções (que podem lançar uma exceção), guarde-as com
/// um opcode OP_Once para garantir que o código seja executado apenas uma vez.
/// Se nenhuma função está envolvida, fatore o código e coloque-o no final da
/// declaração preparada na seção de inicialização.
///
/// Se regDest > 0, o resultado é sempre armazenado naquele registro e não é
/// reutilizável. Se regDest < 0, esta rotina é livre para armazenar o valor
/// aonde quiser. O registro onde a expressão é armazenada é devolvido. Quando
/// regDest < 0, duas expressões idênticas podem codificar para o mesmo registro,
/// se não contiverem chamadas de função e, portanto, forem fatoradas na seção
/// de inicialização no final da declaração preparada.
pub fn expr_code_run_just_once(
    p_parse: &mut Parse,
    p_expr: Option<&Expr>,
    mut reg_dest: i32,
) -> i32 {
    assert!(const_factor_ok(p_parse));
    assert!(reg_dest != 0);
    if reg_dest < 0 {
        if let Some(p) = p_parse.p_const_expr.as_ref() {
            for p_item in p.a.iter().take(p.n_expr as usize) {
                if !p_item.fg.reusable {
                    continue;
                }
                let cmp = match (p_item.p_expr.as_deref(), p_expr) {
                    (Some(a), Some(b)) => expr_compare(None, a, b, -1),
                    (None, None) => 0,
                    _ => 2,
                };
                if cmp == 0 {
                    return p_item.u.i_const_expr_reg;
                }
            }
        }
    }
    let db = p_parse.db.clone().unwrap();
    let p_dup = expr_dup(&db, p_expr, 0);
    if p_dup
        .as_deref()
        .map_or(false, |e| expr_has_property(e, EP_HAS_FUNC))
    {
        let v = p_parse.p_vdbe.clone().unwrap();
        let addr = vdbe_add_op0(&mut v.borrow_mut(), OP_ONCE as i32);
        p_parse.ok_const_factor = 0;
        if !db.borrow().malloc_failed {
            if reg_dest < 0 {
                p_parse.n_mem += 1;
                reg_dest = p_parse.n_mem;
            }
            expr_code(p_parse, p_dup.as_deref(), reg_dest);
        }
        p_parse.ok_const_factor = 1;
        expr_delete(&db, p_dup);
        vdbe_jump_here(&mut v.borrow_mut(), addr);
    } else {
        // O pConstExpr só é tirado do Parse aqui, sem recursão entre o take e a
        // devolução, para a lista não se perder.
        let p = p_parse.p_const_expr.take();
        let mut p = expr_list_append(p_parse, p, p_dup);
        if let Some(p_list) = p.as_mut() {
            let last = (p_list.n_expr - 1) as usize;
            p_list.a[last].fg.reusable = reg_dest < 0;
            if reg_dest < 0 {
                p_parse.n_mem += 1;
                reg_dest = p_parse.n_mem;
            }
            p_list.a[last].u.i_const_expr_reg = reg_dest;
        }
        p_parse.p_const_expr = p;
    }
    reg_dest
}

/// Gera código para avaliar uma expressão e armazenar os resultados em um
/// registro. Retorna o número do registro onde os resultados são armazenados.
///
/// Se o registro é um registro temporário que pode ser desalocado, então
/// escreva seu número em *pReg. Se o registro de resultado não é temporário,
/// então defina *pReg como zero.
///
/// Se pExpr é uma constante, então esta rotina pode gerar código para preencher
/// o registro na seção de inicialização do programa VDBE, para fatorá-lo fora do
/// laço de avaliação.
pub fn expr_code_temp(p_parse: &mut Parse, p_expr: Option<&Expr>, p_reg: &mut i32) -> i32 {
    let r2: i32;
    let p_expr = expr_skip_collate_and_likely(p_expr);
    let factor = match p_expr {
        Some(e) => {
            const_factor_ok(p_parse)
                && e.op != TK_REGISTER
                && expr_is_constant_not_join(p_parse, e)
        }
        None => false,
    };
    if factor {
        *p_reg = 0;
        r2 = expr_code_run_just_once(p_parse, p_expr, -1);
    } else {
        let r1 = get_temp_reg(p_parse);
        r2 = expr_code_target(p_parse, p_expr, r1);
        if r2 == r1 {
            *p_reg = r1;
        } else {
            release_temp_reg(p_parse, r1);
            *p_reg = 0;
        }
    }
    r2
}

/// Gera código que avaliará a expressão pExpr e armazenará os resultados no
/// registro alvo. Os resultados são garantidos de aparecerem no registro alvo.
pub fn expr_code(p_parse: &mut Parse, p_expr: Option<&Expr>, target: i32) {
    assert!(p_expr.map_or(true, |e| !expr_has_vva_property(e, EP_IMMUTABLE)));
    assert!(target > 0 && target <= p_parse.n_mem);
    assert!(p_parse.p_vdbe.is_some() || p_parse.db.as_ref().unwrap().borrow().malloc_failed);
    if p_parse.p_vdbe.is_none() {
        return;
    }
    let in_reg = expr_code_target(p_parse, p_expr, target);
    if in_reg != target {
        let op: u8;
        let p_x = expr_skip_collate_and_likely(p_expr);
        if p_x.map_or(false, |x| expr_has_property(x, EP_SUBQUERY) || x.op == TK_REGISTER) {
            op = OP_COPY;
        } else {
            op = OP_SCOPY;
        }
        let v = p_parse.p_vdbe.clone().unwrap();
        vdbe_add_op2(&mut v.borrow_mut(), op as i32, in_reg, target);
    }
}

/// Faz uma cópia transitória da expressão pExpr e então a codifica usando
/// expr_code(). Esta rotina funciona exatamente como expr_code() exceto que a
/// expressão de entrada é garantida de ser inalterada.
pub fn expr_code_copy(p_parse: &mut Parse, p_expr: Option<&Expr>, target: i32) {
    let db = p_parse.db.clone().unwrap();
    let p_dup = expr_dup(&db, p_expr, 0);
    if !db.borrow().malloc_failed {
        expr_code(p_parse, p_dup.as_deref(), target);
    }
    expr_delete(&db, p_dup);
}

/// Gera código que avaliará a expressão pExpr e armazenará os resultados no
/// registro alvo. Os resultados são garantidos de aparecerem no registro alvo.
/// Se a expressão é uma constante, então esta rotina pode escolher codificar a
/// expressão no tempo de inicialização.
pub fn expr_code_factorable(p_parse: &mut Parse, p_expr: &Expr, target: i32) {
    if p_parse.ok_const_factor != 0 && expr_is_constant_not_join(p_parse, p_expr) {
        expr_code_run_just_once(p_parse, Some(p_expr), target);
    } else {
        expr_code_copy(p_parse, Some(p_expr), target);
    }
}

/// Gera código que empurra o valor de cada elemento da lista de expressões
/// fornecida para uma sequência de registros começando no alvo.
///
/// Retorna o número de elementos avaliados. O número devolvido será normalmente
/// pList->nExpr, mas pode ser reduzido se SQLITE_ECEL_OMITREF está definido.
///
/// A flag SQLITE_ECEL_DUP impede que os argumentos sejam preenchidos usando
/// OP_SCopy. OP_Copy deve ser usado em vez disso.
///
/// O argumento SQLITE_ECEL_FACTOR permite que argumentos constantes sejam
/// fatorados no código de inicialização.
///
/// A flag SQLITE_ECEL_REF significa que expressões na lista com
/// ExprList.a[].u.x.iOrderByCol > 0 já foram avaliadas e armazenadas em
/// registros em srcReg, e assim o valor pode ser copiado de lá. Se
/// SQLITE_ECEL_OMITREF também está definido, então os valores com
/// u.x.iOrderByCol > 0 são simplesmente omitidos em vez de serem copiados
/// de srcReg.
pub fn expr_code_expr_list(
    p_parse: &mut Parse,
    p_list: &ExprList,
    target: i32,
    src_reg: i32,
    flags: u8,
) -> i32 {
    let copy_op: u8 = if (flags & SQLITE_ECEL_DUP) != 0 {
        OP_COPY
    } else {
        OP_SCOPY
    };
    let v = p_parse.p_vdbe.clone().unwrap();
    assert!(target > 0);
    let mut n = p_list.n_expr;
    let mut flags = flags;
    if !const_factor_ok(p_parse) {
        flags &= !SQLITE_ECEL_FACTOR;
    }
    // i é o deslocamento do registro (recuado quando um item é omitido) e idx
    // percorre os itens da lista, como o pItem++ do C.
    let mut i: i32 = 0;
    let mut idx: usize = 0;
    while i < n {
        let p_item = &p_list.a[idx];
        let p_expr = p_item.p_expr.as_deref();
        let j = p_item.u.x.i_order_by_col;
        if (flags & SQLITE_ECEL_REF) != 0 && j > 0 {
            if (flags & SQLITE_ECEL_OMITREF) != 0 {
                i -= 1;
                n -= 1;
            } else {
                vdbe_add_op2(&mut v.borrow_mut(), copy_op as i32, j + src_reg - 1, target + i);
            }
        } else if (flags & SQLITE_ECEL_FACTOR) != 0
            && expr_is_constant_not_join(p_parse, p_expr.unwrap())
        {
            expr_code_run_just_once(p_parse, p_expr, target + i);
        } else {
            let in_reg = expr_code_target(p_parse, p_expr, target + i);
            if in_reg != target + i {
                let mut vm = v.borrow_mut();
                let merge = copy_op == OP_COPY && {
                    let p_op = vdbe_get_last_op(&mut vm);
                    p_op.opcode == OP_COPY
                        && p_op.p1 + p_op.p3 + 1 == in_reg
                        && p_op.p2 + p_op.p3 + 1 == target + i
                        && p_op.p5 == 0 // a flag de não fundir precisa estar limpa
                };
                if merge {
                    vdbe_get_last_op(&mut vm).p3 += 1;
                } else {
                    vdbe_add_op2(&mut vm, copy_op as i32, in_reg, target + i);
                }
            }
        }
        i += 1;
        idx += 1;
    }
    n
}

/// Gera código para um operador BETWEEN.
///
///    x BETWEEN y AND z
///
/// O acima é equivalente a
///
///    x >= y AND x <= z
///
/// Codifique-o desta forma, tomando cuidado para fazer a eliminação de
/// subexpressão comum de x.
///
/// O parâmetro xJump determina os detalhes:
///
///    None:                  armazene o resultado booleano em reg[dest]
///    Some(expr_if_true):    salte para dest se verdadeiro
///    Some(expr_if_false):   salte para dest se falso
///
/// O parâmetro jumpIfNull é ignorado se xJump é None.
fn expr_code_between(
    p_parse: &mut Parse,
    p_expr: &Expr,
    dest: i32,
    x_jump: Option<fn(&mut Parse, &Expr, i32, i32)>,
    jump_if_null: i32,
) {
    let mut reg_free1: i32 = 0;
    let db = p_parse.db.clone().unwrap();

    assert!(expr_use_x_list(p_expr));
    let mut p_del = expr_dup(&db, p_expr.p_left.as_deref(), 0);
    if !db.borrow().malloc_failed {
        // No C, pDel é um ponteiro compartilhado pelos dois termos de
        // comparação e o exprToRegister o transforma em TK_REGISTER. Aqui os
        // termos recebem cópias, então a conversão e a marca EP_OuterON são
        // feitas em pDel antes de ele ser copiado para compLeft e compRight.
        {
            let del = p_del.as_deref_mut().unwrap();
            let r = expr_code_vector(p_parse, del, &mut reg_free1);
            expr_to_register(del, r);
            if x_jump.is_none() {
                // Marca a expressão como vinda de uma cláusula ON ou USING de
                // um join para que expr_code_target() não tente movê-la para a
                // lista Parse.pConstExpr. Deveria haver um bit novo para isso,
                // mas os bits de Expr.flags acabaram, então reaproveita-se o
                // bit EP_OuterON.
                del.flags |= EP_OUTER_ON;
            }
        }
        let p_list = p_expr.x.p_list.as_ref().unwrap();
        let mut comp_left = Expr::default();
        let mut comp_right = Expr::default();
        let mut expr_and = Expr::default();
        comp_left.op = TK_GE;
        comp_left.p_left = p_del.clone();
        comp_left.p_right = p_list.a[0].p_expr.clone();
        comp_right.op = TK_LE;
        comp_right.p_left = p_del.clone();
        comp_right.p_right = p_list.a[1].p_expr.clone();
        expr_and.op = TK_AND;
        expr_and.p_left = Some(Box::new(comp_left));
        expr_and.p_right = Some(Box::new(comp_right));
        if let Some(x_jump_fn) = x_jump {
            x_jump_fn(p_parse, &expr_and, dest, jump_if_null);
        } else {
            expr_code_target(p_parse, Some(&expr_and), dest);
        }
        release_temp_reg(p_parse, reg_free1);
    }
    expr_delete(&db, p_del);
}

/// Gera código para uma expressão booleana tal que um salto é feito para o
/// rótulo "dest" se a expressão for verdadeira, mas a execução continua
/// diretamente se a expressão for falsa.
///
/// Se a expressão é avaliada como NULL (nem verdadeira nem falsa), então faça o
/// salto se a flag jumpIfNull for SQLITE_JUMPIFNULL.
///
/// Este código depende do fato de que certos valores de token (ex: TK_EQ) são os
/// mesmos que valores de opcode (ex: OP_Eq) que implementam a operação
/// correspondente. Comentários especiais em vdbe.c e o script mkopcodeh.awk no
/// processo de compilação causam estes valores a se alinharem. Assert()s no
/// código abaixo verificam que os números estão alinhados corretamente.
pub fn expr_if_true(p_parse: &mut Parse, p_expr: &Expr, dest: i32, jump_if_null: i32) {
    let mut jump_if_null = jump_if_null;
    let mut reg_free1: i32 = 0;
    let mut reg_free2: i32 = 0;

    assert!(jump_if_null == SQLITE_JUMPIFNULL as i32 || jump_if_null == 0);
    let v = match p_parse.p_vdbe.clone() {
        Some(v) => v,
        None => return, // a existência do VDBE é conferida pelo chamador
    };
    assert!(!expr_has_vva_property(p_expr, EP_IMMUTABLE));
    let mut op: u8 = p_expr.op;
    let mut default_expr = false;

    match op {
        TK_AND | TK_OR => {
            let p_alt = expr_simplified_and_or(p_expr);
            if !std::ptr::eq(p_alt, p_expr) {
                expr_if_true(p_parse, p_alt, dest, jump_if_null);
            } else if op == TK_AND {
                let d2 = vdbe_make_label(p_parse);
                expr_if_false(
                    p_parse,
                    p_expr.p_left.as_deref().unwrap(),
                    d2,
                    jump_if_null ^ SQLITE_JUMPIFNULL as i32,
                );
                expr_if_true(p_parse, p_expr.p_right.as_deref().unwrap(), dest, jump_if_null);
                vdbe_resolve_label(&mut v.borrow_mut(), d2);
            } else {
                expr_if_true(p_parse, p_expr.p_left.as_deref().unwrap(), dest, jump_if_null);
                expr_if_true(p_parse, p_expr.p_right.as_deref().unwrap(), dest, jump_if_null);
            }
        }
        TK_NOT => {
            expr_if_false(p_parse, p_expr.p_left.as_deref().unwrap(), dest, jump_if_null);
        }
        TK_TRUTH => {
            let is_not = p_expr.op2 == TK_ISNOT; // IS NOT TRUE ou IS NOT FALSE
            let is_true = expr_truth_value(p_expr.p_right.as_deref().unwrap()) != 0;
            let jin = if is_not { SQLITE_JUMPIFNULL as i32 } else { 0 };
            if is_true ^ is_not {
                expr_if_true(p_parse, p_expr.p_left.as_deref().unwrap(), dest, jin);
            } else {
                expr_if_false(p_parse, p_expr.p_left.as_deref().unwrap(), dest, jin);
            }
        }
        TK_IS | TK_ISNOT | TK_LT | TK_LE | TK_GT | TK_GE | TK_NE | TK_EQ => {
            // No C, TK_IS e TK_ISNOT ajustam op e jumpIfNull e caem (deliberate
            // fall through) no mesmo corpo dos operadores de comparação.
            if op == TK_IS || op == TK_ISNOT {
                op = if op == TK_IS { TK_EQ } else { TK_NE };
                jump_if_null = SQLITE_NULLEQ as i32;
            }
            if expr_is_vector(p_expr.p_left.as_deref()) {
                default_expr = true;
            } else {
                let left = p_expr.p_left.as_deref().unwrap();
                let right = p_expr.p_right.as_deref().unwrap();
                let r1 = expr_code_temp(p_parse, Some(left), &mut reg_free1);
                let r2 = expr_code_temp(p_parse, Some(right), &mut reg_free2);
                code_compare(
                    p_parse,
                    left,
                    right,
                    op as i32,
                    r1,
                    r2,
                    dest,
                    jump_if_null,
                    expr_has_property(p_expr, EP_COMMUTED),
                );
                assert!(TK_LT == OP_LT);
                assert!(TK_LE == OP_LE);
                assert!(TK_GT == OP_GT);
                assert!(TK_GE == OP_GE);
                assert!(TK_EQ == OP_EQ);
                assert!(TK_NE == OP_NE);
            }
        }
        TK_ISNULL | TK_NOTNULL => {
            assert!(TK_ISNULL == OP_ISNULL);
            assert!(TK_NOTNULL == OP_NOTNULL);
            let r1 = expr_code_temp(p_parse, p_expr.p_left.as_deref(), &mut reg_free1);
            vdbe_typeof_column(&mut v.borrow_mut(), r1);
            vdbe_add_op2(&mut v.borrow_mut(), op as i32, r1, dest);
        }
        TK_BETWEEN => {
            expr_code_between(p_parse, p_expr, dest, Some(expr_if_true), jump_if_null);
        }
        TK_IN => {
            let dest_if_false = vdbe_make_label(p_parse);
            let dest_if_null = if jump_if_null != 0 { dest } else { dest_if_false };
            expr_code_in(p_parse, p_expr, dest_if_false, dest_if_null);
            vdbe_goto(&mut v.borrow_mut(), dest);
            vdbe_resolve_label(&mut v.borrow_mut(), dest_if_false);
        }
        _ => {
            default_expr = true;
        }
    }
    if default_expr {
        if expr_always_true(p_expr) {
            vdbe_goto(&mut v.borrow_mut(), dest);
        } else if expr_always_false(p_expr) {
            // Nada a fazer
        } else {
            let r1 = expr_code_temp(p_parse, Some(p_expr), &mut reg_free1);
            vdbe_add_op3(
                &mut v.borrow_mut(),
                OP_IF as i32,
                r1,
                dest,
                (jump_if_null != 0) as i32,
            );
        }
    }
    release_temp_reg(p_parse, reg_free1);
    release_temp_reg(p_parse, reg_free2);
}


// ---- part_015.rs ----

/// Como sqlite3ExprCompare(), mas operadores COLLATE no topo nível são ignorados.
pub fn expr_compare_skip(p_a: &Expr, p_b: &Expr, i_tab: i32) -> i32 {
    expr_compare(
        None,
        expr_skip_collate(Some(p_a)).unwrap(),
        expr_skip_collate(Some(p_b)).unwrap(),
        i_tab,
    )
}

/// Devolve não zero se a expressão p só pode ser verdadeira se pNN não for NULL.
///
/// Ou se seenNot for verdadeiro, devolve não zero se a expressão p só pode
/// ser não NULL se pNN não for NULL.
fn expr_implies_not_null(
    p_parse: Option<&Parse>,
    p: &Expr,
    p_nn: &Expr,
    i_tab: i32,
    seen_not: i32,
) -> i32 {
    if expr_compare(p_parse, p, p_nn, i_tab) == 0 {
        return (p_nn.op != TK_NULL) as i32;
    }

    match p.op {
        TK_IN => {
            if seen_not != 0 && expr_has_property(p, EP_X_IS_SELECT) {
                return 0;
            }
            debug_assert!(expr_use_x_select(p) || (p.x.p_list.is_some() && p.x.p_list.as_ref().unwrap().n_expr > 0));
            return expr_implies_not_null(p_parse, p.p_left.as_ref().unwrap(), p_nn, i_tab, 1);
        }
        TK_BETWEEN => {
            debug_assert!(expr_use_x_list(p));
            let p_list = &p.x.p_list;
            debug_assert!(p_list.is_some());
            let p_list = p_list.as_ref().unwrap();
            debug_assert_eq!(p_list.n_expr, 2);

            if seen_not != 0 {
                return 0;
            }

            if expr_implies_not_null(p_parse, p_list.a[0].p_expr.as_deref().unwrap(), p_nn, i_tab, 1) != 0
                || expr_implies_not_null(p_parse, p_list.a[1].p_expr.as_deref().unwrap(), p_nn, i_tab, 1) != 0
            {
                return 1;
            }

            return expr_implies_not_null(p_parse, p.p_left.as_ref().unwrap(), p_nn, i_tab, 1);
        }
        TK_EQ | TK_NE | TK_LT | TK_LE | TK_GT | TK_GE | TK_PLUS | TK_MINUS | TK_BITOR
        | TK_LSHIFT | TK_RSHIFT | TK_CONCAT => {
            let seen_not = 1;
            if expr_implies_not_null(p_parse, p.p_right.as_ref().unwrap(), p_nn, i_tab, seen_not) != 0 {
                return 1;
            }
            // queda deliberada
            return expr_implies_not_null(p_parse, p.p_left.as_ref().unwrap(), p_nn, i_tab, seen_not);
        }
        TK_STAR | TK_REM | TK_BITAND | TK_SLASH => {
            if expr_implies_not_null(p_parse, p.p_right.as_ref().unwrap(), p_nn, i_tab, seen_not) != 0 {
                return 1;
            }
            // queda deliberada
            return expr_implies_not_null(p_parse, p.p_left.as_ref().unwrap(), p_nn, i_tab, seen_not);
        }
        TK_SPAN | TK_COLLATE | TK_UPLUS | TK_UMINUS => {
            return expr_implies_not_null(p_parse, p.p_left.as_ref().unwrap(), p_nn, i_tab, seen_not);
        }
        TK_TRUTH => {
            if seen_not != 0 {
                return 0;
            }
            if p.op2 != TK_IS {
                return 0;
            }
            return expr_implies_not_null(p_parse, p.p_left.as_ref().unwrap(), p_nn, i_tab, 1);
        }
        TK_BITNOT | TK_NOT => {
            return expr_implies_not_null(p_parse, p.p_left.as_ref().unwrap(), p_nn, i_tab, 1);
        }
        _ => {
            return 0;
        }
    }
}

/// Devolve verdadeiro se conseguimos provar que pE2 sempre será verdadeiro se pE1 é verdadeiro.
/// Devolve falso se não conseguimos completar a prova ou se pE2 pode ser falso. Exemplos:
///
///     pE1: x==5       pE2: x==5             Resultado: verdadeiro
///     pE1: x>0        pE2: x==5             Resultado: falso
///     pE1: x=21       pE2: x=21 OR y=43     Resultado: verdadeiro
///     pE1: x!=123     pE2: x IS NOT NULL    Resultado: verdadeiro
///     pE1: x!=?1      pE2: x IS NOT NULL    Resultado: verdadeiro
///     pE1: x IS NULL  pE2: x IS NOT NULL    Resultado: falso
///     pE1: x IS ?2    pE2: x IS NOT NULL    Resultado: falso
///
/// Quando comparar nós TK_COLUMN entre pE1 e pE2, se pE2 tem Expr.iTable<0
/// então assume um número de tabela dado por iTab.
///
/// Se pParse não for NULL, então os valores de variáveis vinculadas em pE1 são
/// comparados contra valores literais em pE2 e pParse->pVdbe->expmask é
/// modificado para registrar quais variáveis vinculadas são referenciadas. Se pParse
/// for NULL, então falso será devolvido se pE1 contiver qualquer variável vinculada.
///
/// Em dúvida, devolva falso. Devolver verdadeiro pode dar uma melhoria de desempenho.
/// Devolver falso pode causar uma redução de desempenho, mas sempre dará a resposta correta
/// e portanto é sempre seguro.
pub fn expr_implies_expr(p_parse: Option<&Parse>, p_e1: &Expr, p_e2: &Expr, i_tab: i32) -> i32 {
    if expr_compare(p_parse, p_e1, p_e2, i_tab) == 0 {
        return 1;
    }

    if p_e2.op == TK_OR
        && (expr_implies_expr(p_parse, p_e1, p_e2.p_left.as_ref().unwrap(), i_tab) != 0
            || expr_implies_expr(p_parse, p_e1, p_e2.p_right.as_ref().unwrap(), i_tab) != 0)
    {
        return 1;
    }

    if p_e2.op == TK_NOTNULL
        && expr_implies_not_null(p_parse, p_e1, p_e2.p_left.as_ref().unwrap(), i_tab, 0) != 0
    {
        return 1;
    }

    return 0;
}

/// Isto é uma função auxiliar para impliesNotNullRow(). Nesta rotina,
/// coloque pWalker->eCode para um só se *ambas* as expressões de entrada
/// separadamente têm a propriedade impliedNotNullRow.
fn both_imply_not_null_row(p_walker: &mut Walker, p_e1: &mut Expr, p_e2: &mut Expr) {
    if p_walker.e_code == 0 {
        walk_expr_nn(p_walker, p_e1);
        if p_walker.e_code != 0 {
            p_walker.e_code = 0;
            walk_expr_nn(p_walker, p_e2);
        }
    }
}

/// Este é o callback do nó Expr para sqlite3ExprImpliesNonNullRow().
/// Se o nó de expressão requer que a tabela em pWalker->iCur
/// tenha uma ou mais colunas não NULL, então coloque pWalker->eCode para 1 e aborte.
///
/// pWalker->mWFlags é não zero se esta consulta está sendo feita em
/// nome de um RIGHT JOIN (ou FULL JOIN). Isso faz diferença quando
/// avaliando termos na cláusula ON de um inner join.
///
/// Esta rotina controla uma otimização. Falsos positivos (colocando
/// pWalker->eCode para 1 quando não deveria) são mortais, mas falsos negativos
/// (nunca colocando pWalker->eCode) é uma otimização perdida inofensiva.
fn implies_not_null_row(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    if expr_has_property(p_expr, EP_OUTER_ON) {
        return WRC_PRUNE;
    }
    if expr_has_property(p_expr, EP_INNER_ON) && p_walker.m_w_flags != 0 {
        // Se iCur é usado em uma cláusula ON de inner-join à esquerda de um
        // RIGHT JOIN, isso NÃO significa que a tabela deve ser não nula.
        // Mas é difícil conferir essa condição precisamente.
        // Para manter as coisas simples, qualquer uso de iCur de qualquer inner-join é
        // ignorado ao tentar simplificar um RIGHT JOIN.
        return WRC_PRUNE;
    }

    match p_expr.op {
        TK_ISNOT | TK_ISNULL | TK_NOTNULL | TK_IS | TK_VECTOR | TK_FUNCTION
        | TK_TRUTH | TK_CASE => {
            return WRC_PRUNE;
        }

        TK_COLUMN => {
            if let WalkerU::ICur(i_cur) = p_walker.u {
                if i_cur == p_expr.i_table {
                    p_walker.e_code = 1;
                    return WRC_ABORT;
                }
            }
            return WRC_PRUNE;
        }

        TK_OR | TK_AND => {
            // Ambos os lados de um AND ou OR devem separadamente implicar non-null-row.
            // Considerando estes casos:
            //    1.  NOT (x AND y)
            //    2.  x OR y
            // Se apenas um de x ou y é non-null-row, então a expressão geral
            // pode ser verdadeira se o outro braço é falso (caso 1) ou verdadeiro (caso 2).
            both_imply_not_null_row(p_walker, p_expr.p_left.as_mut().unwrap(), p_expr.p_right.as_mut().unwrap());
            return WRC_PRUNE;
        }

        TK_IN => {
            // Cuidado com "x NOT IN ()" e "x NOT IN (SELECT 1 WHERE false)",
            // ambas podem ser verdadeiras. Mas à parte desses casos, se
            // o lado esquerdo do IN é NULL então o IN mesmo será NULL.
            if expr_use_x_list(p_expr) && p_expr.x.p_list.as_ref().map_or(false, |l| l.n_expr > 0) {
                walk_expr_nn(p_walker, p_expr.p_left.as_mut().unwrap());
            }
            return WRC_PRUNE;
        }

        TK_BETWEEN => {
            // Em "x NOT BETWEEN y AND z" ou x deve ser non-null-row ou então
            // ambos y e z devem ser non-null row
            debug_assert!(expr_use_x_list(p_expr));
            debug_assert_eq!(p_expr.x.p_list.as_ref().unwrap().n_expr, 2);

            walk_expr_nn(p_walker, p_expr.p_left.as_mut().unwrap());
            // split_at_mut: os dois itens da mesma lista são emprestados juntos
            let (first, second) = p_expr.x.p_list.as_mut().unwrap().a.split_at_mut(1);
            both_imply_not_null_row(
                p_walker,
                first[0].p_expr.as_deref_mut().unwrap(),
                second[0].p_expr.as_deref_mut().unwrap(),
            );
            return WRC_PRUNE;
        }

        // Tabelas virtuais são permitidas usar restrições como x=NULL. Então
        // um termo da forma x=y não prova que y não é nulo se x
        // for a coluna de uma tabela virtual
        TK_EQ | TK_NE | TK_LT | TK_LE | TK_GT | TK_GE => {
            let p_left = p_expr.p_left.as_deref().unwrap();
            let p_right = p_expr.p_right.as_deref().unwrap();

            // A atribuição y.pTab=0 em wherecode.c sempre acontece após o
            // teste impliesNotNullRow()
            debug_assert!(p_left.op != TK_COLUMN || expr_use_y_tab(p_left));
            debug_assert!(p_right.op != TK_COLUMN || expr_use_y_tab(p_right));

            if (p_left.op == TK_COLUMN
                && p_left.y.p_tab.is_some()
                && is_virtual(p_left.y.p_tab.as_ref().unwrap()))
                || (p_right.op == TK_COLUMN
                    && p_right.y.p_tab.is_some()
                    && is_virtual(p_right.y.p_tab.as_ref().unwrap()))
            {
                return WRC_PRUNE;
            }
            // queda deliberada
            return WRC_CONTINUE;
        }

        _ => {
            return WRC_CONTINUE;
        }
    }
}

/// Devolve verdadeiro (não zero) se a expressão p só pode ser verdadeira se pelo menos
/// uma coluna da tabela iTab for não nula. Em outras palavras, devolva verdadeiro
/// se a expressão p sempre será NULL ou falsa se toda coluna de iTab
/// for NULL.
///
/// Falsos negativos são aceitáveis. Em outras palavras, é ok devolver
/// zero mesmo se a expressão p nunca for verdadeira se toda coluna de iTab
/// for NULL. Um falso negativo é apenas uma oportunidade de otimização perdida.
///
/// Falsos positivos não são permitidos, porém. Um falso positivo pode resultar
/// em uma resposta incorreta.
///
/// Termos de p que estão marcados com EP_OuterON (e portanto vêm das
/// cláusulas ON ou USING de OUTER JOINS) são excluídos da análise.
///
/// Esta rotina é usada para conferir se um LEFT JOIN pode ser convertido em
/// um JOIN ordinário. O argumento p é a cláusula WHERE. Se a cláusula WHERE
/// requer que alguma coluna da tabela direita do LEFT JOIN
/// seja não NULL, então o LEFT JOIN pode ser convertido com segurança em um
/// join ordinário.
pub fn expr_implies_non_null_row(p: &mut Expr, i_tab: i32, is_rj: i32) -> i32 {
    let mut w = Walker {
        p_parse: None,
        x_expr_callback: Some(implies_not_null_row),
        x_select_callback: None,
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: if is_rj != 0 { 1 } else { 0 },
        u: WalkerU::ICur(i_tab),
    };

    // expr_skip_collate_and_likely_mut: variante com &mut da função do C, que
    // devolve o próprio nó (o integrador a cria junto de expr_skip_collate_and_likely).
    let mut p = match expr_skip_collate_and_likely_mut(Some(p)) {
        Some(p) => p,
        None => return 0,
    };
    if p.op == TK_NOTNULL {
        p = p.p_left.as_deref_mut().unwrap();
    } else {
        while p.op == TK_AND {
            if expr_implies_non_null_row(p.p_left.as_deref_mut().unwrap(), i_tab, is_rj) != 0 {
                return 1;
            }
            p = p.p_right.as_deref_mut().unwrap();
        }
    }

    walk_expr_nn(&mut w, p);
    return w.e_code;
}

/// Uma instância da seguinte estrutura é usada pelo caminhador de árvore
/// para determinar se uma expressão pode ser avaliada por referência ao
/// índice apenas, sem ter que fazer uma busca pela entrada
/// de tabela correspondente. O campo IdxCover.pIdx é o índice. IdxCover.iCur
/// é o número de cursor para a tabela.
pub struct IdxCover {
    /// O índice a ser testado para cobertura.
    pub p_idx: Option<IndexRef>,
    /// Número de cursor para a tabela correspondente ao índice.
    pub i_cur: i32,
}

/// Conferir se há referências a colunas na tabela
/// pWalker->u.pIdxCover->iCur podem ser satisfeitas usando o índice
/// pWalker->u.pIdxCover->pIdx.
fn expr_idx_cover(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    if let WalkerU::IdxCover(idx_cover) = &p_walker.u {
        if p_expr.op == TK_COLUMN
            && p_expr.i_table == idx_cover.i_cur
            && table_column_to_index(idx_cover.p_idx.as_ref(), p_expr.i_column) < 0
        {
            p_walker.e_code = 1;
            return WRC_ABORT;
        }
    }
    return WRC_CONTINUE;
}

/// Determine se um índice pIdx na tabela com cursor iCur contém
/// a expressão pExpr. Devolva verdadeiro se o índice cobre a
/// expressão e falso se a expressão pExpr referencia colunas de tabela
/// que não são encontradas no índice pIdx.
///
/// Um índice cobrindo uma expressão significa que a expressão pode ser
/// avaliada usando apenas o índice e sem ter que procurar pela
/// entrada de tabela correspondente.
pub fn expr_covered_by_index(p_expr: &mut Expr, i_cur: i32, p_idx: Option<&IndexRef>) -> i32 {
    let mut w = Walker {
        p_parse: None,
        x_expr_callback: Some(expr_idx_cover),
        x_select_callback: None,
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WalkerU::IdxCover(Box::new(IdxCover {
            p_idx: p_idx.cloned(),
            i_cur,
        })),
    };

    walk_expr_nn(&mut w, p_expr);
    return if w.e_code != 0 { 0 } else { 1 };
}


// ---- part_016.rs ----

/// Estrutura usada para passar informações durante a caminhada do Walker para
/// implementar `references_src_list`.
pub struct RefSrcList {
    /// Conexão com o banco de dados (usada pelo `sqlite3DbRealloc` do C).
    pub db: Weak<RefCell<Sqlite3>>,
    /// Procura por referências a estas tabelas.
    pub p_ref: Option<Box<SrcList>>,
    /// Número de tabelas a excluir da busca.
    pub n_exclude: i64,
    /// IDs de cursor das tabelas a excluir da busca.
    pub ai_exclude: Vec<i32>,
}

/// Callback SELECT do Walker para `references_src_list`.
///
/// Ao entrar numa nova subconsulta, adiciona todas as entradas da cláusula FROM
/// daquela subconsulta à lista de exclusão.
///
/// Ao sair da subconsulta (`select_ref_leave`), remove aquelas entradas da lista.
fn select_ref_enter(p_walker: &mut Walker, p_select: &mut Select) -> i32 {
    let p = match &mut p_walker.u {
        WalkerU::RefSrcList(p) => p,
        _ => return WRC_CONTINUE,
    };
    let p_src = match &p_select.p_src {
        Some(s) => s,
        None => return WRC_CONTINUE,
    };
    if p_src.n_src == 0 {
        return WRC_CONTINUE;
    }
    let mut j = p.n_exclude as usize;
    p.n_exclude += p_src.n_src as i64;
    // `sqlite3DbRealloc` do C: o vetor passa a ter exatamente `n_exclude` entradas.
    p.ai_exclude.resize(p.n_exclude as usize, 0);
    for i in 0..p_src.n_src as usize {
        p.ai_exclude[j] = p_src.a[i].i_cursor;
        j += 1;
    }
    WRC_CONTINUE
}

fn select_ref_leave(p_walker: &mut Walker, p_select: &mut Select) {
    let p = match &mut p_walker.u {
        WalkerU::RefSrcList(p) => p,
        _ => return,
    };
    let n_src = match &p_select.p_src {
        Some(s) => s.n_src as i64,
        None => return,
    };
    if p.n_exclude != 0 {
        debug_assert!(p.n_exclude >= n_src);
        p.n_exclude -= n_src;
    }
}

/// Callback de expressão do Walker para `references_src_list`.
///
/// Liga o bit 0x01 de `e_code` se houver uma referência a qualquer uma das
/// tabelas de `RefSrcList.p_ref`.
///
/// Liga o bit 0x02 de `e_code` se houver uma referência a uma tabela que não
/// está nem em `RefSrcList.p_ref` nem em `RefSrcList.ai_exclude`.
fn expr_ref_to_src_list(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_COLUMN || p_expr.op == TK_AGG_COLUMN {
        let bits: u16 = match &p_walker.u {
            WalkerU::RefSrcList(p) => {
                let n_src = match &p.p_ref {
                    Some(s) => s.n_src as usize,
                    None => 0,
                };
                let mut hit = false;
                if let Some(p_src) = &p.p_ref {
                    for i in 0..n_src {
                        if p_expr.i_table == p_src.a[i].i_cursor {
                            hit = true;
                            break;
                        }
                    }
                }
                if hit {
                    1
                } else {
                    let mut i: i64 = 0;
                    while i < p.n_exclude && p.ai_exclude[i as usize] != p_expr.i_table {
                        i += 1;
                    }
                    if i >= p.n_exclude {
                        2
                    } else {
                        0
                    }
                }
            }
            _ => 0,
        };
        p_walker.e_code |= bits;
    }
    WRC_CONTINUE
}

/// Verifica se `p_expr` referencia alguma tabela de `p_src_list`.
/// Valores de retorno possíveis:
///
///    1         `p_expr` referencia uma tabela de `p_src_list`.
///
///    0         `p_expr` referencia alguma tabela que não está definida em
///              `p_src_list` nem em subconsultas do próprio `p_expr`.
///
///   -1         `p_expr` não referencia nenhuma tabela, ou só referencia
///              tabelas definidas em subconsultas do próprio `p_expr`.
///
/// Como é usada hoje, `p_expr` é sempre uma chamada de função de agregação.
/// Esse fato é explorado por eficiência.
pub fn references_src_list(p_parse: &ParseRef, p_expr: &Expr, p_src_list: &SrcList) -> i32 {
    let db = {
        let parse = p_parse.upgrade().expect("o Parse precisa estar vivo");
        let parse = parse.borrow();
        parse.db.clone()
    };
    let x = RefSrcList {
        db,
        p_ref: Some(Box::new(SrcList {
            n_src: p_src_list.n_src,
            n_alloc: p_src_list.n_alloc,
            a: p_src_list.a.clone(),
        })),
        n_exclude: 0,
        ai_exclude: Vec::new(),
    };
    let mut w = Walker {
        p_parse: None,
        x_expr_callback: Some(expr_ref_to_src_list),
        x_select_callback: Some(select_ref_enter),
        x_select_callback2: Some(select_ref_leave),
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WalkerU::RefSrcList(Box::new(x)),
    };
    debug_assert!(p_expr.op == TK_AGG_FUNCTION);
    debug_assert!(expr_use_x_list(p_expr));
    walk_expr_list(&mut w, p_expr.x.p_list.as_deref());
    if let Some(p_left) = &p_expr.p_left {
        debug_assert!(p_left.op == TK_ORDER);
        debug_assert!(expr_use_x_list(p_left));
        debug_assert!(p_left.x.p_list.is_some());
        walk_expr_list(&mut w, p_left.x.p_list.as_deref());
    }
    if expr_has_property(p_expr, EP_WIN_FUNC) {
        if let Some(p_win) = &p_expr.y.p_win {
            walk_expr(&mut w, p_win.borrow().p_filter.as_ref());
        }
    }
    // `ai_exclude` é liberado junto com o Walker, ao sair do escopo.
    if (w.e_code & 0x01) != 0 {
        1
    } else if w.e_code != 0 {
        0
    } else {
        -1
    }
}

/// Compara por identidade (o `==` de ponteiros do C) o nó guardado no `AggInfo`
/// com o nó que o Walker está visitando.
#[inline]
fn agg_expr_is(p_stored: &Rc<RefCell<Expr>>, p_expr: &Expr) -> bool {
    std::ptr::eq(p_stored.as_ptr() as *const Expr, p_expr as *const Expr)
}

/// Callback de nó de expressão do Walker.
///
/// Para nós Expr que contêm ponteiros `p_agg_info`, garante que o objeto AggInfo
/// referenciado não aponte diretamente para o Expr. Se apontar, faz uma cópia.
/// Isso é feito porque o argumento `p_expr` está sujeito a mudanças.
///
/// A cópia é agendada para exclusão com `expr_deferred_delete()`, que se apoia
/// no mecanismo `parser_add_cleanup()`.
fn agg_info_persist_expr_cb(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    if !expr_has_property(p_expr, EP_TOKEN_ONLY | EP_REDUCED) && p_expr.p_agg_info.is_some() {
        let p_agg_info = p_expr.p_agg_info.clone().unwrap();
        let i_agg = p_expr.i_agg;
        let p_parse = p_walker
            .p_parse
            .clone()
            .expect("o Walker de persistência de AggInfo precisa de Parse");
        let db = {
            let parse = p_parse.upgrade().expect("o Parse precisa estar vivo");
            let db = parse.borrow().db.upgrade().expect("a conexão precisa estar viva");
            db
        };
        debug_assert!(i_agg >= 0);
        let i_agg = i_agg as usize;
        if p_expr.op != TK_AGG_FUNCTION {
            let is_self = {
                let ai = p_agg_info.borrow();
                (i_agg as i32) < ai.n_column
                    && ai.a_col[i_agg]
                        .p_c_expr
                        .as_ref()
                        .map_or(false, |c| agg_expr_is(c, p_expr))
            };
            if is_self {
                if let Some(p_dup) = expr_dup(&db, p_expr, 0) {
                    if expr_deferred_delete(&p_parse, &p_dup) == 0 {
                        p_agg_info.borrow_mut().a_col[i_agg].p_c_expr = Some(p_dup);
                    }
                }
            }
        } else {
            debug_assert!(p_expr.op == TK_AGG_FUNCTION);
            let is_self = {
                let ai = p_agg_info.borrow();
                (i_agg as i32) < ai.n_func
                    && ai.a_func[i_agg]
                        .p_f_expr
                        .as_ref()
                        .map_or(false, |c| agg_expr_is(c, p_expr))
            };
            if is_self {
                if let Some(p_dup) = expr_dup(&db, p_expr, 0) {
                    if expr_deferred_delete(&p_parse, &p_dup) == 0 {
                        p_agg_info.borrow_mut().a_func[i_agg].p_f_expr = Some(p_dup);
                    }
                }
            }
        }
    }
    WRC_CONTINUE
}

/// Inicializa um objeto Walker para que persista as entradas de AggInfo
/// referenciadas pela árvore percorrida.
pub fn agg_info_persist_walker_init(p_walker: &mut Walker, p_parse: &ParseRef) {
    *p_walker = Walker {
        p_parse: Some(p_parse.clone()),
        x_expr_callback: Some(agg_info_persist_expr_cb),
        x_select_callback: Some(select_walk_noop),
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WalkerU::None,
    };
}

/// Adiciona um novo elemento ao array `a_col` de `p_info`. Retorna o índice do
/// novo elemento. Retorna um número negativo se o malloc falhar.
fn add_agg_info_column(db: &mut Sqlite3, p_info: &mut AggInfo) -> i32 {
    let mut i: i32 = 0;
    array_allocate(db, &mut p_info.a_col, &mut p_info.n_column, &mut i);
    i
}

/// Adiciona um novo elemento ao array `a_func` de `p_info`. Retorna o índice do
/// novo elemento. Retorna um número negativo se o malloc falhar.
fn add_agg_info_func(db: &mut Sqlite3, p_info: &mut AggInfo) -> i32 {
    let mut i: i32 = 0;
    array_allocate(db, &mut p_info.a_func, &mut p_info.n_func, &mut i);
    i
}

/// Procura no objeto AggInfo uma entrada de `a_col` que tenha `i_table` e
/// `i_column`. Quando existe, aponta o Expr para ela.
///
/// Se nenhuma entrada anterior for achada, cria uma nova. A nova coluna terá
/// índice `n_column - 1`.
fn find_or_create_agg_info_column(
    p_parse: &ParseRef,
    p_agg_info: &AggInfoRef,
    p_expr: &mut Expr,
) {
    debug_assert!(p_agg_info.borrow().i_first_reg == 0);
    let mut found: Option<usize> = None;
    {
        let ai = p_agg_info.borrow();
        for k in 0..ai.n_column as usize {
            let p_col = &ai.a_col[k];
            if p_col.p_c_expr.as_ref().map_or(false, |c| agg_expr_is(c, p_expr)) {
                return;
            }
            if p_col.i_table == p_expr.i_table
                && p_col.i_column as i32 == p_expr.i_column
                && p_expr.op != TK_IF_NULL_ROW
            {
                found = Some(k);
                break;
            }
        }
    }
    let k = match found {
        Some(k) => k,
        None => {
            let db = {
                let parse = p_parse.upgrade().expect("o Parse precisa estar vivo");
                let db = parse.borrow().db.upgrade().expect("a conexão precisa estar viva");
                db
            };
            let k = {
                let mut ai = p_agg_info.borrow_mut();
                add_agg_info_column(&mut db.borrow_mut(), &mut ai)
            };
            if k < 0 {
                // Falta de memória ao redimensionar.
                debug_assert!(db.borrow().malloc_failed != 0);
                return;
            }
            let k = k as usize;
            debug_assert!(expr_use_y_tab(p_expr));
            let p_c_expr = expr_node_ref(p_expr);
            let mut ai = p_agg_info.borrow_mut();
            {
                let p_col = &mut ai.a_col[k];
                p_col.p_tab = p_expr.y.p_tab.clone();
                p_col.i_table = p_expr.i_table;
                p_col.i_column = p_expr.i_column as i16;
                p_col.i_sorter_column = -1;
                p_col.p_c_expr = Some(p_c_expr);
            }
            if p_expr.op != TK_IF_NULL_ROW {
                if let Some(p_gb) = ai.p_group_by.clone() {
                    let p_gb = p_gb.borrow();
                    let n = p_gb.n_expr as usize;
                    for j in 0..n {
                        if let Some(p_e) = &p_gb.a[j].p_expr {
                            if p_e.op == TK_COLUMN
                                && p_e.i_table == p_expr.i_table
                                && p_e.i_column == p_expr.i_column
                            {
                                ai.a_col[k].i_sorter_column = j as i16;
                                break;
                            }
                        }
                    }
                }
            }
            if ai.a_col[k].i_sorter_column < 0 {
                ai.a_col[k].i_sorter_column = ai.n_sorting_column as i16;
                ai.n_sorting_column += 1;
            }
            k
        }
    };
    // Rótulo `fix_up_expr` do C. O `ExprSetVVAProperty` só existe sob SQLITE_DEBUG.
    debug_assert!(match &p_expr.p_agg_info {
        None => true,
        Some(a) => Rc::ptr_eq(a, p_agg_info),
    });
    p_expr.p_agg_info = Some(p_agg_info.clone());
    if p_expr.op == TK_COLUMN {
        p_expr.op = TK_AGG_COLUMN;
    }
    p_expr.i_agg = k as i16;
}

/// Este é o `x_expr_callback` de um Walker. É usado para implementar
/// `expr_analyze_aggregates`. Veja `expr_analyze_aggregates` para mais informações.
fn analyze_aggregate(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    let (p_parse, src_cursors, nc_flags, p_agg_info) = match &p_walker.u {
        WalkerU::Nc(nc) => {
            let p_agg_info = match &nc.u_nc {
                NameContextUNC::AggInfo(a) => a.clone(),
                _ => return WRC_CONTINUE,
            };
            let src_cursors: Option<Vec<i32>> = nc
                .p_src_list
                .as_ref()
                .map(|s| s.a[..s.n_src as usize].iter().map(|it| it.i_cursor).collect());
            (
                nc.p_parse.clone().expect("o NameContext precisa de Parse"),
                src_cursors,
                nc.nc_flags,
                p_agg_info,
            )
        }
        _ => return WRC_CONTINUE,
    };
    let p_parse_rc = p_parse.upgrade().expect("o Parse precisa estar vivo");
    debug_assert!((nc_flags & NC_UAGGINFO) != 0);
    debug_assert!(p_agg_info.borrow().i_first_reg == 0);
    match p_expr.op {
        TK_IF_NULL_ROW | TK_AGG_COLUMN | TK_COLUMN => {
            // Verifica se a coluna está numa das tabelas da cláusula FROM da
            // consulta de agregação.
            if let Some(cursors) = &src_cursors {
                for &i_cursor in cursors.iter() {
                    debug_assert!(!expr_has_property(p_expr, EP_TOKEN_ONLY | EP_REDUCED));
                    if p_expr.i_table == i_cursor {
                        find_or_create_agg_info_column(&p_parse, &p_agg_info, p_expr);
                        break;
                    }
                }
            }
            WRC_CONTINUE
        }
        TK_AGG_FUNCTION => {
            if (nc_flags & NC_INAGGFUNC) == 0
                && p_walker.walker_depth == p_expr.op2 as i32
                && p_expr.p_agg_info.is_none()
            {
                // Verifica se `p_expr` duplica outra função de agregação que já está
                // na estrutura `p_agg_info`.
                let n_func = p_agg_info.borrow().n_func;
                let mut i: i32 = 0;
                while i < n_func {
                    let p_f_expr = p_agg_info.borrow().a_func[i as usize].p_f_expr.clone();
                    if let Some(p_f) = &p_f_expr {
                        if agg_expr_is(p_f, p_expr) {
                            break;
                        }
                        if expr_compare(None, &p_f.borrow(), p_expr, -1) == 0 {
                            break;
                        }
                    }
                    i += 1;
                }
                if i >= n_func {
                    // `p_expr` é original. Cria uma nova entrada em `a_func`.
                    let db = p_parse_rc
                        .borrow()
                        .db
                        .upgrade()
                        .expect("a conexão precisa estar viva");
                    let enc_v: u8 = enc(&db.borrow());
                    i = {
                        let mut ai = p_agg_info.borrow_mut();
                        add_agg_info_func(&mut db.borrow_mut(), &mut ai)
                    };
                    if i >= 0 {
                        let iu = i as usize;
                        debug_assert!(!expr_has_property(p_expr, EP_X_IS_SELECT));
                        debug_assert!(expr_use_u_token(p_expr));
                        let n_arg: i32 = p_expr.x.p_list.as_ref().map_or(0, |l| l.n_expr);
                        let p_func = find_function(&mut db.borrow_mut(), &p_expr.u.z_token, n_arg, enc_v, false);
                        let func_flags: u32 = p_func.as_ref().map_or(0, |f| f.borrow().func_flags);
                        let p_f_expr = expr_node_ref(p_expr);
                        let mut ai = p_agg_info.borrow_mut();
                        ai.a_func[iu].p_f_expr = Some(p_f_expr);
                        ai.a_func[iu].p_func = p_func;
                        debug_assert!(ai.a_func[iu].b_ob_unique == 0);
                        if let (Some(p_left), true) =
                            (&p_expr.p_left, (func_flags & SQLITE_FUNC_NEEDCOLL) == 0)
                        {
                            // O teste de NEEDCOLL faz com que qualquer ORDER BY nas
                            // agregações min() e max() seja ignorado.
                            debug_assert!(n_arg > 0);
                            debug_assert!(p_left.op == TK_ORDER);
                            debug_assert!(expr_use_x_list(p_left));
                            {
                                let mut parse = p_parse_rc.borrow_mut();
                                ai.a_func[iu].i_ob_tab = parse.n_tab;
                                parse.n_tab += 1;
                            }
                            let p_ob_list = p_left.x.p_list.as_ref().expect("lista do ORDER BY");
                            debug_assert!(p_ob_list.n_expr > 0);
                            debug_assert!(ai.a_func[iu].b_ob_unique == 0);
                            let same_as_arg = p_ob_list.n_expr == 1
                                && n_arg == 1
                                && match (
                                    &p_ob_list.a[0].p_expr,
                                    &p_expr.x.p_list.as_ref().expect("argumentos").a[0].p_expr,
                                ) {
                                    (Some(a), Some(b)) => expr_compare(None, a, b, 0) == 0,
                                    _ => false,
                                };
                            if same_as_arg {
                                ai.a_func[iu].b_ob_payload = 0;
                                ai.a_func[iu].b_ob_unique =
                                    expr_has_property(p_expr, EP_DISTINCT) as u8;
                            } else {
                                ai.a_func[iu].b_ob_payload = 1;
                            }
                            ai.a_func[iu].b_use_subtype =
                                ((func_flags & (SQLITE_SUBTYPE as u32)) != 0) as u8;
                        } else {
                            ai.a_func[iu].i_ob_tab = -1;
                        }
                        if expr_has_property(p_expr, EP_DISTINCT) && ai.a_func[iu].b_ob_unique == 0 {
                            let mut parse = p_parse_rc.borrow_mut();
                            ai.a_func[iu].i_distinct = parse.n_tab;
                            parse.n_tab += 1;
                        } else {
                            ai.a_func[iu].i_distinct = -1;
                        }
                    }
                }
                // Faz `p_expr` apontar para a entrada correta de `a_func`.
                debug_assert!(!expr_has_property(p_expr, EP_TOKEN_ONLY | EP_REDUCED));
                p_expr.i_agg = i as i16;
                p_expr.p_agg_info = Some(p_agg_info.clone());
                WRC_PRUNE
            } else {
                WRC_CONTINUE
            }
        }
        _ => {
            debug_assert!(p_parse_rc.borrow().i_self_tab == 0);
            if (nc_flags & NC_INAGGFUNC) == 0 {
                return WRC_CONTINUE;
            }
            // Procura, em `p_idx_expr`, uma expressão de índice igual a `p_expr`.
            let hit: Option<(i32, i32, i32)> = {
                let parse = p_parse_rc.borrow();
                let mut cur = parse.p_idx_expr.as_deref();
                let mut hit = None;
                while let Some(p_ie) = cur {
                    let i_data_cur = p_ie.i_data_cur;
                    if i_data_cur >= 0
                        && expr_compare(None, p_expr, &p_ie.p_expr, i_data_cur) == 0
                    {
                        hit = Some((i_data_cur, p_ie.i_idx_cur, p_ie.i_idx_col));
                        break;
                    }
                    cur = p_ie.p_ie_next.as_deref();
                }
                hit
            };
            let (i_data_cur, i_idx_cur, i_idx_col) = match hit {
                Some(h) => h,
                None => return WRC_CONTINUE,
            };
            if !expr_use_y_tab(p_expr) {
                return WRC_CONTINUE;
            }
            let cursors = src_cursors.as_deref().unwrap_or(&[]);
            // O C compara `a[0]` em todas as voltas do laço (comportamento mantido).
            let mut i = 0usize;
            while i < cursors.len() {
                if cursors[0] == i_data_cur {
                    break;
                }
                i += 1;
            }
            if i >= cursors.len() {
                return WRC_CONTINUE;
            }
            if p_expr.p_agg_info.is_some() {
                return WRC_CONTINUE; // Resolvido por um contexto externo
            }
            if p_parse_rc.borrow().n_err != 0 {
                return WRC_ABORT;
            }
            // Se chegou aqui, a expressão `p_expr` pode ser traduzida numa
            // referência a uma coluna de índice, como descrito por `p_ie`.
            let mut tmp = Expr::default();
            tmp.op = TK_AGG_COLUMN;
            tmp.i_table = i_idx_cur;
            tmp.i_column = i_idx_col;
            find_or_create_agg_info_column(&p_parse, &p_agg_info, &mut tmp);
            if p_parse_rc.borrow().n_err != 0 {
                return WRC_ABORT;
            }
            debug_assert!(!p_agg_info.borrow().a_col.is_empty());
            debug_assert!((tmp.i_agg as i32) < p_agg_info.borrow().n_column);
            p_agg_info.borrow_mut().a_col[tmp.i_agg as usize].p_c_expr = Some(expr_node_ref(p_expr));
            p_expr.p_agg_info = Some(p_agg_info.clone());
            p_expr.i_agg = tmp.i_agg;
            WRC_PRUNE
        }
    }
}


// ---- part_017.rs ----

/// Analisa a expressão `p_expr` procurando por funções de agregação e por variáveis
/// que precisam ser adicionadas ao objeto `AggInfo` ao qual `p_nc->u_nc.p_agg_info` aponta.
/// Entradas extras são feitas no objeto `AggInfo` conforme necessário.
///
/// Esta rotina só deve ser chamada depois que a expressão foi analisada por
/// `resolve_expr_names()`.
pub fn expr_analyze_aggregates(p_nc: &mut NameContext, p_expr: Option<&mut Expr>) {
    debug_assert!(p_nc.p_src_list.is_some());
    // O `w.u.pNC = pNC` do C compartilha o ponteiro; como `WalkerU::Nc` é dono (Box), o
    // contexto real é movido para o Walker durante a caminhada (o callback altera n_ref,
    // n_nc_err e o AggInfo por ele) e devolvido ao chamador depois. No lugar fica um
    // contexto vazio, que ninguém lê enquanto o Walker está com o original.
    let vazio = NameContext {
        p_parse: None,
        p_src_list: None,
        u_nc: NameContextUNC::IBaseReg(0),
        p_next: None,
        n_ref: 0,
        n_nc_err: 0,
        nc_flags: 0,
        n_nested_select: 0,
        p_win_select: None,
    };
    let original = std::mem::replace(p_nc, vazio);
    let mut w = Walker {
        p_parse: None,
        x_expr_callback: Some(analyze_aggregate),
        x_select_callback: Some(walker_depth_increase),
        x_select_callback2: Some(walker_depth_decrease),
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WalkerU::Nc(Box::new(original)),
    };
    walk_expr(&mut w, p_expr);
    if let WalkerU::Nc(nc) = w.u {
        *p_nc = *nc;
    }
}

/// Chama `expr_analyze_aggregates()` para cada expressão de uma lista de expressões.
/// Retorna o número de erros.
///
/// Se um erro for encontrado, a análise é cortada.
pub fn expr_analyze_agg_list(p_nc: &mut NameContext, p_list: Option<&mut ExprList>) {
    if let Some(list) = p_list {
        for i in 0..list.n_expr {
            if i < list.a.len() as i32 {
                if let Some(p_expr) = &mut list.a[i as usize].p_expr {
                    expr_analyze_aggregates(p_nc, Some(p_expr.as_mut()));
                }
            }
        }
    }
}

/// Aloca um único registro novo para uso no armazenamento de algum resultado intermediário.
pub fn get_temp_reg(p_parse: &mut Parse) -> i32 {
    if p_parse.n_temp_reg == 0 {
        p_parse.n_mem += 1;
        p_parse.n_mem
    } else {
        let reg = p_parse.a_temp_reg[p_parse.n_temp_reg as usize - 1];
        p_parse.n_temp_reg -= 1;
        reg
    }
}

/// Desaloca um registro, deixando-o disponível para reutilização para outro propósito.
pub fn release_temp_reg(p_parse: &mut Parse, i_reg: i32) {
    if i_reg != 0 {
        vdbe_release_registers(p_parse, i_reg, 1, 0, 0);
        if (p_parse.n_temp_reg as usize) < p_parse.a_temp_reg.len() {
            p_parse.a_temp_reg[p_parse.n_temp_reg as usize] = i_reg;
            p_parse.n_temp_reg += 1;
        }
    }
}

/// Aloca ou desaloca um bloco de `n_reg` registros consecutivos.
pub fn get_temp_range(p_parse: &mut Parse, n_reg: i32) -> i32 {
    if n_reg == 1 {
        return get_temp_reg(p_parse);
    }
    let mut i = p_parse.i_range_reg;
    let n = p_parse.n_range_reg;
    if n_reg <= n {
        p_parse.i_range_reg += n_reg;
        p_parse.n_range_reg -= n_reg;
    } else {
        i = p_parse.n_mem + 1;
        p_parse.n_mem += n_reg;
    }
    i
}

pub fn release_temp_range(p_parse: &mut Parse, i_reg: i32, n_reg: i32) {
    if n_reg == 1 {
        release_temp_reg(p_parse, i_reg);
        return;
    }
    vdbe_release_registers(p_parse, i_reg, n_reg, 0, 0);
    if n_reg > p_parse.n_range_reg {
        p_parse.n_range_reg = n_reg;
        p_parse.i_range_reg = i_reg;
    }
}

/// Marca todos os registros temporários como indisponíveis para reutilização.
///
/// Sempre invoque este procedimento depois de codificar uma sub-rotina ou corrotina
/// que pode ser invocada de outras partes do código, para garantir que a
/// sub-rotina/corrotina não use registros em comum com o código que a invoca.
pub fn clear_temp_reg_cache(p_parse: &mut Parse) {
    p_parse.n_temp_reg = 0;
    p_parse.n_range_reg = 0;
}

/// Garante que registros suficientes foram alocados de forma que `i_reg` seja um
/// número de registro válido.
pub fn touch_register(p_parse: &mut Parse, i_reg: i32) {
    if p_parse.n_mem < i_reg {
        p_parse.n_mem = i_reg;
    }
}

/// Retorna o registro mais recente reutilizável no conjunto de todos os registros.
/// O valor retornado não é menor que `i_min`. Se algum registro `i_min` ou maior
/// está em uso permanente, então retorna um a mais que o último registro permanente.
pub fn first_available_register(p_parse: &mut Parse, mut i_min: i32) -> i32 {
    if let Some(p_list) = &p_parse.p_const_expr {
        for i in 0..p_list.n_expr {
            if i < p_list.a.len() as i32 {
                if let ExprListItemU::IConstExprReg(reg) = p_list.a[i as usize].u {
                    if reg >= i_min {
                        i_min = reg + 1;
                    }
                }
            }
        }
    }
    p_parse.n_temp_reg = 0;
    p_parse.n_range_reg = 0;
    i_min
}

// `sqlite3NoTempsInRange` só existe sob SQLITE_DEBUG (usada em assert), então some do porte.

