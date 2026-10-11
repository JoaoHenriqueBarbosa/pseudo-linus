//! Conjunto de rowids (rowset.c do SQLite 3.46.1).
//!
//! O `RowSet` é uma coleção de rowids inseridos em ordem arbitrária. Inserções podem se
//! intercalar com testes de pertinência (por lote) e, no fim, os elementos saem em ordem
//! crescente. Depois que a extração começa (`row_set_next`), não se insere mais. `TEST` e
//! `NEXT` não se misturam no mesmo conjunto.
//!
//! O C aloca as entradas em blocos e liga tudo por ponteiros (`pRight`/`pLeft`). Aqui as
//! entradas moram num `Vec` (arena) e os ponteiros são índices; os blocos e `pFresh`/`nFresh`
//! só existiam para economizar `malloc` e somem. A semântica dos algoritmos (lista, floresta
//! de árvores, ordenação por mescla em 40 baldes) é a mesma.

/// `ROWSET_SORTED`: a lista `p_entry` está ordenada.
const ROWSET_SORTED: u16 = 0x01;
/// `ROWSET_NEXT`: `row_set_next` já foi chamado.
const ROWSET_NEXT: u16 = 0x02;

/// `struct RowSetEntry`. A mesma estrutura serve de nó de lista da floresta: `p_right` aponta a
/// próxima árvore, `p_left` aponta a árvore e `v` não é usado.
#[derive(Clone, Copy)]
struct RowSetEntry {
    /// Valor do rowid desta entrada.
    v: i64,
    /// Subárvore direita (maiores) ou próximo da lista.
    p_right: Option<usize>,
    /// Subárvore esquerda (menores).
    p_left: Option<usize>,
}

pub struct RowSet {
    /// Arena de todas as entradas vivas (substitui os blocos `RowSetChunk`).
    entries: Vec<RowSetEntry>,
    /// Lista de entradas ligada por `p_right`.
    p_entry: Option<usize>,
    /// Última entrada da lista `p_entry`.
    p_last: Option<usize>,
    /// Lista de árvores binárias de entradas.
    p_forest: Option<usize>,
    /// Flags (`ROWSET_*`).
    rs_flags: u16,
    /// Lote de inserção atual.
    i_batch: i32,
}

/// `sqlite3RowSetInit`: cria um conjunto vazio.
pub fn row_set_init() -> RowSet {
    RowSet {
        entries: Vec::new(),
        p_entry: None,
        p_last: None,
        p_forest: None,
        rs_flags: ROWSET_SORTED,
        i_batch: 0,
    }
}

/// `sqlite3RowSetClear`: libera todas as entradas. O `iBatch` não muda.
pub fn row_set_clear(p: &mut RowSet) {
    p.entries = Vec::new();
    p.p_entry = None;
    p.p_last = None;
    p.p_forest = None;
    p.rs_flags = ROWSET_SORTED;
}

/// `sqlite3RowSetDelete`: o destrutor (limpa e libera o próprio objeto).
pub fn row_set_delete(mut p: RowSet) {
    row_set_clear(&mut p);
}

/// `rowSetEntryAlloc`: nova entrada na arena.
fn row_set_entry_alloc(p: &mut RowSet) -> usize {
    p.entries.push(RowSetEntry { v: 0, p_right: None, p_left: None });
    p.entries.len() - 1
}

/// `sqlite3RowSetInsert`: insere um rowid. Nunca se chama depois de `row_set_next`.
pub fn row_set_insert(p: &mut RowSet, rowid: i64) {
    debug_assert!((p.rs_flags & ROWSET_NEXT) == 0);
    let entry = row_set_entry_alloc(p);
    p.entries[entry].v = rowid;
    p.entries[entry].p_right = None;
    match p.p_last {
        Some(last) => {
            if rowid <= p.entries[last].v {
                // Preserva ROWSET_SORTED sempre que possível, para evitar ordenações.
                p.rs_flags &= !ROWSET_SORTED;
            }
            p.entries[last].p_right = Some(entry);
        }
        None => p.p_entry = Some(entry),
    }
    p.p_last = Some(entry);
}

/// Liga `x` depois de `tail`; sem `tail`, `x` vira a cabeça (`head.pRight` do C).
fn link_after(entries: &mut [RowSetEntry], head: &mut Option<usize>, tail: Option<usize>, x: usize) {
    match tail {
        Some(t) => entries[t].p_right = Some(x),
        None => *head = Some(x),
    }
}

