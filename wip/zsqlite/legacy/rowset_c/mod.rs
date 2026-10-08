// Mesclado das partes traduzidas de rowset_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Tamanho objetivo para blocos de alocação.
pub const ROWSET_ALLOCATION_SIZE: usize = 1024;

/// Tamanho em bytes de `struct RowSetEntry` no C (i64 mais dois ponteiros de 8 bytes). Fixo para
/// que `ROWSET_ENTRY_PER_CHUNK` valha 42 como no Debian de 64 bits.
pub const ROWSET_ENTRY_SIZE: usize = 24;

/// Número de entradas de RowSet por bloco de alocação.
pub const ROWSET_ENTRY_PER_CHUNK: usize = (ROWSET_ALLOCATION_SIZE - 8) / ROWSET_ENTRY_SIZE;

/// Cada entrada num RowSet é uma instância do seguinte objeto.
///
/// Este mesmo objeto é reutilizado para guardar uma lista encadeada de árvores de objetos
/// RowSetEntry. Nesse uso alternativo, p_right aponta para a próxima entrada da lista, p_left
/// aponta para a árvore, e v não é usado. O valor RowSet.p_forest aponta para a cabeça desta
/// lista de floresta.
///
/// Os ponteiros do C viram índices na arena `RowSet.a_entry` (os blocos de alocação do C).
#[derive(Clone, Copy, Default)]
pub struct RowSetEntry {
    /// Valor de ROWID para esta entrada.
    pub v: i64,
    /// Subárvore direita (entradas maiores) ou lista.
    pub p_right: Option<usize>,
    /// Subárvore esquerda (entradas menores).
    pub p_left: Option<usize>,
}

/// Um RowSet é uma instância da seguinte estrutura.
///
/// A lista de `RowSetChunk` do C (`p_chunk`) é a própria arena `a_entry`: cada bloco de
/// `ROWSET_ENTRY_PER_CHUNK` entradas é acrescentado ao final do vetor e todos são liberados de
/// uma vez em `row_set_clear`.
pub struct RowSet {
    /// A conexão do banco de dados.
    pub db: std::rc::Weak<std::cell::RefCell<Sqlite3>>,
    /// Arena com todas as entradas alocadas (blocos de alocação).
    pub a_entry: Vec<RowSetEntry>,
    /// Lista de entradas usando p_right.
    pub p_entry: Option<usize>,
    /// Última entrada na lista p_entry.
    pub p_last: Option<usize>,
    /// Fonte de novos objetos de entrada (índice da próxima entrada livre).
    pub p_fresh: usize,
    /// Lista de árvores binárias de entradas.
    pub p_forest: Option<usize>,
    /// Número de objetos em p_fresh.
    pub n_fresh: u16,
    /// Vários sinalizadores.
    pub rs_flags: u16,
    /// Lote de inserção atual.
    pub i_batch: i32,
}

/// Valores permitidos para RowSet.rs_flags.
pub const ROWSET_SORTED: u16 = 0x01; // verdadeiro se RowSet.p_entry está ordenado
pub const ROWSET_NEXT: u16 = 0x02; // verdadeiro se row_set_next() foi chamado

/// Aloca um objeto RowSet. No C retorna NULL se a alocação falha; aqui a alocação não falha.
///
/// O C aproveita a sobra do tamanho real do bloco do malloc para pré-povoar `n_fresh`; isso não é
/// observável (só muda quando a memória é pedida), então a arena começa vazia com `n_fresh` zero.
pub fn row_set_init(db: &Sqlite3Ref) -> Option<Box<RowSet>> {
    Some(Box::new(RowSet {
        db: std::rc::Rc::downgrade(db),
        a_entry: Vec::new(),
        p_entry: None,
        p_last: None,
        p_fresh: 0,
        p_forest: None,
        n_fresh: 0,
        rs_flags: ROWSET_SORTED,
        i_batch: 0,
    }))
}

/// Desaloca todos os blocos de um RowSet. Isto libera toda a memória que o RowSet alocou ao
/// longo de sua vida. Esta rotina é o destrutor para o RowSet.
pub fn row_set_clear(p: &mut RowSet) {
    p.a_entry = Vec::new();
    p.p_fresh = 0;
    p.n_fresh = 0;
    p.p_entry = None;
    p.p_last = None;
    p.p_forest = None;
    p.rs_flags = ROWSET_SORTED;
}

