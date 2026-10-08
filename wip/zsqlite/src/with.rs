//! As CTEs e a cláusula WITH que o analisador monta: `sqlite3CteNew`, `sqlite3WithAdd` e a forma
//! que a gramática usa de `sqlite3WithPush` (`build.c` e `select.c`).
//!
//! Desvios do C, todos decorrentes do modelo v2 (CONVENTIONS.md):
//!
//! - `sqlite3CteDelete`, `cteClear`, `sqlite3WithDelete` e `sqlite3WithDeleteGeneric` não
//!   existem: o que o C libera à mão é o `Drop` de `Cte` e de `With`;
//! - `sqlite3WithDup` já vive em `expr.rs` (é parte da cópia profunda do `Select`); aqui só é
//!   reexportada, para que quem procura a família `With*` encontre tudo num módulo;
//! - `sqlite3WithPush` tem uma única tradução, a de `select2.rs` (protocolo "mover para dentro e
//!   devolver o anterior", que o resolvedor de nomes e o `ALTER TABLE` usam). [`with_push`] é a
//!   chamada do analisador (`with ::= WITH wqlist`): o `bFree` do C, que passava a posse ao
//!   `Parse` para liberar no fim da análise, é o próprio movimento da cláusula para
//!   `Parse.p_with`; o `pOuter` do C é a pilha de WITH do `Parse`, e o analisador só empilha a
//!   cláusula de nível mais externo, então não há anterior a preservar;
//! - a falha de alocação não existe, então os ramos `mallocFailed` somem.

use crate::connection::{Connection, Parse};
use crate::sqlite_int::{Cte, ExprList, Select, Token, With};
use crate::build::{name_from_token, text_arg};
use crate::select2::with_push as push_on_parse;
use crate::util::{error_msg, str_icmp};

pub use crate::expr::with_dup;

/// `sqlite3CteNew`: cria uma CTE nova. `p_name` é o nome da tabela comum, `p_arglist` a lista
/// opcional de nomes de coluna, `p_query` o SELECT que a inicializa e `e_m10d` a flag
/// MATERIALIZED.
pub fn cte_new(
    _db: &mut Connection,
    _parse: &mut Parse,
    p_name: &Token,
    p_arglist: Option<Box<ExprList>>,
    p_query: Option<Box<Select>>,
    e_m10d: u8,
) -> Option<Box<Cte>> {
    Some(Box::new(Cte {
        p_select: p_query,
        p_cols: p_arglist,
        z_name: name_from_token(Some(p_name)),
        e_m10d,
        ..Cte::default()
    }))
}

/// `sqlite3WithAdd`: o analisador a chama uma vez por CTE ao ler uma cláusula WITH. A CTE `p_cte`
/// entra na cláusula `p_with`; se `p_with` é `None`, cria-se um `With` novo.
pub fn with_add(
    db: &mut Connection,
    parse: &mut Parse,
    p_with: Option<Box<With>>,
    p_cte: Option<Box<Cte>>,
) -> Option<Box<With>> {
    let Some(p_cte) = p_cte else {
        return p_with;
    };

    // Confere que o nome da CTE é único dentro desta cláusula WITH. Se não, grava o erro no
    // `Parse`.
    if let (Some(z_name), Some(w)) = (p_cte.z_name.as_deref(), p_with.as_deref()) {
        for c in w.a.iter() {
            if c.z_name.as_deref().map_or(false, |z| str_icmp(z_name, z) == 0) {
                error_msg(db, parse, b"duplicate WITH table name: %s", &[text_arg(z_name)]);
            }
        }
    }

    let mut p_new = p_with.unwrap_or_default();
    p_new.a.push(*p_cte);
    Some(p_new)
}

/// `sqlite3WithPush(pParse, pWith, bFree)` como o analisador a chama: a cláusula WITH passa a ser
/// a do topo da pilha do `Parse`, que a possui até o fim da análise. Sem erro de análise a
/// cláusula entra em `parse.p_with`; com erro (ou sem cláusula) nada é empilhado e ela é
/// liberada aqui.
pub fn with_push(_db: &mut Connection, parse: &mut Parse, p_with: Option<Box<With>>, _b_free: i32) {
    // O anterior (`pOuter` do C) é sempre `None` na gramática: um comando só tem uma cláusula
    // WITH de nível mais externo. Uma cláusula devolvida em `Err` é descartada (o `bFree`).
    let _ = push_on_parse(parse, p_with);
}