/// `rowSetEntryMerge`: mescla duas listas ordenadas (ligadas por `p_right`), removendo
/// duplicatas. Ambas não vazias.
fn row_set_entry_merge(entries: &mut [RowSetEntry], mut a: usize, mut b: usize) -> usize {
    let mut head: Option<usize> = None;
    let mut tail: Option<usize> = None;
    loop {
        if entries[a].v <= entries[b].v {
            if entries[a].v < entries[b].v {
                link_after(entries, &mut head, tail, a);
                tail = Some(a);
            }
            match entries[a].p_right {
                Some(next) => a = next,
                None => {
                    link_after(entries, &mut head, tail, b);
                    break;
                }
            }
        } else {
            link_after(entries, &mut head, tail, b);
            tail = Some(b);
            match entries[b].p_right {
                Some(next) => b = next,
                None => {
                    link_after(entries, &mut head, tail, a);
                    break;
                }
            }
        }
    }
    head.expect("mescla de duas listas não vazias")
}

/// `rowSetEntrySort`: ordena a lista em ordem crescente de `v` (mescla em baldes).
fn row_set_entry_sort(entries: &mut [RowSetEntry], p_in: Option<usize>) -> Option<usize> {
    let mut p_in = p_in;
    let mut a_bucket: [Option<usize>; 40] = [None; 40];
    while let Some(cur) = p_in {
        let p_next = entries[cur].p_right;
        entries[cur].p_right = None;
        let mut merged = cur;
        let mut i = 0;
        while let Some(b) = a_bucket[i] {
            merged = row_set_entry_merge(entries, b, merged);
            a_bucket[i] = None;
            i += 1;
        }
        a_bucket[i] = Some(merged);
        p_in = p_next;
    }
    let mut p_in = a_bucket[0];
    for b in a_bucket.iter().skip(1) {
        let Some(b) = *b else { continue };
        p_in = Some(match p_in {
            Some(cur) => row_set_entry_merge(entries, cur, b),
            None => b,
        });
    }
    p_in
}

/// `rowSetTreeToList`: converte uma árvore binária numa lista ligada por `p_right` e devolve
/// `(primeiro, último)`.
fn row_set_tree_to_list(entries: &mut [RowSetEntry], p_in: usize) -> (usize, usize) {
    let first = match entries[p_in].p_left {
        Some(left) => {
            let (first, last) = row_set_tree_to_list(entries, left);
            entries[last].p_right = Some(p_in);
            first
        }
        None => p_in,
    };
    let last = match entries[p_in].p_right {
        Some(right) => {
            let (sub_first, last) = row_set_tree_to_list(entries, right);
            entries[p_in].p_right = Some(sub_first);
            last
        }
        None => p_in,
    };
    debug_assert!(entries[last].p_right.is_none());
    (first, last)
}

/// `rowSetNDeepTree`: converte uma lista ordenada numa árvore binária de profundidade `depth`
/// (1 é um nó só). Consome da cabeça de `list` e deixa o resto em `list`; se a lista acabar,
/// a árvore sai incompleta e `list` fica `None`.
fn row_set_n_deep_tree(entries: &mut [RowSetEntry], list: &mut Option<usize>, depth: i32) -> Option<usize> {
    // Evita recursão profunda desnecessária quando as entradas acabam.
    let mut p = (*list)?;
    if depth > 1 {
        // Este ramo gera uma árvore balanceada.
        let p_left = row_set_n_deep_tree(entries, list, depth - 1);
        p = match *list {
            Some(x) => x,
            None => return p_left,
        };
        entries[p].p_left = p_left;
        *list = entries[p].p_right;
        entries[p].p_right = row_set_n_deep_tree(entries, list, depth - 1);
    } else {
        *list = entries[p].p_right;
        entries[p].p_left = None;
        entries[p].p_right = None;
    }
    Some(p)
}

/// `rowSetListToTree`: converte uma lista ordenada (não vazia) numa árvore binária com a
/// profundidade necessária para conter tudo.
fn row_set_list_to_tree(entries: &mut [RowSetEntry], p_list: usize) -> usize {
    let mut p = p_list;
    let mut list = entries[p].p_right;
    entries[p].p_left = None;
    entries[p].p_right = None;
    let mut depth = 1;
    while let Some(next) = list {
        let p_left = p;
        p = next;
        list = entries[p].p_right;
        entries[p].p_left = Some(p_left);
        entries[p].p_right = row_set_n_deep_tree(entries, &mut list, depth);
        depth += 1;
    }
    p
}