/// Desaloca todos os blocos de um RowSet e o próprio RowSet. Esta rotina é o destrutor para o
/// RowSet.
pub fn row_set_delete(mut p: Box<RowSet>) {
    row_set_clear(&mut p);
    drop(p);
}

/// Aloca um novo objeto RowSetEntry associado ao RowSet dado. Retorna o índice do novo objeto,
/// completamente não inicializado.
///
/// No C, numa situação de OOM o sinalizador db->mallocFailed é definido e a rotina retorna NULL.
/// Aqui a alocação de um bloco não falha, mas o chamador continua tratando o `None`.
fn row_set_entry_alloc(p: &mut RowSet) -> Option<usize> {
    if p.n_fresh == 0 {
        // Poderíamos alocar um RowSetEntry novo a cada vez que fosse preciso, mas é mais
        // eficiente tirar uma entrada pré-alocada do conjunto.
        p.p_fresh = p.a_entry.len();
        p.a_entry
            .resize(p.p_fresh + ROWSET_ENTRY_PER_CHUNK, RowSetEntry::default());
        p.n_fresh = ROWSET_ENTRY_PER_CHUNK as u16;
    }
    p.n_fresh -= 1;
    let idx = p.p_fresh;
    p.p_fresh += 1;
    Some(idx)
}

/// Insere um novo valor num RowSet.
///
/// O sinalizador mallocFailed da conexão do banco de dados é definido se uma alocação de memória
/// falha.
pub fn row_set_insert(p: &mut RowSet, rowid: i64) {
    // Esta rotina nunca é chamada depois de row_set_next()
    assert!((p.rs_flags & ROWSET_NEXT) == 0);

    let p_entry = match row_set_entry_alloc(p) {
        Some(e) => e,
        None => return,
    };
    p.a_entry[p_entry].v = rowid;
    p.a_entry[p_entry].p_right = None;
    match p.p_last {
        Some(p_last) => {
            if rowid <= p.a_entry[p_last].v {
                // Evita ordenações desnecessárias preservando o sinalizador ROWSET_SORTED onde
                // possível.
                p.rs_flags &= !ROWSET_SORTED;
            }
            p.a_entry[p_last].p_right = Some(p_entry);
        }
        None => {
            p.p_entry = Some(p_entry);
        }
    }
    p.p_last = Some(p_entry);
}

/// Mescla duas listas de objetos RowSetEntry. Remove duplicatas.
///
/// As listas de entrada são conectadas via p_right e assume-se que cada uma já está ordenada.
/// O `head` do C (entrada local na pilha) vira a variável `head`; `tail` vale `None` enquanto
/// ainda aponta para ele.
fn row_set_entry_merge(a: &mut [RowSetEntry], mut p_a: usize, mut p_b: usize) -> Option<usize> {
    let mut head: Option<usize> = None;
    let mut tail: Option<usize> = None;

    fn set_right(
        a: &mut [RowSetEntry],
        head: &mut Option<usize>,
        tail: Option<usize>,
        x: Option<usize>,
    ) {
        match tail {
            None => *head = x,
            Some(t) => a[t].p_right = x,
        }
    }

    loop {
        debug_assert!(a[p_a].p_right.map_or(true, |r| a[p_a].v <= a[r].v));
        debug_assert!(a[p_b].p_right.map_or(true, |r| a[p_b].v <= a[r].v));
        if a[p_a].v <= a[p_b].v {
            if a[p_a].v < a[p_b].v {
                set_right(a, &mut head, tail, Some(p_a));
                tail = Some(p_a);
            }
            match a[p_a].p_right {
                Some(next) => p_a = next,
                None => {
                    set_right(a, &mut head, tail, Some(p_b));
                    break;
                }
            }
        } else {
            set_right(a, &mut head, tail, Some(p_b));
            tail = Some(p_b);
            match a[p_b].p_right {
                Some(next) => p_b = next,
                None => {
                    set_right(a, &mut head, tail, Some(p_a));
                    break;
                }
            }
        }
    }
    head
}

/// Ordena todos os elementos da lista de objetos RowSetEntry em ordem de v crescente.
fn row_set_entry_sort(a: &mut [RowSetEntry], mut p_in: Option<usize>) -> Option<usize> {
    let mut a_bucket: [Option<usize>; 40] = [None; 40];

    while let Some(entry) = p_in {
        let p_next = a[entry].p_right;
        a[entry].p_right = None;
        let mut cur = entry;
        let mut i = 0usize;
        while let Some(b) = a_bucket[i] {
            cur = row_set_entry_merge(a, b, cur).unwrap();
            a_bucket[i] = None;
            i += 1;
        }
        a_bucket[i] = Some(cur);
        p_in = p_next;
    }
    let mut result = a_bucket[0];
    for i in 1..a_bucket.len() {
        let b = match a_bucket[i] {
            Some(b) => b,
            None => continue,
        };
        result = match result {
            Some(r) => row_set_entry_merge(a, r, b),
            None => Some(b),
        };
    }
    result
}