/// `sqlite3RowSetNext`: extrai o menor elemento. Devolve `None` se o conjunto já está vazio.
/// Depois da primeira chamada não se pode mais inserir; também não se pode usar depois de
/// `row_set_test`.
pub fn row_set_next(p: &mut RowSet) -> Option<i64> {
    debug_assert!(p.p_forest.is_none());

    // Na primeira chamada, junta tudo numa só lista ordenada.
    if (p.rs_flags & ROWSET_NEXT) == 0 {
        if (p.rs_flags & ROWSET_SORTED) == 0 {
            p.p_entry = row_set_entry_sort(&mut p.entries, p.p_entry);
        }
        p.rs_flags |= ROWSET_SORTED | ROWSET_NEXT;
    }

    // Devolve a próxima entrada da lista.
    let entry = p.p_entry?;
    let rowid = p.entries[entry].v;
    p.p_entry = p.entries[entry].p_right;
    if p.p_entry.is_none() {
        // Libera a memória já, sem esperar o finalize.
        row_set_clear(p);
    }
    Some(rowid)
}

/// Onde ligar a árvore nova da floresta (o `ppPrevTree` do C).
enum PrevTree {
    /// `&pRowSet->pForest`.
    Forest,
    /// `&pTree->pRight` do nó dado.
    Node(usize),
}

/// `sqlite3RowSetTest`: o rowid foi inserido em algum lote anterior a `i_batch`?
///
/// Na primeira chamada de um lote novo, as entradas de `p_entry` são ordenadas e passadas
/// para a floresta, para poderem ser testadas.
pub fn row_set_test(row_set: &mut RowSet, i_batch: i32, i_rowid: i64) -> bool {
    // Nunca se chama depois de row_set_next.
    debug_assert!((row_set.rs_flags & ROWSET_NEXT) == 0);

    // Passa as entradas para a floresta na primeira busca de um lote novo.
    if i_batch != row_set.i_batch {
        if let Some(first) = row_set.p_entry {
            let mut pp_prev_tree = PrevTree::Forest;
            let mut p = first;
            if (row_set.rs_flags & ROWSET_SORTED) == 0 {
                // Só ordena o conjunto atual se precisar.
                p = row_set_entry_sort(&mut row_set.entries, Some(p)).expect("lista não vazia");
            }
            let mut p_tree = row_set.p_forest;
            while let Some(tree) = p_tree {
                pp_prev_tree = PrevTree::Node(tree);
                if row_set.entries[tree].p_left.is_none() {
                    let sub = row_set_list_to_tree(&mut row_set.entries, p);
                    row_set.entries[tree].p_left = Some(sub);
                    break;
                } else {
                    let left = row_set.entries[tree].p_left.expect("subárvore presente");
                    let (aux, _tail) = row_set_tree_to_list(&mut row_set.entries, left);
                    row_set.entries[tree].p_left = None;
                    p = row_set_entry_merge(&mut row_set.entries, aux, p);
                }
                p_tree = row_set.entries[tree].p_right;
            }
            if p_tree.is_none() {
                let tree = row_set_entry_alloc(row_set);
                match pp_prev_tree {
                    PrevTree::Forest => row_set.p_forest = Some(tree),
                    PrevTree::Node(n) => row_set.entries[n].p_right = Some(tree),
                }
                row_set.entries[tree].v = 0;
                row_set.entries[tree].p_right = None;
                let sub = row_set_list_to_tree(&mut row_set.entries, p);
                row_set.entries[tree].p_left = Some(sub);
            }
            row_set.p_entry = None;
            row_set.p_last = None;
            row_set.rs_flags |= ROWSET_SORTED;
        }
        row_set.i_batch = i_batch;
    }

    // Procura o rowid em todas as árvores da floresta.
    let mut p_tree = row_set.p_forest;
    while let Some(tree) = p_tree {
        let mut p = row_set.entries[tree].p_left;
        while let Some(node) = p {
            let e = &row_set.entries[node];
            if e.v < i_rowid {
                p = e.p_right;
            } else if e.v > i_rowid {
                p = e.p_left;
            } else {
                return true;
            }
        }
        p_tree = row_set.entries[tree].p_right;
    }
    false
}