/// A entrada, p_in, é uma árvore binária (ou subárvore) de objetos RowSetEntry. Converte esta
/// árvore numa lista encadeada conectada pelos ponteiros p_right e retorna o primeiro e o
/// último elementos da nova lista, nessa ordem.
fn row_set_tree_to_list(a: &mut [RowSetEntry], p_in: usize) -> (usize, usize) {
    let first;
    match a[p_in].p_left {
        Some(left) => {
            let (f, p) = row_set_tree_to_list(a, left);
            a[p].p_right = Some(p_in);
            first = f;
        }
        None => {
            first = p_in;
        }
    }
    let last;
    match a[p_in].p_right {
        Some(right) => {
            // No C: rowSetTreeToList(pIn->pRight, &pIn->pRight, ppLast)
            let (f, l) = row_set_tree_to_list(a, right);
            a[p_in].p_right = Some(f);
            last = l;
        }
        None => {
            last = p_in;
        }
    }
    debug_assert!(a[last].p_right.is_none());
    (first, last)
}

/// Converte uma lista ordenada de elementos (conectados por p_right) numa árvore binária com
/// profundidade i_depth. Uma profundidade de 1 significa que a árvore contém um único nó tirado
/// da cabeça de *pp_list. Uma profundidade de 2 significa uma árvore com três nós. E assim por
/// diante.
///
/// Usa quantas entradas da lista de entrada forem necessárias e atualiza *pp_list para apontar
/// para os elementos não usados da lista. Se a lista de entrada tem poucos elementos, constrói
/// uma árvore incompleta e deixa *pp_list como NULL.
///
/// Retorna a raiz da árvore binária construída.
fn row_set_n_deep_tree(
    a: &mut [RowSetEntry],
    pp_list: &mut Option<usize>,
    i_depth: i32,
) -> Option<usize> {
    if pp_list.is_none() {
        // Evita recursão profunda desnecessária quando as entradas acabam.
        return None;
    }
    let p;
    if i_depth > 1 {
        // Este ramo faz gerar uma árvore *balanceada*. Uma árvore válida ainda é gerada sem ele,
        // mas ela fica extremamente desbalanceada e ineficiente.
        let p_left = row_set_n_deep_tree(a, pp_list, i_depth - 1);
        p = match *pp_list {
            Some(p) => p,
            None => {
                // É seguro sempre retornar aqui, mas a árvore resultante ficaria desbalanceada.
                return p_left;
            }
        };
        a[p].p_left = p_left;
        *pp_list = a[p].p_right;
        a[p].p_right = row_set_n_deep_tree(a, pp_list, i_depth - 1);
    } else {
        p = pp_list.unwrap();
        *pp_list = a[p].p_right;
        a[p].p_left = None;
        a[p].p_right = None;
    }
    Some(p)
}


// ---- part_001.rs ----

/// Converte uma lista ordenada de elementos numa árvore binária. A árvore fica tão profunda
/// quanto for preciso para conter a lista inteira.
fn row_set_list_to_tree(a: &mut [RowSetEntry], p_list: usize) -> usize {
    let mut list: Option<usize> = a[p_list].p_right; // resto da lista ainda não consumido
    let mut p = p_list; // raiz atual da árvore
    a[p].p_left = None;
    a[p].p_right = None;
    let mut i_depth: i32 = 1;
    while let Some(next) = list {
        let p_left = p;
        p = next;
        list = a[p].p_right;
        a[p].p_left = Some(p_left);
        let right = row_set_n_deep_tree(a, &mut list, i_depth);
        a[p].p_right = right;
        i_depth += 1;
    }
    p
}

/// Extrai o menor elemento do RowSet. Escreve o elemento em *p_rowid. Retorna 1 em caso de
/// sucesso. Retorna 0 se o RowSet já está vazio.
///
/// Depois que esta rotina é chamada, row_set_insert() não pode mais ser chamada.
///
/// Esta rotina não pode ser chamada depois que row_set_test() foi usada. Versões antigas do
/// RowSet permitiam isso, mas o gerador de código não usava a capacidade e ela foi removida.
pub fn row_set_next(p: &mut RowSet, p_rowid: &mut i64) -> i32 {
    assert!(p.p_forest.is_none()); // não pode ser usada junto com row_set_test()

    // Mescla a floresta numa única lista ordenada na primeira chamada
    if (p.rs_flags & ROWSET_NEXT) == 0 {
        if (p.rs_flags & ROWSET_SORTED) == 0 {
            p.p_entry = row_set_entry_sort(&mut p.a_entry, p.p_entry);
        }
        p.rs_flags |= ROWSET_SORTED | ROWSET_NEXT;
    }

    // Retorna a próxima entrada da lista
    match p.p_entry {
        Some(e) => {
            *p_rowid = p.a_entry[e].v;
            p.p_entry = p.a_entry[e].p_right;
            if p.p_entry.is_none() {
                // Libera a memória imediatamente, em vez de esperar o finalize
                row_set_clear(p);
            }
            1
        }
        None => 0,
    }
}

/// Verifica se o elemento i_rowid foi inserido no rowset como parte de algum lote de inserção
/// anterior a i_batch. Retorna 1 ou 0.
///
/// Se este é o primeiro teste de um novo lote e existem entradas em p_row_set.p_entry, essas
/// entradas são ordenadas para dentro da floresta em p_row_set.p_forest para poderem ser
/// testadas.
pub fn row_set_test(p_row_set: &mut RowSet, i_batch: i32, i_rowid: i64) -> i32 {
    // Esta rotina nunca é chamada depois de row_set_next()
    assert!((p_row_set.rs_flags & ROWSET_NEXT) == 0);

    // Ordena as entradas para dentro da floresta no primeiro teste de um novo lote. Para poupar
    // trabalho, só faz isso quando o número do lote muda.
    if i_batch != p_row_set.i_batch {
        if let Some(p0) = p_row_set.p_entry {
            let mut p = p0;
            // pp_prev_tree do C: None representa &p_row_set.p_forest, Some(t) representa &t.p_right
            let mut pp_prev_tree: Option<usize> = None;
            if (p_row_set.rs_flags & ROWSET_SORTED) == 0 {
                // Só ordena o conjunto atual de entradas se precisar
                p = row_set_entry_sort(&mut p_row_set.a_entry, Some(p)).unwrap();
            }
            let mut p_tree = p_row_set.p_forest;
            while let Some(t) = p_tree {
                pp_prev_tree = Some(t);
                if p_row_set.a_entry[t].p_left.is_none() {
                    let tree = row_set_list_to_tree(&mut p_row_set.a_entry, p);
                    p_row_set.a_entry[t].p_left = Some(tree);
                    break;
                } else {
                    let left = p_row_set.a_entry[t].p_left.unwrap();
                    let (p_aux, _p_tail) = row_set_tree_to_list(&mut p_row_set.a_entry, left);
                    p_row_set.a_entry[t].p_left = None;
                    p = row_set_entry_merge(&mut p_row_set.a_entry, p_aux, p).unwrap();
                }
                p_tree = p_row_set.a_entry[t].p_right;
            }
            if p_tree.is_none() {
                p_tree = row_set_entry_alloc(p_row_set);
                match pp_prev_tree {
                    None => p_row_set.p_forest = p_tree,
                    Some(prev) => p_row_set.a_entry[prev].p_right = p_tree,
                }
                if let Some(t) = p_tree {
                    p_row_set.a_entry[t].v = 0;
                    p_row_set.a_entry[t].p_right = None;
                    let tree = row_set_list_to_tree(&mut p_row_set.a_entry, p);
                    p_row_set.a_entry[t].p_left = Some(tree);
                }
            }
            p_row_set.p_entry = None;
            p_row_set.p_last = None;
            p_row_set.rs_flags |= ROWSET_SORTED;
        }
        p_row_set.i_batch = i_batch;
    }

    // Verifica se o valor i_rowid aparece em algum ponto da floresta. Retorna 1 se aparece e 0
    // se não.
    let mut p_tree = p_row_set.p_forest;
    while let Some(t) = p_tree {
        let mut p = p_row_set.a_entry[t].p_left;
        while let Some(n) = p {
            let v = p_row_set.a_entry[n].v;
            if v < i_rowid {
                p = p_row_set.a_entry[n].p_right;
            } else if v > i_rowid {
                p = p_row_set.a_entry[n].p_left;
            } else {
                return 1;
            }
        }
        p_tree = p_row_set.a_entry[t].p_right;
    }
    0
}

