// Mesclado das partes traduzidas de btree_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Cabeçalho que aparece no início de todo banco de dados SQLite.
pub const Z_MAGIC_HEADER: &[u8] = SQLITE_FILE_HEADER;

/// Valores passados como quinto argumento de `allocate_btree_page()`.
/// Aloca qualquer página.
pub const BTALLOC_ANY: u8 = 0;
/// Aloca a página exata, se possível.
pub const BTALLOC_EXACT: u8 = 1;
/// Aloca qualquer página menor ou igual ao parâmetro.
pub const BTALLOC_LE: u8 = 2;

/// Extrai um inteiro big-endian de 2 bytes de `x` na posição `off`. Se o valor
/// for zero, devolve 65536.
///
/// Usada para extrair o "deslocamento da área de conteúdo das células" do
/// cabeçalho de uma página b-tree. Se a página tem 65536 bytes e está vazia, o
/// deslocamento deveria ser 65536, mas os 2 bytes guardam zero. Esta rotina faz
/// o ajuste necessário.
#[inline]
pub fn get2byte_not_zero(x: &[u8], off: usize) -> i32 {
    (((get2byte(x, off) as i32) - 1) & 0xffff) + 1
}

/// Equivalente da macro `IfNotOmitAV(expr)`. O Debian 13 não define
/// `SQLITE_OMIT_AUTOVACUUM`, então devolve a expressão como está.
#[inline]
pub fn if_not_omit_av<T>(expr: T) -> T {
    expr
}

thread_local! {
    /// Lista de objetos `BtShared` elegíveis para participar do cache
    /// compartilhado (`sqlite3SharedCacheList`). Em C o acesso é protegido por
    /// `SQLITE_MUTEX_STATIC_MAIN`; aqui cada thread tem a sua cabeça de lista, e
    /// o encadeamento segue por `BtShared.p_next`.
    pub static SHARED_CACHE_LIST: std::cell::RefCell<Option<BtSharedRef>> = const { std::cell::RefCell::new(None) };
}

/// Liga ou desliga os recursos de pager e esquema compartilhados.
///
/// Não tem efeito sobre conexões já existentes. A configuração afeta somente
/// futuras chamadas de `sqlite3_open()`, `sqlite3_open16()` e `sqlite3_open_v2()`.
pub fn enable_shared_cache(enable: i32) -> i32 {
    with_global_config(|config| config.shared_cache_enabled = enable);
    SQLITE_OK
}

/// Equivalente de `SQLITE_CORRUPT_PAGE(pMemPage)` numa compilação sem
/// `SQLITE_DEBUG`: o mesmo que `SQLITE_CORRUPT_PGNO(pMemPage->pgno)`.
#[inline]
pub fn sqlite_corrupt_page(p: &MemPage) -> i32 {
    sqlite_corrupt_pgno(p.pgno)
}

/// Pergunta se o handle Btree `p` pode obter um bloqueio do tipo `e_lock`
/// (`READ_LOCK` ou `WRITE_LOCK`) na tabela com página raiz `i_tab`. Devolve
/// `SQLITE_OK` se o bloqueio pode ser obtido (chamando
/// `set_shared_cache_table_lock()`), ou `SQLITE_LOCKED_SHAREDCACHE` se não.
pub fn query_shared_cache_table_lock(p: &BtreeRef, i_tab: u32, e_lock: u8) -> i32 {
    let (p_bt, sharable, db) = {
        let b = p.borrow();
        (b.p_bt.clone(), b.sharable, b.db.clone())
    };

    // Esta rotina não faz nada se o cache compartilhado não está ativo.
    if sharable == 0 {
        return SQLITE_OK;
    }

    // Se outra conexão mantém um bloqueio exclusivo, o bloqueio pedido não pode
    // ser obtido.
    let (writer, bts_flags, locks) = {
        let bt = p_bt.borrow();
        let writer = bt.p_writer.as_ref().and_then(|w| w.upgrade());
        (writer, bt.bts_flags, bt.p_lock.clone())
    };
    let is_writer = match &writer {
        Some(w) => std::rc::Rc::ptr_eq(w, p),
        None => false,
    };
    if !is_writer && (bts_flags & BTS_EXCLUSIVE) != 0 {
        if let Some(w) = &writer {
            let writer_db = w.borrow().db.clone();
            connection_blocked(&db, &writer_db);
        }
        return SQLITE_LOCKED_SHAREDCACHE;
    }

    let me = std::rc::Rc::downgrade(p);
    for p_iter in locks.iter() {
        // A condição (pIter->eLock!=eLock) do `if` abaixo é uma simplificação de
        // (eLock==WRITE_LOCK || pIter->eLock==WRITE_LOCK), pois se
        // eLock==WRITE_LOCK nenhuma outra conexão pode manter um WRITE_LOCK em
        // qualquer tabela deste arquivo (só existe um escritor).
        if !p_iter.p_btree.ptr_eq(&me) && p_iter.i_table == i_tab && p_iter.e_lock != e_lock {
            if let Some(other) = p_iter.p_btree.upgrade() {
                let other_db = other.borrow().db.clone();
                connection_blocked(&db, &other_db);
            }
            if e_lock == WRITE_LOCK {
                p_bt.borrow_mut().bts_flags |= BTS_PENDING;
            }
            return SQLITE_LOCKED_SHAREDCACHE;
        }
    }
    SQLITE_OK
}


// ---- part_001.rs ----

/// Adiciona um bloqueio na tabela com página raiz `i_table` ao btree
/// compartilhado usado pelo handle Btree `p`. `e_lock` deve ser `READ_LOCK` ou
/// `WRITE_LOCK`.
///
/// Pressupõe que (a) `p` está ligado a um banco compartilhável e (b) nenhum
/// outro Btree mantém um bloqueio conflitante (isto é,
/// `query_shared_cache_table_lock()` já devolveu `SQLITE_OK`).
///
/// O `BtLock` embutido `Btree.lock` do C (usado para a tabela 1) é uma entrada
/// comum do `Vec` aqui. Não existe o caminho de `SQLITE_NOMEM`, pois a
/// alocação em Rust não falha de forma recuperável.
pub fn set_shared_cache_table_lock(p: &BtreeRef, i_table: u32, e_lock: u8) -> i32 {
    let p_bt = p.borrow().p_bt.clone();
    let mut bt = p_bt.borrow_mut();
    let me = std::rc::Rc::downgrade(p);

    // Primeiro procura na lista um bloqueio existente nesta tabela.
    let found = bt
        .p_lock
        .iter()
        .position(|l| l.i_table == i_table && l.p_btree.ptr_eq(&me));

    // Se a busca não achou um BtLock associando o Btree p à tabela i_table,
    // cria um e liga no início da lista.
    let idx = match found {
        Some(i) => i,
        None => {
            bt.p_lock.insert(
                0,
                BtLock {
                    p_btree: me,
                    i_table,
                    e_lock: 0,
                },
            );
            0
        }
    };

    // Deixa BtLock.e_lock no máximo entre o bloqueio atual e o pedido. Assim,
    // se um bloqueio de escrita já era mantido e um de leitura é pedido, o
    // bloqueio não é rebaixado por engano.
    if e_lock > bt.p_lock[idx].e_lock {
        bt.p_lock[idx].e_lock = e_lock;
    }

    SQLITE_OK
}

/// Libera todos os bloqueios de tabela (obtidos por `set_shared_cache_table_lock()`)
/// mantidos pelo objeto Btree `p`.
///
/// Pressupõe que `p` tem uma transação de leitura ou escrita aberta. Se não
/// tiver, o flag `BTS_PENDING` pode ser limpo indevidamente.
pub fn clear_all_shared_cache_table_locks(p: &BtreeRef) {
    let p_bt = p.borrow().p_bt.clone();
    let mut bt = p_bt.borrow_mut();
    let me = std::rc::Rc::downgrade(p);

    bt.p_lock.retain(|l| !l.p_btree.ptr_eq(&me));

    let is_writer = match &bt.p_writer {
        Some(w) => w.ptr_eq(&me),
        None => false,
    };
    if is_writer {
        bt.p_writer = None;
        bt.bts_flags &= !(BTS_EXCLUSIVE | BTS_PENDING);
    } else if bt.n_transaction == 2 {
        // Esta função é chamada quando o Btree p está concluindo sua transação.
        // Se existe um escritor e p não é ele, o número de bloqueios mantidos
        // por conexões que não são o escritor está para chegar a zero. Então
        // zera BTS_PENDING.
        //
        // Se não existe escritor, BTS_PENDING já é zero e a linha é inofensiva.
        bt.bts_flags &= !BTS_PENDING;
    }
}

/// Converte em bloqueios de leitura todos os bloqueios de escrita mantidos pelo
/// Btree `p`.
pub fn downgrade_all_shared_cache_table_locks(p: &BtreeRef) {
    let p_bt = p.borrow().p_bt.clone();
    let mut bt = p_bt.borrow_mut();
    let me = std::rc::Rc::downgrade(p);

    let is_writer = match &bt.p_writer {
        Some(w) => w.ptr_eq(&me),
        None => false,
    };
    if is_writer {
        bt.p_writer = None;
        bt.bts_flags &= !(BTS_EXCLUSIVE | BTS_PENDING);
        for p_lock in bt.p_lock.iter_mut() {
            p_lock.e_lock = READ_LOCK;
        }
    }
}

/// Invalida o cache de overflow do cursor passado.
#[inline]
pub fn invalidate_overflow_cache(p_cur: &mut BtCursor) {
    p_cur.cur_flags &= !BTCF_VALIDOVFL;
}

/// Invalida o cache da lista de páginas de overflow de todos os cursores
/// abertos na estrutura btree compartilhada `p_bt`.
pub fn invalidate_all_overflow_cache(p_bt: &BtShared) {
    let mut p = p_bt.p_cursor.clone();
    while let Some(c) = p {
        let mut b = c.borrow_mut();
        invalidate_overflow_cache(&mut b);
        let next = b.p_next.clone();
        drop(b);
        p = next;
    }
}

/// Chamada antes de modificar o conteúdo de uma tabela, para invalidar os
/// cursores incrblob abertos na linha (ou numa das linhas) que será modificada.
///
/// Se `is_clear_table` é verdadeiro, o conteúdo inteiro da tabela será apagado:
/// invalida todo cursor incrblob aberto em qualquer linha da tabela com página
/// raiz `pgno_root`. Senão, a linha com rowid `i_row` está sendo substituída ou
/// apagada, e só são invalidados os cursores incrblob abertos nessa linha.
pub fn invalidate_incrblob_cursors(
    p_btree: &BtreeRef,
    pgno_root: u32,
    i_row: i64,
    is_clear_table: i32,
) {
    let p_bt = p_btree.borrow().p_bt.clone();
    let mut has_incrblob_cur = 0;
    let mut p = p_bt.borrow().p_cursor.clone();
    while let Some(c) = p {
        let mut b = c.borrow_mut();
        if (b.cur_flags & BTCF_INCRBLOB) != 0 {
            has_incrblob_cur = 1;
            if b.pgno_root == pgno_root && (is_clear_table != 0 || b.info.n_key == i_row) {
                b.e_state = CURSOR_INVALID;
            }
        }
        let next = b.p_next.clone();
        drop(b);
        p = next;
    }
    p_btree.borrow_mut().has_incrblob_cur = has_incrblob_cur;
}

/// Liga o bit `pgno` do bitvec `BtShared.p_has_content`. Chamada quando uma
/// página que tinha dados vira folha da lista livre.
///
/// O bitvec existe para contornar um bug obscuro causado pela interação de duas
/// otimizações de E/S sobre páginas folha da lista livre:
///
///   1) Quando todos os dados de uma página são apagados e ela vira folha da
///      lista livre, a página não é escrita no banco (folhas livres não têm
///      conteúdo útil). Às vezes nem é gravada no journal.
///
///   2) Quando uma folha livre é reaproveitada, seu conteúdo não é lido do banco
///      nem escrito no journal.
///
/// Sozinhas funcionam bem. Mas se uma página vai para a lista livre e é
/// reaproveitada na mesma transação sem ter ido ao journal em nenhum dos dois
/// momentos, o dado original pode se perder, e num rollback talvez não seja
/// possível restaurar o banco à configuração original.
///
/// A solução é o bitvec: cada página que vira folha livre tem seu bit ligado, e
/// ao extrair uma folha da lista livre a otimização 2 é omitida se o bit já
/// estiver ligado. O conteúdo do bitvec é limpo ao fim de toda transação.
pub fn btree_set_has_content(p_bt: &mut BtShared, pgno: u32) -> i32 {
    let mut rc = SQLITE_OK;
    if p_bt.p_has_content.is_none() {
        p_bt.p_has_content = Some(bitvec_create(p_bt.n_page));
    }
    if let Some(bv) = p_bt.p_has_content.as_mut() {
        if pgno <= bitvec_size(bv) {
            rc = bitvec_set(bv, pgno);
        }
    }
    rc
}

/// Consulta o vetor `BtShared.p_has_content`.
///
/// Chamada quando uma folha da lista livre é removida para reuso. Devolve falso
/// se é seguro obter a página do pager com o flag 'no-content'. Verdadeiro caso
/// contrário.
pub fn btree_get_has_content(p_bt: &BtShared, pgno: u32) -> i32 {
    match &p_bt.p_has_content {
        Some(p) => (pgno > bitvec_size(p) || bitvec_test_not_null(p, pgno) != 0) as i32,
        None => 0,
    }
}

/// Limpa (destrói) o bitvec `BtShared.p_has_content`. Deve ser chamada ao fim de
/// toda transação de escrita.
pub fn btree_clear_has_content(p_bt: &mut BtShared) {
    p_bt.p_has_content = None;
}

/// Libera todas as páginas de `ap_page[]` de um cursor.
pub fn btree_release_all_cursor_pages(p_cur: &mut BtCursor) {
    if p_cur.i_page >= 0 {
        let n = p_cur.i_page as usize;
        let ap_page = std::mem::take(&mut p_cur.ap_page);
        for page in ap_page.into_iter().take(n) {
            release_page_not_null(page);
        }
        if let Some(page) = p_cur.p_page.take() {
            release_page_not_null(page);
        }
        p_cur.i_page = -1;
    }
}

/// O cursor passado deve apontar para uma entrada válida (`e_state ==
/// CURSOR_VALID`). Salva a chave atual nas variáveis `n_key` e `p_key`. Devolve
/// `SQLITE_OK` ou um código de erro do SQLite.
///
/// Se o cursor é de uma tabela intkey, o rowid vai para `n_key` e `p_key` fica
/// vazio. Se não é intkey, `p_key` recebe um buffer com os `n_key` bytes da chave.
/// (Em C `pKey` NULL equivale a `Vec` vazio: o buffer de índice sempre tem os 17
/// bytes de enchimento, então nunca é vazio.)
pub fn save_cursor_key(p_cur: &mut BtCursor) -> i32 {
    let mut rc = SQLITE_OK;

    if p_cur.cur_int_key != 0 {
        // Para uma tabela btree só o rowid é necessário.
        p_cur.n_key = btree_integer_key(p_cur);
    } else {
        // Para um índice btree, salva o conteúdo completo da chave. A chave
        // atual pode estar corrompida; nesse caso, ao restaurar a posição,
        // `vdbe_record_unpack()` pode ler além do buffer em até 1 varint mais um
        // valor de 8 bytes. Daí os 17 bytes de enchimento alocados abaixo.
        let n_key = btree_payload_size(p_cur) as i64;
        p_cur.n_key = n_key;
        let mut p_key = vec![0u8; n_key as usize + 9 + 8];
        rc = btree_payload(p_cur, 0, n_key as u32, &mut p_key[..n_key as usize]);
        if rc == SQLITE_OK {
            p_cur.p_key = p_key;
        }
    }
    rc
}


// ---- part_002.rs ----

/// Salva a posição atual do cursor nas variáveis `BtCursor.n_key` e
/// `BtCursor.p_key`. O estado do cursor passa a `CURSOR_REQUIRESEEK`.
///
/// O chamador deve garantir que o cursor é válido (`e_state == CURSOR_VALID`)
/// antes de chamar esta rotina.
pub fn save_cursor_position(p_cur: &mut BtCursor) -> i32 {
    if (p_cur.cur_flags & BTCF_PINNED) != 0 {
        return SQLITE_CONSTRAINT_PINNED;
    }
    if p_cur.e_state == CURSOR_SKIPNEXT {
        p_cur.e_state = CURSOR_VALID;
    } else {
        p_cur.skip_next = 0;
    }

    let rc = save_cursor_key(p_cur);
    if rc == SQLITE_OK {
        btree_release_all_cursor_pages(p_cur);
        p_cur.e_state = CURSOR_REQUIRESEEK;
    }

    p_cur.cur_flags &= !(BTCF_VALIDNKEY | BTCF_VALIDOVFL | BTCF_ATLAST);
    rc
}

/// Salva as posições de todos os cursores (exceto `p_except`) abertos na tabela
/// com página raiz `i_root`. "Salvar a posição" é lembrar o local na btree de
/// modo que seja possível voltar ao mesmo ponto depois que a btree for
/// modificada. Chamada logo antes de `p_except` ser usado para modificar a
/// tabela, por exemplo em `btree_delete()` ou `btree_insert()`.
///
/// Se há dois ou mais cursores na mesma btree, todos devem ter o flag
/// `BTCF_MULTIPLE` ligado (`btree_cursor()` impõe a regra). Esta rotina só
/// precisa ser chamada no caso incomum em que `p_except` tem `BTCF_MULTIPLE`.
///
/// Se `p_except` existe e nenhum outro cursor está na mesma página raiz, o flag
/// `BTCF_MULTIPLE` de `p_except` é limpo, para evitar outra chamada inútil.
///
/// Nota de implementação: esta rotina só confere se algum cursor precisa ser
/// salvo. Chama `save_cursors_on_list()` no caso (incomum) em que algum precisa.
///
/// `p_except` chega como `&mut BtCursor` (o chamador já o tem emprestado); a
/// identidade na lista se confere pelo endereço do `RefCell`, sem emprestá-lo.
pub fn save_all_cursors(p_bt: &BtShared, i_root: u32, mut p_except: Option<&mut BtCursor>) -> i32 {
    let except_ptr: *const BtCursor = match p_except.as_deref() {
        Some(e) => e as *const BtCursor,
        None => std::ptr::null(),
    };
    let mut p = p_bt.p_cursor.clone();
    while let Some(c) = p {
        if std::ptr::eq(c.as_ptr() as *const BtCursor, except_ptr) {
            p = p_except.as_deref().and_then(|e| e.p_next.clone());
            continue;
        }
        let (matches, next) = {
            let b = c.borrow();
            (i_root == 0 || b.pgno_root == i_root, b.p_next.clone())
        };
        if matches {
            return save_cursors_on_list(c, i_root, p_except);
        }
        p = next;
    }
    if let Some(e) = p_except.as_deref_mut() {
        e.cur_flags &= !BTCF_MULTIPLE;
    }
    SQLITE_OK
}

/// Rotina auxiliar de `save_all_cursors()` que faz o trabalho de salvar os
/// cursores quando se encontra algum que realmente precisa. O caso comum é
/// nenhum cursor precisar ser salvo, por isso esta rotina fica separada do
/// chamador, para evitar movimento desnecessário do ponteiro de pilha.
///
/// `p` é o primeiro cursor que precisa ser salvo; `i_root`, se não for zero,
/// restringe aos cursores dessa raiz; `p_except` não é salvo.
pub fn save_cursors_on_list(p: BtCursorRef, i_root: u32, p_except: Option<&mut BtCursor>) -> i32 {
    let except_ptr: *const BtCursor = match p_except.as_deref() {
        Some(e) => e as *const BtCursor,
        None => std::ptr::null(),
    };
    let mut cur = Some(p);
    while let Some(c) = cur {
        if std::ptr::eq(c.as_ptr() as *const BtCursor, except_ptr) {
            cur = p_except.as_deref().and_then(|e| e.p_next.clone());
            continue;
        }
        let mut b = c.borrow_mut();
        if i_root == 0 || b.pgno_root == i_root {
            if b.e_state == CURSOR_VALID || b.e_state == CURSOR_SKIPNEXT {
                let rc = save_cursor_position(&mut b);
                if rc != SQLITE_OK {
                    return rc;
                }
            } else {
                btree_release_all_cursor_pages(&mut b);
            }
        }
        let next = b.p_next.clone();
        drop(b);
        cur = next;
    }
    SQLITE_OK
}

/// Limpa a posição atual do cursor.
pub fn btree_clear_cursor(p_cur: &mut BtCursor) {
    p_cur.p_key = Vec::new();
    p_cur.e_state = CURSOR_INVALID;
}

/// Nesta versão de `btree_moveto`, `p_key` é um registro de índice empacotado,
/// como o gerado pelo opcode `OP_MakeRecord`. Desempacota o registro e chama
/// `btree_index_moveto()` para fazer o trabalho.
pub fn btree_moveto(
    p_cur: &mut BtCursor,
    p_key: Option<&[u8]>,
    n_key: i64,
    bias: i32,
    p_res: &mut i32,
) -> i32 {
    match p_key {
        Some(key) => {
            let p_key_info = match p_cur.p_key_info.clone() {
                Some(k) => k,
                None => return SQLITE_CORRUPT_BKPT,
            };
            let mut p_idx_key = vdbe_alloc_unpacked_record(&p_key_info);
            vdbe_record_unpack(&p_key_info, n_key as i32, key, &mut p_idx_key);
            if p_idx_key.n_field == 0 || p_idx_key.n_field > p_key_info.n_all_field {
                SQLITE_CORRUPT_BKPT
            } else {
                btree_index_moveto(p_cur, &mut p_idx_key, p_res)
            }
        }
        None => btree_table_moveto(p_cur, n_key, bias, p_res),
    }
}

/// Restaura o cursor à posição em que estava (ou à mais próxima possível)
/// quando `save_cursor_position()` foi chamada. Esta chamada apaga a informação
/// de posição salva, então só pode haver uma chamada efetiva de
/// `restore_cursor_position()` após cada `save_cursor_position()`.
pub fn btree_restore_cursor_position(p_cur: &mut BtCursor) -> i32 {
    let mut skip_next = 0;
    if p_cur.e_state == CURSOR_FAULT {
        return p_cur.skip_next;
    }
    p_cur.e_state = CURSOR_INVALID;

    // A chave sai do cursor durante a busca (o C passa `pCur->pKey` e o libera
    // depois); se der erro, ela volta para o cursor.
    let key = std::mem::take(&mut p_cur.p_key);
    let n_key = p_cur.n_key;
    let rc = if fault_sim(410) != 0 {
        SQLITE_IOERR
    } else {
        let key_arg = if key.is_empty() {
            None
        } else {
            Some(&key[..n_key as usize])
        };
        btree_moveto(p_cur, key_arg, n_key, 0, &mut skip_next)
    };
    if rc == SQLITE_OK {
        if skip_next != 0 {
            p_cur.skip_next = skip_next;
        }
        if p_cur.skip_next != 0 && p_cur.e_state == CURSOR_VALID {
            p_cur.e_state = CURSOR_SKIPNEXT;
        }
    } else {
        p_cur.p_key = key;
    }
    rc
}

/// Equivalente da macro `restoreCursorPosition(p)`: restaura a posição do
/// cursor se necessário.
#[inline]
pub fn restore_cursor_position(p: &mut BtCursor) -> i32 {
    if p.e_state >= CURSOR_REQUIRESEEK {
        btree_restore_cursor_position(p)
    } else {
        SQLITE_OK
    }
}

/// Determina se um cursor se moveu da posição onde foi colocado pela última vez,
/// ou foi invalidado por outro motivo. Cursores podem se mover quando a linha
/// apontada é apagada, por exemplo, ou quando a btree é rebalanceada.
///
/// Use `btree_cursor_restore()` para devolver o cursor ao lugar certo quando
/// esta rotina devolver verdadeiro.
pub fn btree_cursor_has_moved(p_cur: &BtCursor) -> i32 {
    (p_cur.e_state != CURSOR_VALID) as i32
}

/// Devolve um objeto BtCursor falso que sempre responde falso para
/// `btree_cursor_has_moved()`. O cursor devolvido não pode ser usado com
/// nenhuma outra interface Btree. (Em C é um `u8` estático com `CURSOR_VALID`
/// lido pelo primeiro byte da estrutura; aqui é um cursor com `e_state` válido.)
pub fn btree_fake_valid_cursor() -> BtCursor {
    BtCursor {
        e_state: CURSOR_VALID,
        ..BtCursor::default()
    }
}

/// Restaura um cursor à posição original depois que alguma atividade externa o
/// moveu (rebalanceamento da btree, ou linha apagada por baixo do cursor).
///
/// Em caso de sucesso, `*p_different_row` é falso se o cursor continua apontando
/// exatamente para a mesma linha; é verdadeiro se a linha apontada foi apagada,
/// forçando o cursor a apontar para uma linha próxima.
///
/// Só deve ser chamada para um cursor que acabou de devolver verdadeiro em
/// `btree_cursor_has_moved()`.
pub fn btree_cursor_restore(p_cur: &mut BtCursor, p_different_row: &mut i32) -> i32 {
    let rc = restore_cursor_position(p_cur);
    if rc != 0 {
        *p_different_row = 1;
        return rc;
    }
    if p_cur.e_state != CURSOR_VALID {
        *p_different_row = 1;
    } else {
        *p_different_row = 0;
    }
    SQLITE_OK
}

/// Dá ao cursor dicas na forma de flags.
pub fn btree_cursor_hint_flags(p_cur: &mut BtCursor, x: u32) {
    p_cur.hints = x as u8;
}

/// Dado o número de uma página regular do banco, devolve o número da página do
/// mapa de ponteiros que contém a entrada da página dada.
///
/// Devolve 0 (página inválida) para `pgno == 1`, pois não há mapa de ponteiros
/// associado à página 1. A lógica do integrity_check exige que
/// `ptrmap_pageno(*, 1) != 1`.
pub fn ptrmap_pageno(p_bt: &BtShared, pgno: u32) -> u32 {
    if pgno < 2 {
        return 0;
    }
    let n_pages_per_map_page: u32 = (p_bt.usable_size / 5) + 1;
    let i_ptr_map: u32 = (pgno - 2) / n_pages_per_map_page;
    let mut ret: u32 = i_ptr_map.wrapping_mul(n_pages_per_map_page).wrapping_add(2);
    if ret == pending_byte_page(p_bt) {
        ret = ret.wrapping_add(1);
    }
    ret
}

/// Escreve uma entrada no mapa de ponteiros.
///
/// Atualiza a entrada da página `key` para que mapeie ao tipo `e_type` e à
/// página pai `parent`.
///
/// Se `*p_rc` já é diferente de `SQLITE_OK`, a rotina não faz nada. Se ocorre
/// um erro, o código apropriado é escrito em `*p_rc`.
pub fn ptrmap_put(p_bt: &BtShared, key: u32, e_type: u8, parent: u32, p_rc: &mut i32) {
    if *p_rc != 0 {
        return;
    }

    if key == 0 {
        *p_rc = SQLITE_CORRUPT_BKPT;
        return;
    }
    let i_ptrmap = ptrmap_pageno(p_bt, key);
    let (rc, p_db_page) = pager_get(&p_bt.p_pager, i_ptrmap, 0);
    if rc != SQLITE_OK {
        *p_rc = rc;
        return;
    }
    let p_db_page = match p_db_page {
        Some(pg) => pg,
        None => {
            *p_rc = SQLITE_CORRUPT_BKPT;
            return;
        }
    };
    if p_db_page.borrow().p_extra[0] != 0 {
        // O primeiro byte dos dados extras é o byte MemPage.isInit. Se estiver
        // ligado, esta página também está sendo usada como página de btree.
        *p_rc = SQLITE_CORRUPT_BKPT;
        pager_unref(&p_db_page);
        return;
    }
    let offset = ptrmap_ptroffset(i_ptrmap, key);
    if offset < 0 {
        *p_rc = SQLITE_CORRUPT_BKPT;
        pager_unref(&p_db_page);
        return;
    }
    let off = offset as usize;
    let (cur_type, cur_parent) = {
        let pg = p_db_page.borrow();
        (pg.p_data[off], get4byte(&pg.p_data, off + 1))
    };

    if e_type != cur_type || cur_parent != parent {
        let rc = pager_write(&p_db_page);
        *p_rc = rc;
        if rc == SQLITE_OK {
            let mut pg = p_db_page.borrow_mut();
            pg.p_data[off] = e_type;
            put4byte(&mut pg.p_data, off + 1, parent);
        }
    }

    pager_unref(&p_db_page);
}


// ---- part_003.rs ----

/// Lê uma entrada do mapa de ponteiros.
///
/// Recupera a entrada do mapa de ponteiros da página `key`, escrevendo o tipo em
/// `*p_etype` e o número da página pai em `*p_pgno` (que pode ser `None`, como o
/// ponteiro NULL do C). Devolve um código de erro se algo falhar, senão
/// `SQLITE_OK`.
pub fn ptrmap_get(
    p_bt: &BtShared,
    key: u32,
    p_etype: &mut u8,
    p_pgno: Option<&mut u32>,
) -> i32 {
    let i_ptrmap = ptrmap_pageno(p_bt, key);
    let (rc, p_db_page) = pager_get(&p_bt.p_pager, i_ptrmap, 0);
    if rc != 0 {
        return rc;
    }
    let p_db_page = match p_db_page {
        Some(pg) => pg,
        None => return SQLITE_CORRUPT_BKPT,
    };

    let offset = ptrmap_ptroffset(i_ptrmap, key);
    if offset < 0 {
        pager_unref(&p_db_page);
        return SQLITE_CORRUPT_BKPT;
    }
    let off = offset as usize;
    {
        let pg = p_db_page.borrow();
        *p_etype = pg.p_data[off];
        if let Some(p_pgno) = p_pgno {
            *p_pgno = get4byte(&pg.p_data, off + 1);
        }
    }

    pager_unref(&p_db_page);
    if *p_etype < 1 || *p_etype > 5 {
        return sqlite_corrupt_pgno(i_ptrmap);
    }
    SQLITE_OK
}

/// Dada uma página btree e um índice de célula (0 é a primeira célula da página,
/// 1 a segunda, e assim por diante), devolve o deslocamento do conteúdo da
/// célula em `a_data`. Só funciona em páginas sem células de overflow.
#[inline]
pub fn find_cell(p: &MemPage, i: usize) -> usize {
    (p.mask_page as usize) & (get2byte_aligned(&p.a_data, p.a_cell_idx + 2 * i) as usize)
}

/// Faz o mesmo que `find_cell()`, mas parte de `a_data_ofst`, pulando os 4 bytes
/// iniciais do ponteiro de filho das páginas interiores, se houver.
#[inline]
pub fn find_cell_past_ptr(p: &MemPage, i: usize) -> usize {
    p.a_data_ofst
        + ((p.mask_page as usize) & (get2byte_aligned(&p.a_data, p.a_cell_idx + 2 * i) as usize))
}

/// Tamanho utilizável do banco, lido do `BtShared` da página (`pPage->pBt->usableSize`).
#[inline]
fn page_usable_size(p_page: &MemPage) -> u32 {
    match p_page.p_bt.upgrade() {
        Some(bt) => bt.borrow().usable_size,
        None => 0,
    }
}

/// Processamento comum de cauda de `btree_parse_cell_ptr()` e
/// `btree_parse_cell_ptr_index()` para o caso em que a célula não cabe inteira
/// numa única página b-tree. Faz os ajustes necessários na estrutura `CellInfo`.
///
/// `p_cell` começa no início da célula e `CellInfo.p_payload` é um deslocamento
/// relativo a esse início.
pub fn btree_parse_cell_adjust_size_for_overflow(
    p_page: &MemPage,
    _p_cell: &[u8],
    p_info: &mut CellInfo,
) {
    // Se a carga útil não cabe por inteiro na página local, é preciso decidir
    // quanto guardar localmente e quanto derramar para páginas de overflow. A
    // estratégia é minimizar o espaço sem uso nas páginas de overflow mantendo a
    // quantidade local entre min_local e max_local.
    //
    // Aviso: mudar de qualquer forma a distribuição da carga de overflow gera um
    // formato de arquivo incompatível.
    let min_local: i32 = p_page.min_local as i32;
    let max_local: i32 = p_page.max_local as i32;
    let surplus: i32 = min_local
        .wrapping_add(
            p_info
                .n_payload
                .wrapping_sub(min_local as u32)
                .wrapping_rem(page_usable_size(p_page).wrapping_sub(4)) as i32,
        );
    if surplus <= max_local {
        p_info.n_local = surplus as u16;
    } else {
        p_info.n_local = min_local as u16;
    }
    p_info.n_size = ((p_info.p_payload + p_info.n_local as usize) as u16).wrapping_add(4);
}

/// Dado um registro com `n_payload` bytes de carga útil guardado na página
/// `p_page`, devolve quantos bytes da carga ficam armazenados localmente.
pub fn btree_payload_to_local(p_page: &MemPage, n_payload: i64) -> i32 {
    let max_local: i32 = p_page.max_local as i32;
    if n_payload <= max_local as i64 {
        n_payload as i32
    } else {
        let min_local: i32 = p_page.min_local as i32;
        let surplus: i32 = (min_local as i64
            + (n_payload - min_local as i64) % (page_usable_size(p_page).wrapping_sub(4) as i64))
            as i32;
        if surplus <= max_local {
            surplus
        } else {
            min_local
        }
    }
}

/// As rotinas a seguir implementam o método `MemPage.x_parse_cell()`: analisam o
/// bloco de conteúdo de uma célula e preenchem a estrutura `CellInfo`.
///
/// `btree_parse_cell_ptr_no_payload()` => nós internos de btree de tabela
/// `btree_parse_cell_ptr()`            => folhas de btree de tabela
/// `btree_parse_cell_ptr_index()`      => nós de btree de índice
///
/// Existe também o invólucro `btree_parse_cell()`, que serve a todos os tipos de
/// MemPage e referencia a célula por índice em vez de por ponteiro.
///
/// Em todas, `p_cell` começa no início da célula.
pub fn btree_parse_cell_ptr_no_payload(_p_page: &MemPage, p_cell: &[u8], p_info: &mut CellInfo) {
    let mut n_key: u64 = 0;
    let n = get_varint(&p_cell[4..], &mut n_key);
    p_info.n_size = (4 + n as u32) as u16;
    p_info.n_key = n_key as i64;
    p_info.n_payload = 0;
    p_info.n_local = 0;
    p_info.p_payload = 0;
}

pub fn btree_parse_cell_ptr(p_page: &MemPage, p_cell: &[u8], p_info: &mut CellInfo) {
    let mut it: usize = 0;

    // O bloco seguinte equivale a `it += getVarint32(it, n_payload)`, expandido
    // para evitar uma chamada de função.
    let mut n_payload: u32 = p_cell[it] as u32;
    if n_payload >= 0x80 {
        let end = it + 8;
        n_payload &= 0x7f;
        loop {
            it += 1;
            n_payload = (n_payload << 7) | (p_cell[it] as u32 & 0x7f);
            if !(p_cell[it] >= 0x80 && it < end) {
                break;
            }
        }
    }
    it += 1;

    // O bloco seguinte equivale a `it += getVarint(it, n_key)`, expandido e com
    // o laço desenrolado por desempenho. Esta rotina é de uso intenso.
    let mut i_key: u64 = p_cell[it] as u64;
    if i_key >= 0x80 {
        it += 1;
        let mut x = p_cell[it];
        i_key = (i_key << 7) ^ (x as u64);
        if x >= 0x80 {
            it += 1;
            x = p_cell[it];
            i_key = (i_key << 7) ^ (x as u64);
            if x >= 0x80 {
                it += 1;
                x = p_cell[it];
                i_key = (i_key << 7) ^ 0x10204000 ^ (x as u64);
                if x >= 0x80 {
                    it += 1;
                    x = p_cell[it];
                    i_key = (i_key << 7) ^ 0x4000 ^ (x as u64);
                    if x >= 0x80 {
                        it += 1;
                        x = p_cell[it];
                        i_key = (i_key << 7) ^ 0x4000 ^ (x as u64);
                        if x >= 0x80 {
                            it += 1;
                            x = p_cell[it];
                            i_key = (i_key << 7) ^ 0x4000 ^ (x as u64);
                            if x >= 0x80 {
                                it += 1;
                                x = p_cell[it];
                                i_key = (i_key << 7) ^ 0x4000 ^ (x as u64);
                                if x >= 0x80 {
                                    it += 1;
                                    i_key = (i_key << 8) ^ 0x8000 ^ (p_cell[it] as u64);
                                }
                            }
                        }
                    }
                }
            } else {
                i_key ^= 0x204000;
            }
        } else {
            i_key ^= 0x4000;
        }
    }
    it += 1;

    p_info.n_key = i_key as i64;
    p_info.n_payload = n_payload;
    p_info.p_payload = it;
    if n_payload <= p_page.max_local as u32 {
        // Caso comum (fácil): a carga útil inteira cabe na página local, sem
        // overflow.
        p_info.n_size = n_payload.wrapping_add(it as u32) as u16;
        if p_info.n_size < 4 {
            p_info.n_size = 4;
        }
        p_info.n_local = n_payload as u16;
    } else {
        btree_parse_cell_adjust_size_for_overflow(p_page, p_cell, p_info);
    }
}

pub fn btree_parse_cell_ptr_index(p_page: &MemPage, p_cell: &[u8], p_info: &mut CellInfo) {
    let mut it: usize = p_page.child_ptr_size as usize;
    let mut n_payload: u32 = p_cell[it] as u32;
    if n_payload >= 0x80 {
        let end = it + 8;
        n_payload &= 0x7f;
        loop {
            it += 1;
            n_payload = (n_payload << 7) | (p_cell[it] as u32 & 0x7f);
            if !(p_cell[it] >= 0x80 && it < end) {
                break;
            }
        }
    }
    it += 1;
    p_info.n_key = n_payload as i64;
    p_info.n_payload = n_payload;
    p_info.p_payload = it;
    if n_payload <= p_page.max_local as u32 {
        // Caso comum (fácil): a carga útil inteira cabe na página local, sem
        // overflow.
        p_info.n_size = n_payload.wrapping_add(it as u32) as u16;
        if p_info.n_size < 4 {
            p_info.n_size = 4;
        }
        p_info.n_local = n_payload as u16;
    } else {
        btree_parse_cell_adjust_size_for_overflow(p_page, p_cell, p_info);
    }
}

pub fn btree_parse_cell(p_page: &MemPage, i_cell: usize, p_info: &mut CellInfo) {
    let off = find_cell(p_page, i_cell);
    (p_page.x_parse_cell)(p_page, &p_page.a_data[off..], p_info);
}

/// As rotinas a seguir implementam o método `MemPage.x_cell_size`.
///
/// Calculam o total de bytes que uma célula ocupa na área de dados de células da
/// página btree. O número devolvido inclui o cabeçalho da célula e a carga
/// local, mas não as páginas de overflow nem o espaço do ponteiro de célula.
///
/// `cell_size_ptr_no_payload()` => nós internos de tabela
/// `cell_size_ptr_table_leaf()` => folhas de tabela
/// `cell_size_ptr()`            => nós internos de índice
/// `cell_size_ptr_idx_leaf()`   => folhas de índice
///
/// `p_cell` começa no início da célula.
pub fn cell_size_ptr(p_page: &MemPage, p_cell: &[u8]) -> u16 {
    let mut it: usize = 4;
    let mut n_size: u32 = p_cell[it] as u32;
    if n_size >= 0x80 {
        let end = it + 8;
        n_size &= 0x7f;
        loop {
            it += 1;
            n_size = (n_size << 7) | (p_cell[it] as u32 & 0x7f);
            if !(p_cell[it] >= 0x80 && it < end) {
                break;
            }
        }
    }
    it += 1;
    if n_size <= p_page.max_local as u32 {
        n_size = n_size.wrapping_add(it as u32);
    } else {
        let min_local = p_page.min_local as u32;
        n_size = min_local.wrapping_add(
            n_size
                .wrapping_sub(min_local)
                .wrapping_rem(page_usable_size(p_page).wrapping_sub(4)),
        );
        if n_size > p_page.max_local as u32 {
            n_size = min_local;
        }
        n_size = n_size.wrapping_add(4 + it as u32);
    }
    n_size as u16
}

pub fn cell_size_ptr_idx_leaf(p_page: &MemPage, p_cell: &[u8]) -> u16 {
    let mut it: usize = 0;
    let mut n_size: u32 = p_cell[it] as u32;
    if n_size >= 0x80 {
        let end = it + 8;
        n_size &= 0x7f;
        loop {
            it += 1;
            n_size = (n_size << 7) | (p_cell[it] as u32 & 0x7f);
            if !(p_cell[it] >= 0x80 && it < end) {
                break;
            }
        }
    }
    it += 1;
    if n_size <= p_page.max_local as u32 {
        n_size = n_size.wrapping_add(it as u32);
        if n_size < 4 {
            n_size = 4;
        }
    } else {
        let min_local = p_page.min_local as u32;
        n_size = min_local.wrapping_add(
            n_size
                .wrapping_sub(min_local)
                .wrapping_rem(page_usable_size(p_page).wrapping_sub(4)),
        );
        if n_size > p_page.max_local as u32 {
            n_size = min_local;
        }
        n_size = n_size.wrapping_add(4 + it as u32);
    }
    n_size as u16
}


// ---- part_004.rs ----

// Convenções deste trecho (as asserções do C somem, porque o Debian compila sem
// SQLITE_DEBUG e com NDEBUG; os `testcase()` também somem).
//
// Uma `u8 *pCell` vira `&[u8]` que começa na célula e vai até o fim do buffer onde
// ela mora (para uma célula na página, até o fim de `a_data`). Um `u8 *` devolvido
// vira o índice `usize` em `a_data` (0 significa NULL, porque nenhum espaço
// alocável começa no byte 0 da página).

/// Tamanho de uma célula de página interna de tabela (sem payload): 4 bytes do
/// ponteiro de filho mais o varint da chave (no máximo 9 bytes).
fn cell_size_ptr_no_payload(p_page: &MemPage, p_cell: &[u8]) -> u16 {
    let _ = p_page;
    let mut p_iter: usize = 4; // índice para percorrer os bytes de p_cell
    let p_end = p_iter + 9; // marca de fim de um varint
    loop {
        let b = p_cell[p_iter];
        p_iter += 1;
        if !((b & 0x80) != 0 && p_iter < p_end) {
            break;
        }
    }
    p_iter as u16
}

/// Tamanho de uma célula de folha de tabela intKey: varint do tamanho do payload,
/// varint da chave de 64 bits e, se o payload não cabe local, o ponteiro de overflow.
fn cell_size_ptr_table_leaf(p_page: &MemPage, p_cell: &[u8]) -> u16 {
    let mut p_iter: usize = 0; // índice para percorrer os bytes de p_cell
    let mut n_size: u32 = p_cell[p_iter] as u32; // valor de tamanho a devolver
    if n_size >= 0x80 {
        let p_end = p_iter + 8;
        n_size &= 0x7f;
        loop {
            p_iter += 1;
            n_size = (n_size << 7) | ((p_cell[p_iter] & 0x7f) as u32);
            if !(p_cell[p_iter] >= 0x80 && p_iter < p_end) {
                break;
            }
        }
    }
    p_iter += 1;
    // p_iter agora aponta para a chave inteira de 64 bits, um inteiro de tamanho
    // variável. O bloco a seguir move p_iter para o primeiro byte depois do fim da
    // chave (até 8 bytes com o bit alto ligado, e então um nono byte inteiro).
    let mut more = true;
    for _ in 0..8 {
        let b = p_cell[p_iter];
        p_iter += 1;
        if (b & 0x80) == 0 {
            more = false;
            break;
        }
    }
    if more {
        p_iter += 1;
    }
    let max_local = p_page.max_local as u32;
    if n_size <= max_local {
        n_size = n_size.wrapping_add(p_iter as u32);
        if n_size < 4 {
            n_size = 4;
        }
    } else {
        let min_local = p_page.min_local as u32;
        let usable_size = page_usable_size(p_page) as u32;
        n_size = min_local.wrapping_add(n_size.wrapping_sub(min_local) % usable_size.wrapping_sub(4));
        if n_size > max_local {
            n_size = min_local;
        }
        n_size = n_size.wrapping_add(4).wrapping_add((p_iter as u16) as u32);
    }
    n_size as u16
}

// `cellSize()` só existe sob SQLITE_DEBUG (usada dentro de assert): não é traduzida.

/// A célula p_cell hoje faz parte da página p_src mas acabará fazendo parte de
/// p_page (as duas costumam ser a mesma). Se ela contém um ponteiro para página de
/// overflow, grava no mapa de ponteiros a entrada da página de overflow que será
/// válida depois que a célula for movida para p_page.
///
/// O `SQLITE_OVERFLOW(pSrc->aDataEnd, pCell, pCell+nLocal)` do C testa se a célula
/// atravessa o fim da página: como `p_cell` vai até o fim do buffer da página de
/// origem, isso é `n_local > p_cell.len()`.
fn ptrmap_put_ovfl_ptr(p_page: &MemPage, _p_src: &MemPage, p_cell: &[u8], p_rc: &mut i32) {
    let mut info = CellInfo::default();
    if *p_rc != 0 {
        return;
    }
    (p_page.x_parse_cell)(p_page, p_cell, &mut info);
    if (info.n_local as u32) < info.n_payload {
        if sqlite_overflow(p_cell.len(), 0, info.n_local as usize) {
            *p_rc = sqlite_corrupt_bkpt(line!() as i32);
            return;
        }
        let ovfl: u32 = get4byte(&p_cell[(info.n_size as usize - 4)..]);
        if let Some(p_bt) = p_page.p_bt.as_ref().and_then(|w| w.upgrade()) {
            ptrmap_put(&p_bt.borrow(), ovfl, PTRMAP_OVERFLOW1, p_page.pgno, p_rc);
        }
    }
}

/// Desfragmenta a página: reorganiza as células para que não haja blocos livres na
/// lista de blocos livres.
///
/// n_max_frag é a quantidade máxima de espaço fragmentado que pode restar na página
/// quando a rotina retornar.
///
/// EVIDENCE-OF: R-44582-60138 o SQLite pode reorganizar uma página de b-tree para
/// que não haja blocos livres nem bytes de fragmento, todos os bytes não usados
/// fiquem na região não alocada e todas as células fiquem compactadas no fim.
fn defragment_page(p_page: &mut MemPage, n_max_frag: i32) -> i32 {
    let hdr = p_page.hdr_offset as usize; // deslocamento do cabeçalho da página
    let cell_offset = p_page.cell_offset as usize; // deslocamento do vetor de ponteiros
    let n_cell = p_page.n_cell as usize; // número de células na página
    let i_cell_first = cell_offset + 2 * n_cell; // primeiro índice de célula permitido
    let usable_size = page_usable_size(p_page); // bytes usáveis na página
    let mut c_brk: usize = 0; // deslocamento da área de conteúdo das células

    'defragment_out: {
        // Este bloco trata páginas com até dois blocos livres e n_max_frag ou
        // menos bytes fragmentados. Nesse caso é mais rápido mover os dois (ou
        // um) blocos de células com memmove e somar os deslocamentos a cada
        // ponteiro do vetor do que reconstruir a página inteira.
        if (p_page.a_data[hdr + 7] as i32) <= n_max_frag {
            let i_free = get2byte(&p_page.a_data[hdr + 1..]) as usize;
            if i_free > usable_size - 4 {
                return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
            }
            if i_free != 0 {
                let i_free2 = get2byte(&p_page.a_data[i_free..]) as usize;
                if i_free2 > usable_size - 4 {
                    return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
                }
                if 0 == i_free2 || (p_page.a_data[i_free2] == 0 && p_page.a_data[i_free2 + 1] == 0) {
                    let p_end = cell_offset + n_cell * 2;
                    let mut sz2: usize = 0;
                    let mut sz = get2byte(&p_page.a_data[i_free + 2..]) as usize;
                    let top = get2byte(&p_page.a_data[hdr + 5..]) as usize;
                    if top >= i_free {
                        return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
                    }
                    if i_free2 != 0 {
                        if i_free + sz > i_free2 {
                            return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
                        }
                        sz2 = get2byte(&p_page.a_data[i_free2 + 2..]) as usize;
                        if i_free2 + sz2 > usable_size {
                            return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
                        }
                        p_page.a_data.copy_within((i_free + sz)..i_free2, i_free + sz + sz2);
                        sz += sz2;
                    } else if i_free + sz > usable_size {
                        return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
                    }

                    c_brk = top + sz;
                    p_page.a_data.copy_within(top..i_free, c_brk);
                    let mut p_addr = cell_offset;
                    while p_addr < p_end {
                        let pc = get2byte(&p_page.a_data[p_addr..]) as usize;
                        if pc < i_free {
                            put2byte(&mut p_page.a_data[p_addr..], (pc + sz) as u16);
                        } else if pc < i_free2 {
                            put2byte(&mut p_page.a_data[p_addr..], (pc + sz2) as u16);
                        }
                        p_addr += 2;
                    }
                    break 'defragment_out;
                }
            }
        }

        c_brk = usable_size;
        let i_cell_last = usable_size - 4; // último índice de célula possível
        let i_cell_start = get2byte(&p_page.a_data[hdr + 5..]) as usize; // primeiro deslocamento de célula na entrada
        if n_cell > 0 {
            // sqlite3PagerTempSpace() é um buffer de rascunho do paginador: uma
            // cópia da página dá o mesmo efeito.
            let src: Vec<u8> = p_page.a_data[..usable_size].to_vec();
            for i in 0..n_cell {
                let p_addr = cell_offset + i * 2; // i-ésimo ponteiro de célula
                let pc = get2byte(&p_page.a_data[p_addr..]) as usize;
                // Estas condições já foram verificadas em btree_init_page() se
                // PRAGMA cell_size_check=ON.
                if pc > i_cell_last {
                    return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
                }
                let size = (p_page.x_cell_size)(p_page, &src[pc..]) as usize;
                // cbrk < size equivale a cbrk - size < 0 <= i_cell_start no C.
                if size > c_brk {
                    return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
                }
                c_brk -= size;
                if c_brk < i_cell_start || pc + size > usable_size {
                    return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
                }
                put2byte(&mut p_page.a_data[p_addr..], c_brk as u16);
                p_page.a_data[c_brk..c_brk + size].copy_from_slice(&src[pc..pc + size]);
            }
        }
        p_page.a_data[hdr + 7] = 0;
    }

    // defragment_out:
    if (p_page.a_data[hdr + 7] as i32) + (c_brk as i32) - (i_cell_first as i32) != p_page.n_free {
        return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
    }
    put2byte(&mut p_page.a_data[hdr + 5..], c_brk as u16);
    p_page.a_data[hdr + 1] = 0;
    p_page.a_data[hdr + 2] = 0;
    p_page.a_data[i_cell_first..c_brk].fill(0);
    SQLITE_OK
}

/// Procura na lista livre da página p_pg espaço para uma célula de n_byte bytes.
/// Se achar, devolve o índice do espaço em `a_data` e o remove da lista livre.
/// Se não houver espaço adequado na lista livre, devolve 0 (o NULL do C).
///
/// A função pode detectar corrupção em p_pg: nesse caso grava SQLITE_CORRUPT em
/// *p_rc e devolve 0.
///
/// Slots da lista livre de 1 a 3 bytes maiores que n_byte são ignorados se somar o
/// espaço extra à contagem de fragmentação fizer a contagem passar de 60.
fn page_find_slot(p_pg: &mut MemPage, n_byte: usize, p_rc: &mut i32) -> usize {
    let hdr = p_pg.hdr_offset as usize; // deslocamento do cabeçalho da página
    let n_byte = n_byte as i32;
    let mut i_addr = hdr + 1; // endereço do ponteiro para pc
    let mut pc = get2byte(&p_pg.a_data[i_addr..]) as i32; // endereço de um slot livre
    let max_pc = page_usable_size(p_pg) as i32 - n_byte; // maior endereço de um slot usável

    while pc <= max_pc {
        // EVIDENCE-OF: R-22710-53328 o terceiro e o quarto bytes de cada bloco
        // livre formam um inteiro big-endian com o tamanho do bloco em bytes,
        // incluindo o cabeçalho de 4 bytes.
        let size = get2byte(&p_pg.a_data[(pc as usize + 2)..]) as i32; // tamanho do slot livre
        let x = size - n_byte; // excesso de tamanho do slot
        if x >= 0 {
            if x < 4 {
                // EVIDENCE-OF: R-11498-58022 numa página de b-tree bem formada, o
                // total de bytes em fragmentos não pode passar de 60.
                if p_pg.a_data[hdr + 7] > 57 {
                    return 0;
                }

                // Tira o slot da lista livre e atualiza o número de bytes
                // fragmentados da página.
                p_pg.a_data.copy_within((pc as usize)..(pc as usize + 2), i_addr);
                p_pg.a_data[hdr + 7] = p_pg.a_data[hdr + 7].wrapping_add(x as u8);
                return pc as usize;
            } else if x + pc > max_pc {
                // O slot passa do fim da parte usável da página.
                *p_rc = sqlite_corrupt_pgno(line!() as i32, p_pg.pgno);
                return 0;
            } else {
                // O slot continua na lista livre. Reduz o tamanho dele pela parte
                // usada na nova alocação.
                put2byte(&mut p_pg.a_data[(pc as usize + 2)..], x as u16);
            }
            return (pc + x) as usize;
        }
        i_addr = pc as usize;
        pc = get2byte(&p_pg.a_data[i_addr..]) as i32;
        if pc <= i_addr as i32 {
            if pc != 0 {
                // O próximo slot da cadeia vem antes do atual.
                *p_rc = sqlite_corrupt_pgno(line!() as i32, p_pg.pgno);
            }
            return 0;
        }
    }
    if pc > max_pc + n_byte - 4 {
        // A cadeia de slots livres passa do fim da página.
        *p_rc = sqlite_corrupt_pgno(line!() as i32, p_pg.pgno);
    }
    0
}

/// Aloca n_byte bytes de espaço dentro da página de b-tree. Grava em *p_idx o
/// índice em `a_data` do primeiro byte alocado. Devolve SQLITE_OK ou um código de
/// erro (normalmente SQLITE_CORRUPT).
///
/// O chamador garante que há espaço suficiente. A rotina pode precisar desfragmentar
/// para juntar todo o espaço. Ela evita usar os dois primeiros bytes depois da área
/// de ponteiros de célula, porque a alocação serve para inserir uma célula nova e
/// portanto também será preciso um novo ponteiro de célula.
#[inline]
fn allocate_space(p_page: &mut MemPage, n_byte: usize, p_idx: &mut usize) -> i32 {
    let hdr = p_page.hdr_offset as usize; // cópia local de hdr_offset
    let usable_size = page_usable_size(p_page);
    let mut rc: i32; // código de retorno

    // primeiro byte do vão entre os ponteiros de célula e o conteúdo
    let gap = p_page.cell_offset as usize + 2 * p_page.n_cell as usize;
    // EVIDENCE-OF: R-29356-02391 com página de 65536 bytes e reserva zero, o
    // deslocamento do conteúdo de uma página vazia quer ser 65536, que não cabe em
    // 2 bytes, então se grava 0 no lugar.
    let mut top = get2byte(&p_page.a_data[hdr + 5..]) as usize; // primeiro byte da área de conteúdo
    if gap > top {
        if top == 0 && usable_size == 65536 {
            top = 65536;
        } else {
            return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
        }
    } else if top > usable_size {
        return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
    }

    // Se há espaço entre gap e top para mais um ponteiro de célula e a lista livre
    // não está vazia, procura na lista livre um slot grande o bastante.
    if (p_page.a_data[hdr + 2] != 0 || p_page.a_data[hdr + 1] != 0) && gap + 2 <= top {
        rc = SQLITE_OK;
        let p_space = page_find_slot(p_page, n_byte, &mut rc);
        if p_space != 0 {
            *p_idx = p_space;
            if p_space <= gap {
                return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
            } else {
                return SQLITE_OK;
            }
        } else if rc != 0 {
            return rc;
        }
    }

    // O pedido não coube num slot da lista livre. Vê se é preciso desfragmentar.
    if gap + 2 + n_byte > top {
        rc = defragment_page(p_page, std::cmp::min(4, p_page.n_free - (2 + n_byte as i32)));
        if rc != 0 {
            return rc;
        }
        top = get2byte_not_zero(&p_page.a_data[hdr + 5..]) as usize;
    }

    // Aloca do vão entre o vetor de ponteiros e a área de conteúdo. A chamada a
    // btree_compute_free_space() já validou a lista livre; sendo ela válida, a
    // alocação não passa do fim da página.
    top -= n_byte;
    put2byte(&mut p_page.a_data[hdr + 5..], top as u16);
    *p_idx = top;
    SQLITE_OK
}

// Auxiliares compartilhados com outros trechos de btree.c.

/// `testcase()` do C: marcador de cobertura, sem efeito fora de SQLITE_COVERAGE_TEST.
#[inline]
fn testcase(_x: bool) {}

/// `sqlite3PagerIswriteable(pPage->pDbPage)`.
fn is_pager_writeable(p_pg: Option<&PgHdrRef>) -> bool {
    match p_pg {
        Some(p) => pager_iswriteable(p) != 0,
        None => false,
    }
}

/// `pPage->pBt->usableSize`.
fn page_usable_size(p_page: &MemPage) -> usize {
    p_page
        .p_bt
        .as_ref()
        .and_then(|w| w.upgrade())
        .map(|b| b.borrow().usable_size as usize)
        .unwrap_or(0)
}

#[inline]
fn get_4byte(x: &[u8]) -> u32 {
    get4byte(x)
}

/// `CORRUPT_DB` (`sqlite3Config.neverCorrupt==0`): só aparece dentro de assert() e
/// de testcase(), logo o valor não influencia o resultado.
const CORRUPT_DB: bool = true;


// ---- part_005.rs ----

// Convenções deste trecho: as asserções do C somem (o Debian compila sem
// SQLITE_DEBUG), e `testcase()` também. O `SQLITE_CORRUPT_PAGE(p)` do C é
// `sqlite_corrupt_pgno(linha, p.pgno)`.

/// Devolve uma seção de p_page.a_data à lista livre. O primeiro byte do novo bloco
/// livre é p_page.a_data[i_start] e o bloco tem i_size bytes.
///
/// Blocos livres adjacentes são fundidos.
///
/// Embora a lista de blocos livres tenha sido verificada por
/// btree_compute_free_space(), aquela rotina não detecta sobreposição entre células
/// ou blocos livres, nem células ou blocos livres que invadem os bytes reservados no
/// fim da página. Por isso esta rotina faz verificações de corrupção adicionais e
/// devolve SQLITE_CORRUPT se achar algum problema.
pub fn free_space(p_page: &mut MemPage, mut i_start: u16, mut i_size: u16) -> i32 {
    let mut i_ptr: u16; // endereço do ponteiro para o próximo bloco livre
    let mut i_free_blk: u16; // endereço do próximo bloco livre
    let hdr: u8 = p_page.hdr_offset; // tamanho do cabeçalho da página: 0 ou 100
    let mut n_frag: u8 = 0; // redução na fragmentação
    let i_orig_size: u16 = i_size; // valor original de i_size
    let x: u16; // deslocamento da área de conteúdo das células
    let mut i_end: u32 = i_start as u32 + i_size as u32; // primeiro byte depois do buffer i_start
    let usable_size = page_usable_size(p_page) as u32;
    let bts_flags = p_page
        .p_bt
        .as_ref()
        .and_then(|w| w.upgrade())
        .map(|b| b.borrow().bts_flags)
        .unwrap_or(0);

    // A lista de blocos livres precisa estar em ordem crescente. Acha o ponto da
    // lista onde i_start deve ser inserido.
    i_ptr = hdr as u16 + 1;
    if p_page.a_data[i_ptr as usize + 1] == 0 && p_page.a_data[i_ptr as usize] == 0 {
        i_free_blk = 0; // atalho para o caso de lista livre vazia
    } else {
        loop {
            i_free_blk = get2byte(&p_page.a_data[i_ptr as usize..]);
            if !(i_free_blk < i_start) {
                break;
            }
            if i_free_blk <= i_ptr {
                if i_free_blk == 0 {
                    break; // TH3: corrupt082.100
                }
                return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
            }
            i_ptr = i_free_blk;
        }
        if (i_free_blk as u32) > usable_size - 4 {
            // TH3: corrupt081.100
            return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
        }

        // Neste ponto:
        //    i_free_blk: primeiro bloco livre depois de i_start, ou zero se não há
        //    i_ptr:      endereço de um ponteiro para i_free_blk
        //
        // Vê se i_free_blk deve ser fundido ao fim de i_start.
        if i_free_blk != 0 && i_end + 3 >= i_free_blk as u32 {
            n_frag = (i_free_blk as u32).wrapping_sub(i_end) as u8;
            if i_end > i_free_blk as u32 {
                return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
            }
            i_end = i_free_blk as u32 + get2byte(&p_page.a_data[i_free_blk as usize + 2..]) as u32;
            if i_end > usable_size {
                return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
            }
            i_size = i_end.wrapping_sub(i_start as u32) as u16;
            i_free_blk = get2byte(&p_page.a_data[i_free_blk as usize..]);
        }

        // Se i_ptr é outro bloco livre (isto é, não é o ponteiro da lista livre no
        // cabeçalho da página), vê se i_start deve ser fundido ao fim de i_ptr.
        if i_ptr as u32 > hdr as u32 + 1 {
            let i_ptr_end: i32 = i_ptr as i32 + get2byte(&p_page.a_data[i_ptr as usize + 2..]) as i32;
            if i_ptr_end + 3 >= i_start as i32 {
                if i_ptr_end > i_start as i32 {
                    return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
                }
                n_frag = n_frag.wrapping_add((i_start as i32 - i_ptr_end) as u8);
                i_size = i_end.wrapping_sub(i_ptr as u32) as u16;
                i_start = i_ptr;
            }
        }
        if n_frag > p_page.a_data[hdr as usize + 7] {
            return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
        }
        p_page.a_data[hdr as usize + 7] -= n_frag;
    }
    x = get2byte(&p_page.a_data[hdr as usize + 5..]);
    if (bts_flags & BTS_FAST_SECURE) != 0 {
        // Sobrescreve a informação apagada com zeros quando a opção secure_delete
        // está ligada.
        p_page.a_data[i_start as usize..i_start as usize + i_size as usize].fill(0);
    }
    if i_start <= x {
        // O novo bloco livre está no começo da área de conteúdo das células,
        // então basta estender a área de conteúdo em vez de criar outra entrada na
        // lista livre.
        if i_start < x {
            return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
        }
        if i_ptr as u32 != hdr as u32 + 1 {
            return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
        }
        put2byte(&mut p_page.a_data[hdr as usize + 1..], i_free_blk);
        put2byte(&mut p_page.a_data[hdr as usize + 5..], i_end as u16);
    } else {
        // Insere o novo bloco livre na lista livre.
        put2byte(&mut p_page.a_data[i_ptr as usize..], i_start);
        put2byte(&mut p_page.a_data[i_start as usize..], i_free_blk);
        put2byte(&mut p_page.a_data[i_start as usize + 2..], i_size);
    }
    p_page.n_free += i_orig_size as i32;
    SQLITE_OK
}

/// Decodifica o byte de flags (o primeiro byte do cabeçalho) de uma página e
/// inicializa os campos da MemPage de acordo.
///
/// Só as combinações a seguir são aceitas. Qualquer outra indica arquivo de banco
/// de dados corrompido:
///
///         PTF_ZERODATA                             (0x02,  2)
///         PTF_LEAFDATA | PTF_INTKEY                (0x05,  5)
///         PTF_ZERODATA | PTF_LEAF                  (0x0a, 10)
///         PTF_LEAFDATA | PTF_INTKEY | PTF_LEAF     (0x0d, 13)
pub fn decode_flags(p_page: &mut MemPage, flag_byte: i32) -> i32 {
    // cópia dos campos de BtShared (p_bt do C)
    let (max1byte_payload, max_leaf, min_leaf, max_local, min_local) =
        match p_page.p_bt.as_ref().and_then(|w| w.upgrade()) {
            Some(b) => {
                let b = b.borrow();
                (b.max1byte_payload, b.max_leaf, b.min_leaf, b.max_local, b.min_local)
            }
            None => return sqlite_corrupt_pgno(line!() as i32, p_page.pgno),
        };

    p_page.max1byte_payload = max1byte_payload;
    if flag_byte >= (PTF_ZERODATA | PTF_LEAF) as i32 {
        p_page.child_ptr_size = 0;
        p_page.leaf = 1;
        if flag_byte == (PTF_LEAFDATA | PTF_INTKEY | PTF_LEAF) as i32 {
            p_page.int_key_leaf = 1;
            p_page.x_cell_size = cell_size_ptr_table_leaf;
            p_page.x_parse_cell = btree_parse_cell_ptr;
            p_page.int_key = 1;
            p_page.max_local = max_leaf;
            p_page.min_local = min_leaf;
        } else if flag_byte == (PTF_ZERODATA | PTF_LEAF) as i32 {
            p_page.int_key = 0;
            p_page.int_key_leaf = 0;
            p_page.x_cell_size = cell_size_ptr_idx_leaf;
            p_page.x_parse_cell = btree_parse_cell_ptr_index;
            p_page.max_local = max_local;
            p_page.min_local = min_local;
        } else {
            p_page.int_key = 0;
            p_page.int_key_leaf = 0;
            p_page.x_cell_size = cell_size_ptr_idx_leaf;
            p_page.x_parse_cell = btree_parse_cell_ptr_index;
            return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
        }
    } else {
        p_page.child_ptr_size = 4;
        p_page.leaf = 0;
        if flag_byte == PTF_ZERODATA as i32 {
            p_page.int_key = 0;
            p_page.int_key_leaf = 0;
            p_page.x_cell_size = cell_size_ptr;
            p_page.x_parse_cell = btree_parse_cell_ptr_index;
            p_page.max_local = max_local;
            p_page.min_local = min_local;
        } else if flag_byte == (PTF_LEAFDATA | PTF_INTKEY) as i32 {
            p_page.int_key_leaf = 0;
            p_page.x_cell_size = cell_size_ptr_no_payload;
            p_page.x_parse_cell = btree_parse_cell_ptr_no_payload;
            p_page.int_key = 1;
            p_page.max_local = max_leaf;
            p_page.min_local = min_leaf;
        } else {
            p_page.int_key = 0;
            p_page.int_key_leaf = 0;
            p_page.x_cell_size = cell_size_ptr;
            p_page.x_parse_cell = btree_parse_cell_ptr_index;
            return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
        }
    }
    SQLITE_OK
}

/// Calcula a quantidade de espaço livre na página, isto é, preenche o campo
/// p_page.n_free.
pub fn btree_compute_free_space(p_page: &mut MemPage) -> i32 {
    let usable_size: i32 = page_usable_size(p_page) as i32; // espaço usável em cada página
    let hdr = p_page.hdr_offset as usize; // deslocamento do início do cabeçalho
    // EVIDENCE-OF: R-58015-48175 o inteiro de dois bytes no deslocamento 5 marca o
    // início da área de conteúdo das células. O valor zero é lido como 65536.
    let top: i32 = get2byte_not_zero(&p_page.a_data[hdr + 5..]) as i32; // primeiro byte da área de conteúdo
    // primeiro deslocamento permitido de célula ou bloco livre
    let i_cell_first: i32 = hdr as i32 + 8 + p_page.child_ptr_size as i32 + 2 * p_page.n_cell as i32;
    // último deslocamento possível de célula ou bloco livre
    let i_cell_last: i32 = usable_size - 4;

    // Calcula o espaço livre total da página.
    // EVIDENCE-OF: R-23588-34450 o inteiro de dois bytes no deslocamento 1 dá o
    // início do primeiro bloco livre da página, ou zero se não há blocos livres.
    let mut pc: i32 = get2byte(&p_page.a_data[hdr + 1..]) as i32; // endereço de um bloco livre
    // começa n_free com o espaço livre que não é bloco livre
    let mut n_free: i32 = p_page.a_data[hdr + 7] as i32 + top;
    if pc > 0 {
        let mut next: i32;
        let mut size: i32;
        if pc < top {
            // EVIDENCE-OF: R-55530-52930 numa página de b-tree bem formada há
            // sempre ao menos uma célula antes do primeiro bloco livre.
            return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
        }
        loop {
            if pc > i_cell_last {
                // Bloco livre fora do fim da página
                return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
            }
            next = get2byte(&p_page.a_data[pc as usize..]) as i32;
            size = get2byte(&p_page.a_data[pc as usize + 2..]) as i32;
            n_free += size;
            if next <= pc + size + 3 {
                break;
            }
            pc = next;
        }
        if next > 0 {
            // Bloco livre fora de ordem crescente
            return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
        }
        if (pc + size) as u32 > usable_size as u32 {
            // O último bloco livre passa do fim da página
            return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
        }
    }

    // Neste ponto n_free guarda a soma do deslocamento do início da área de
    // conteúdo com o número de bytes livres dentro dela. Se passar do tamanho
    // usável da página, a página está corrompida. A verificação também confirma
    // que o deslocamento do início da área de conteúdo, segundo o cabeçalho, cai
    // dentro da página.
    if n_free > usable_size || n_free < i_cell_first {
        return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
    }
    p_page.n_free = ((n_free - i_cell_first) as u16) as i32;
    SQLITE_OK
}

/// Verificação adicional de sanidade depois de btree_init_page() se
/// PRAGMA cell_size_check=ON.
pub fn btree_cell_size_check(p_page: &MemPage) -> i32 {
    let i_cell_first: i32 = p_page.cell_offset as i32 + 2 * p_page.n_cell as i32; // primeiro deslocamento permitido
    let usable_size: i32 = page_usable_size(p_page) as i32; // espaço usável máximo na página
    let mut i_cell_last: i32 = usable_size - 4; // último deslocamento possível
    let cell_offset: usize = p_page.cell_offset as usize; // início do vetor de ponteiros de célula
    if p_page.leaf == 0 {
        i_cell_last -= 1;
    }
    for i in 0..p_page.n_cell as usize {
        let pc: i32 = get2byte_aligned(&p_page.a_data[cell_offset + i * 2..]) as i32;
        if pc < i_cell_first || pc > i_cell_last {
            return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
        }
        let sz: i32 = (p_page.x_cell_size)(p_page, &p_page.a_data[pc as usize..]) as i32;
        if pc + sz > usable_size {
            return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
        }
    }
    SQLITE_OK
}

/// Inicializa a informação auxiliar de um bloco de disco.
///
/// Devolve SQLITE_OK em caso de sucesso. Se a página não contém uma página de banco
/// de dados bem formada, devolve SQLITE_CORRUPT. Um retorno SQLITE_OK não garante
/// que a página esteja bem formada: só mostra que nenhuma corrupção foi detectada.
pub fn btree_init_page(p_page: &mut MemPage) -> i32 {
    let p_bt = match p_page.p_bt.as_ref().and_then(|w| w.upgrade()) {
        Some(b) => b,
        None => return sqlite_corrupt_pgno(line!() as i32, p_page.pgno),
    };
    let hdr = p_page.hdr_offset as usize; // `data` do C é a_data + hdr_offset
    // EVIDENCE-OF: R-28594-02890 o byte de flag no deslocamento 0 indica o tipo da
    // página de b-tree.
    let flag_byte = p_page.a_data[hdr] as i32;
    if decode_flags(p_page, flag_byte) != 0 {
        return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
    }
    let page_size: u32 = p_bt.borrow().page_size;
    p_page.mask_page = (page_size - 1) as u16;
    p_page.n_overflow = 0;
    p_page.cell_offset = (hdr as u16) + 8 + p_page.child_ptr_size as u16;
    p_page.a_cell_idx = hdr + p_page.child_ptr_size as usize + 8;
    p_page.a_data_end = page_size as usize;
    p_page.a_data_ofst = p_page.child_ptr_size as usize;
    // EVIDENCE-OF: R-37002-32774 o inteiro de dois bytes no deslocamento 3 dá o
    // número de células da página.
    p_page.n_cell = get2byte(&p_page.a_data[hdr + 3..]);
    if p_page.n_cell as u32 > mx_cell(page_size) {
        // Células demais para uma única página: a página está corrompida.
        return sqlite_corrupt_pgno(line!() as i32, p_page.pgno);
    }
    // EVIDENCE-OF: R-24089-57979 se a página não tem células (só possível na raiz de
    // uma tabela sem linhas), o deslocamento da área de conteúdo é o tamanho da
    // página menos os bytes reservados.
    p_page.n_free = -1; // indica que o valor ainda não foi calculado
    p_page.is_init = 1;
    let db_flags: u64 = p_bt
        .borrow()
        .db
        .as_ref()
        .and_then(|w| w.upgrade())
        .map(|d| d.borrow().flags)
        .unwrap_or(0);
    if (db_flags & SQLITE_CELL_SIZE_CK) != 0 {
        return btree_cell_size_check(p_page);
    }
    SQLITE_OK
}


// ---- part_006.rs ----

// Convenções deste trecho: as asserções do C somem (o Debian compila sem
// SQLITE_DEBUG). Falha de alocação não existe em Rust seguro, então os ramos
// SQLITE_NOMEM de sqlite3MallocZero/sqlite3Malloc não são traduzidos.
//
// Modelagem do "extra" do paginador: em C, `sqlite3PagerGetExtra(pDbPage)` é o
// próprio MemPage, alocado junto da página. Aqui isso é `pager_get_mem_page`, que
// devolve o `MemPageRef` associado ao `PgHdrRef` (a ser fornecida pelo módulo do
// paginador, junto com o campo correspondente em `PgHdr`).

/// Configura uma página bruta para que pareça uma página de banco de dados sem
/// nenhuma entrada.
fn zero_page(p_page: &mut MemPage, flags: i32) {
    let (usable_size, page_size, bts_flags) = match p_page.p_bt.as_ref().and_then(|w| w.upgrade()) {
        Some(b) => {
            let b = b.borrow();
            (b.usable_size, b.page_size, b.bts_flags)
        }
        None => return,
    };
    let hdr = p_page.hdr_offset as usize;
    if (bts_flags & BTS_FAST_SECURE) != 0 {
        p_page.a_data[hdr..usable_size as usize].fill(0);
    }
    p_page.a_data[hdr] = flags as u8;
    let first: u16 = hdr as u16 + if (flags & PTF_LEAF as i32) == 0 { 12 } else { 8 };
    p_page.a_data[hdr + 1..hdr + 5].fill(0);
    p_page.a_data[hdr + 7] = 0;
    put2byte(&mut p_page.a_data[hdr + 5..], usable_size as u16);
    p_page.n_free = (usable_size.wrapping_sub(first as u32) as u16) as i32;
    decode_flags(p_page, flags);
    p_page.cell_offset = first;
    p_page.a_data_end = page_size as usize;
    p_page.a_cell_idx = first as usize;
    p_page.a_data_ofst = p_page.child_ptr_size as usize;
    p_page.n_overflow = 0;
    p_page.mask_page = (page_size - 1) as u16;
    p_page.n_cell = 0;
    p_page.is_init = 1;
}

/// Converte uma DbPage obtida do paginador em uma MemPage usada pela camada btree.
fn btree_page_from_db_page(p_db_page: &PgHdrRef, pgno: Pgno, p_bt: &BtSharedRef) -> MemPageRef {
    let p_page = pager_get_mem_page(p_db_page);
    {
        let mut pg = p_page.borrow_mut();
        if pgno != pg.pgno {
            pg.a_data = pager_get_data(p_db_page).to_vec();
            pg.p_db_page = Some(p_db_page.clone());
            pg.p_bt = Some(std::rc::Rc::downgrade(p_bt));
            pg.pgno = pgno;
            pg.hdr_offset = if pgno == 1 { 100 } else { 0 };
        }
    }
    p_page
}

/// Obtém uma página do paginador. Inicializa MemPage.p_bt e MemPage.a_data se for
/// preciso. Veja também: btree_get_unused_page().
///
/// Se a flag PAGER_GET_NOCONTENT está ligada, o conteúdo da página não importa por
/// ora, então não se vai ao disco buscá-lo: o conteúdo é preenchido com zeros. Se
/// no futuro sqlite3PagerWrite() for chamada nesta página, é porque o conteúdo
/// passou a importar e a leitura do disco deve acontecer naquele ponto.
fn btree_get_page(p_bt: &BtSharedRef, pgno: Pgno, pp_page: &mut Option<MemPageRef>, flags: i32) -> i32 {
    let mut p_db_page: Option<PgHdrRef> = None;
    let p_pager = p_bt.borrow().p_pager.clone().expect("BtShared sem pager");
    let rc = pager_get(&mut p_pager.borrow_mut(), pgno, &mut p_db_page, flags);
    if rc != 0 {
        return rc;
    }
    *pp_page = Some(btree_page_from_db_page(&p_db_page.expect("pager_get sem página"), pgno, p_bt));
    SQLITE_OK
}

/// Recupera uma página do cache do paginador. Se a página pedida não está no cache,
/// devolve None. Inicializa MemPage.p_bt e MemPage.a_data se for preciso.
fn btree_page_lookup(p_bt: &BtSharedRef, pgno: Pgno) -> Option<MemPageRef> {
    let p_pager = p_bt.borrow().p_pager.clone().expect("BtShared sem pager");
    let p_db_page = pager_lookup(&p_pager.borrow(), pgno);
    p_db_page.map(|d| btree_page_from_db_page(&d, pgno, p_bt))
}

/// Devolve o tamanho do arquivo de banco de dados em páginas. Em caso de qualquer
/// erro, devolve ((unsigned int)-1).
fn btree_pagecount(p_bt: &BtSharedRef) -> Pgno {
    p_bt.borrow().n_page
}

/// `sqlite3BtreeLastPage`.
pub fn btree_last_page(p: &BtreeRef) -> Pgno {
    let p_bt = p.borrow().p_bt.clone().expect("Btree sem BtShared");
    btree_pagecount(&p_bt)
}

/// Obtém uma página do paginador e a inicializa.
fn get_and_init_page(
    p_bt: &BtSharedRef,
    pgno: Pgno,
    pp_page: &mut Option<MemPageRef>,
    b_read_only: i32,
) -> i32 {
    if pgno > btree_pagecount(p_bt) {
        *pp_page = None;
        return sqlite_corrupt_bkpt(line!() as i32);
    }
    let mut p_db_page: Option<PgHdrRef> = None;
    let p_pager = p_bt.borrow().p_pager.clone().expect("BtShared sem pager");
    let mut rc = pager_get(&mut p_pager.borrow_mut(), pgno, &mut p_db_page, b_read_only);
    if rc != 0 {
        *pp_page = None;
        return rc;
    }
    let p_db_page = p_db_page.expect("pager_get sem página");
    let p_page = pager_get_mem_page(&p_db_page);
    if p_page.borrow().is_init == 0 {
        btree_page_from_db_page(&p_db_page, pgno, p_bt);
        rc = btree_init_page(&mut p_page.borrow_mut());
        if rc != SQLITE_OK {
            release_page(Some(&p_page));
            *pp_page = None;
            return rc;
        }
    }
    *pp_page = Some(p_page);
    SQLITE_OK
}

/// Libera uma MemPage. Deve ser chamada uma vez para cada chamada anterior a
/// btree_get_page.
///
/// A página 1 é um caso especial e precisa ser liberada com release_page_one().
fn release_page_not_null(p_page: &MemPageRef) {
    let p_db_page = p_page.borrow().p_db_page.clone().expect("MemPage sem DbPage");
    pager_unref_not_null(&p_db_page);
}

fn release_page(p_page: Option<&MemPageRef>) {
    if let Some(p) = p_page {
        release_page_not_null(p);
    }
}

fn release_page_one(p_page: &MemPageRef) {
    let p_db_page = p_page.borrow().p_db_page.clone().expect("MemPage sem DbPage");
    pager_unref_page_one(&p_db_page);
}

/// Obtém uma página não usada.
///
/// Funciona como btree_get_page() com o acréscimo de:
///
///   *  se a página já está em uso para outro fim, libera-a de imediato e devolve
///      um erro SQLITE_CORRUPT;
///   *  garantir que a flag is_init esteja limpa.
fn btree_get_unused_page(
    p_bt: &BtSharedRef,
    pgno: Pgno,
    pp_page: &mut Option<MemPageRef>,
    flags: i32,
) -> i32 {
    let rc = btree_get_page(p_bt, pgno, pp_page, flags);
    if rc == SQLITE_OK {
        let p_page = pp_page.clone().expect("btree_get_page sem página");
        let p_db_page = p_page.borrow().p_db_page.clone().expect("MemPage sem DbPage");
        if pager_page_refcount(&p_db_page) > 1 {
            release_page(Some(&p_page));
            *pp_page = None;
            return sqlite_corrupt_bkpt(line!() as i32);
        }
        p_page.borrow_mut().is_init = 0;
    } else {
        *pp_page = None;
    }
    rc
}

/// Num rollback, quando o paginador recarrega informação no cache para que ele volte
/// ao estado original do início da transação, esta rotina é chamada para cada página
/// restaurada.
///
/// A rotina precisa reiniciar a seção de dados extras no fim da página para
/// concordar com os dados restaurados.
fn page_reinit(p_data: &PgHdrRef) {
    let p_page = pager_get_mem_page(p_data);
    if p_page.borrow().is_init != 0 {
        p_page.borrow_mut().is_init = 0;
        if pager_page_refcount(p_data) > 1 {
            // p_page pode não ser uma página de btree: pode ser uma página de
            // overflow, de ptrmap ou livre. Nesses casos a chamada a
            // btree_init_page() abaixo provavelmente devolve SQLITE_CORRUPT, mas
            // isso não causa dano. E é muito importante que btree_init_page() seja
            // chamada em toda página de btree, então a chamada é feita para toda
            // página que chega para reinicialização.
            btree_init_page(&mut p_page.borrow_mut());
        }
    }
}

/// Invoca o tratador de ocupado (busy handler) de um btree.
fn btree_invoke_busy_handler(p_arg: &std::rc::Weak<std::cell::RefCell<BtShared>>) -> i32 {
    let p_bt = p_arg.upgrade().expect("BtShared liberado");
    let db = p_bt
        .borrow()
        .db
        .as_ref()
        .and_then(|w| w.upgrade())
        .expect("BtShared sem conexão");
    let rc = invoke_busy_handler(&mut db.borrow_mut().busy_handler);
    rc
}

/// Abre um arquivo de banco de dados.
///
/// z_filename é o nome do arquivo. Se for None, cria-se um banco de dados efêmero,
/// que pode ficar só em memória ou usar um cache em memória baseado em disco. De
/// qualquer modo, ele é apagado automaticamente quando btree_close() é chamada.
///
/// Se z_filename é ":memory:", cria-se um banco de dados em memória, destruído
/// automaticamente ao ser fechado.
///
/// O parâmetro flags é uma máscara que pode conter bits como BTREE_OMIT_JOURNAL
/// e/ou BTREE_MEMORY.
///
/// Se o banco de dados já está aberto na mesma conexão e estamos em modo de cache
/// compartilhado, a abertura falha com SQLITE_CONSTRAINT. Não se pode permitir dois
/// ou mais objetos BtShared na mesma conexão, pois isso causa problemas de bloqueio.
pub fn btree_open(
    p_vfs: std::rc::Rc<dyn Vfs>,
    z_filename: Option<&[u8]>,
    db: &Sqlite3Ref,
    pp_btree: &mut Option<BtreeRef>,
    mut flags: u32,
    mut vfs_flags: i32,
) -> i32 {
    use std::cell::RefCell;
    use std::rc::Rc;
    let mut p_bt: Option<BtSharedRef> = None; // parte compartilhada da estrutura btree
    let mut mutex_open: Option<Rc<sqlite3_mutex>> = None; // evita corrida (ticket #3537)
    let mut rc = SQLITE_OK; // código de resultado desta função
    let mut n_reserve: u8; // byte de espaço não usado em cada página
    let mut z_db_header = [0u8; 100]; // conteúdo do cabeçalho do banco de dados
    let mut p_pager_box: Option<Box<Pager>> = None;

    // Verdadeiro se abre um banco de dados efêmero e temporário
    let is_temp_db = match z_filename {
        None => true,
        Some(f) => f.is_empty() || f[0] == 0,
    };

    // is_memdb é verdadeiro para banco de dados em memória, falso para em arquivo.
    let is_memdb = z_filename.map_or(false, |f| f == b":memory:")
        || (is_temp_db && temp_in_memory(&db.borrow()) != 0)
        || (vfs_flags & SQLITE_OPEN_MEMORY) != 0;

    if is_memdb {
        flags |= BTREE_MEMORY;
    }
    if (vfs_flags & SQLITE_OPEN_MAIN_DB) != 0 && (is_memdb || is_temp_db) {
        vfs_flags = (vfs_flags & !SQLITE_OPEN_MAIN_DB) | SQLITE_OPEN_TEMP_DB;
    }
    let p: BtreeRef = Rc::new(RefCell::new(Btree::default()));
    {
        let mut pb = p.borrow_mut();
        pb.in_trans = TRANS_NONE;
        pb.db = Some(Rc::downgrade(db));
        pb.lock.p_btree = Some(Rc::downgrade(&p));
        pb.lock.i_table = 1;
    }

    // Se este Btree é candidato a cache compartilhado, tenta achar um BtShared
    // existente para compartilhar.
    if !is_temp_db && (!is_memdb || (vfs_flags & SQLITE_OPEN_URI) != 0) {
        if (vfs_flags & SQLITE_OPEN_SHAREDCACHE) != 0 {
            let z_name = z_filename.expect("nome ausente fora de banco temporário");
            let n_filename = strlen30(Some(z_name)) + 1;
            let n_full_pathname = p_vfs.mx_pathname() + 1;
            let mut z_full_pathname = vec![0u8; std::cmp::max(n_full_pathname, n_filename) as usize];

            p.borrow_mut().sharable = 1;
            if is_memdb {
                z_full_pathname[..n_filename as usize - 1].copy_from_slice(&z_name[..n_filename as usize - 1]);
            } else {
                rc = os_full_pathname(&*p_vfs, z_name, &mut z_full_pathname);
                if rc != 0 {
                    if rc == SQLITE_OK_SYMLINK {
                        rc = SQLITE_OK;
                    } else {
                        return rc;
                    }
                }
            }
            mutex_open = mutex_alloc(SQLITE_MUTEX_STATIC_OPEN);
            mutex_enter(mutex_open.as_deref());
            let mutex_shared = mutex_alloc(SQLITE_MUTEX_STATIC_MAIN);
            mutex_enter(mutex_shared.as_deref());
            let nul = |s: &[u8]| s.iter().position(|&c| c == 0).unwrap_or(s.len());
            let mut cur = SHARED_CACHE_LIST.with(|l| l.borrow().clone());
            while let Some(bt) = cur {
                let (same_name, same_vfs) = {
                    let b = bt.borrow();
                    let pager = b.p_pager.as_ref().expect("BtShared sem pager").borrow();
                    let fname = pager_filename(&pager, 0);
                    (
                        z_full_pathname[..nul(&z_full_pathname)] == fname[..nul(fname)],
                        pager.p_vfs.as_ref().map_or(false, |v| Rc::ptr_eq(v, &p_vfs)),
                    )
                };
                if same_name && same_vfs {
                    let n_db = db.borrow().n_db;
                    for i_db in (0..n_db).rev() {
                        let p_existing = db.borrow().a_db[i_db as usize].p_bt.clone();
                        if let Some(e) = p_existing {
                            let e_bt = e.borrow().p_bt.clone();
                            if e_bt.map_or(false, |x| Rc::ptr_eq(&x, &bt)) {
                                mutex_leave(mutex_shared.as_deref());
                                mutex_leave(mutex_open.as_deref());
                                return SQLITE_CONSTRAINT;
                            }
                        }
                    }
                    p.borrow_mut().p_bt = Some(bt.clone());
                    bt.borrow_mut().n_ref += 1;
                    p_bt = Some(bt.clone());
                    break;
                }
                cur = bt.borrow().p_next.clone();
            }
            mutex_leave(mutex_shared.as_deref());
        }
    }
    let mut p_bt_new: Option<BtSharedRef> = None; // BtShared criado nesta chamada
    // btree_open_out: o bloco abaixo sai por `break 'btree_open_out` nos erros.
    'btree_open_out: {
        if p_bt.is_none() {
            // Suprime falso positivo do PVS-Studio: zera zDbHeader[16..24]
            z_db_header[16..24].fill(0);

            let bt: BtSharedRef = Rc::new(RefCell::new(BtShared::default()));
            rc = pager_open(
                p_vfs.clone(),
                &mut p_pager_box,
                z_filename,
                0, // n_extra: o MemPage não mora mais nos bytes extras da página
                flags as i32,
                vfs_flags,
                None, // x_reinit: page_reinit é ligada pelo módulo do paginador
            );
            if rc == SQLITE_OK {
                let pager = p_pager_box.as_mut().expect("pager_open sem pager");
                pager_set_mmap_limit(pager, db.borrow().sz_mmap);
                rc = pager_read_fileheader(pager, z_db_header.len() as i32, &mut z_db_header);
            }
            if rc != SQLITE_OK {
                break 'btree_open_out;
            }
            {
                let mut b = bt.borrow_mut();
                b.open_flags = flags as u8;
                b.db = Some(Rc::downgrade(db));
            }
            {
                let weak = Rc::downgrade(&bt);
                let pager = p_pager_box.as_mut().expect("pager_open sem pager");
                pager_set_busy_handler(pager, Some(Rc::new(move || btree_invoke_busy_handler(&weak))));
            }
            p.borrow_mut().p_bt = Some(bt.clone());

            {
                let mut b = bt.borrow_mut();
                b.p_cursor = None;
                b.p_page_1 = None;
                if pager_isreadonly(p_pager_box.as_ref().expect("pager_open sem pager")) != 0 {
                    b.bts_flags |= BTS_READ_ONLY;
                }
                // SQLITE_SECURE_DELETE está ligado no Debian 13
                b.bts_flags |= BTS_SECURE_DELETE;
                // EVIDENCE-OF: R-51873-39618 o tamanho de página do arquivo é o
                // inteiro de 2 bytes no deslocamento 16 a partir do começo do arquivo.
                b.page_size = ((z_db_header[16] as u32) << 8) | ((z_db_header[17] as u32) << 16);
                if b.page_size < 512
                    || b.page_size > SQLITE_MAX_PAGE_SIZE as u32
                    || ((b.page_size - 1) & b.page_size) != 0
                {
                    b.page_size = 0;
                    // Se o nome mágico ":memory:" cria um banco em memória, deixa
                    // auto_vacuum em 0 mesmo que SQLITE_DEFAULT_AUTOVACUUM seja
                    // verdadeiro (no Debian ele é 0).
                    if z_filename.is_some() && !is_memdb {
                        b.auto_vacuum = 0;
                        b.incr_vacuum = 0;
                    }
                    n_reserve = 0;
                } else {
                    // EVIDENCE-OF: R-37497-42412 o tamanho da região reservada é o
                    // inteiro de 1 byte sem sinal no deslocamento 20 do cabeçalho.
                    n_reserve = z_db_header[20];
                    b.bts_flags |= BTS_PAGESIZE_FIXED;
                    b.auto_vacuum = if get4byte(&z_db_header[36 + 4 * 4..]) != 0 { 1 } else { 0 };
                    b.incr_vacuum = if get4byte(&z_db_header[36 + 7 * 4..]) != 0 { 1 } else { 0 };
                }
                rc = pager_set_pagesize(
                    p_pager_box.as_mut().expect("pager_open sem pager"),
                    &mut b.page_size,
                    n_reserve as i32,
                );
                if rc != 0 {
                    break 'btree_open_out;
                }
                b.usable_size = b.page_size - n_reserve as u32;
            }

            // Acrescenta o novo BtShared à lista encadeada de BtShared compartilháveis.
            {
                let pager = p_pager_box.take().expect("pager_open sem pager");
                bt.borrow_mut().p_pager = Some(Rc::new(RefCell::new(*pager)));
            }
            p_bt_new = Some(bt.clone());
            bt.borrow_mut().n_ref = 1;
            if p.borrow().sharable != 0 {
                // SQLITE_THREADSAFE=1
                if with_global_config(|cfg| cfg.b_core_mutex) != 0 {
                    bt.borrow_mut().mutex = mutex_alloc(SQLITE_MUTEX_FAST);
                    if bt.borrow().mutex.is_none() {
                        rc = SQLITE_NOMEM_BKPT;
                        break 'btree_open_out;
                    }
                }
                let mutex_shared = mutex_alloc(SQLITE_MUTEX_STATIC_MAIN);
                mutex_enter(mutex_shared.as_deref());
                bt.borrow_mut().p_next = SHARED_CACHE_LIST.with(|l| l.borrow().clone());
                SHARED_CACHE_LIST.with(|l| *l.borrow_mut() = Some(bt.clone()));
                mutex_leave(mutex_shared.as_deref());
            }
            p_bt = Some(bt);
        }

        // Se o novo Btree usa um BtShared compartilhável, liga o novo Btree na lista
        // de todos os Btrees compartilháveis da mesma conexão. A lista é mantida em
        // ordem crescente do endereço de p_bt.
        if p.borrow().sharable != 0 {
            let addr = |b: &BtreeRef| -> usize {
                Rc::as_ptr(b.borrow().p_bt.as_ref().expect("Btree sem BtShared")) as *const () as usize
            };
            let n_db = db.borrow().n_db;
            for i in 0..n_db {
                let sib0 = db.borrow().a_db[i as usize].p_bt.clone();
                if let Some(mut p_sib) = sib0 {
                    if p_sib.borrow().sharable == 0 {
                        continue;
                    }
                    loop {
                        let prev = p_sib.borrow().p_prev.as_ref().and_then(|w| w.upgrade());
                        match prev {
                            Some(pp) => p_sib = pp,
                            None => break,
                        }
                    }
                    if addr(&p) < addr(&p_sib) {
                        p.borrow_mut().p_next = Some(p_sib.clone());
                        p.borrow_mut().p_prev = None;
                        p_sib.borrow_mut().p_prev = Some(Rc::downgrade(&p));
                    } else {
                        loop {
                            let next = p_sib.borrow().p_next.clone();
                            match next {
                                Some(n) if addr(&n) < addr(&p) => p_sib = n,
                                _ => break,
                            }
                        }
                        let next = p_sib.borrow().p_next.clone();
                        p.borrow_mut().p_next = next.clone();
                        p.borrow_mut().p_prev = Some(Rc::downgrade(&p_sib));
                        if let Some(n) = next {
                            n.borrow_mut().p_prev = Some(Rc::downgrade(&p));
                        }
                        p_sib.borrow_mut().p_next = Some(p.clone());
                    }
                    break;
                }
            }
        }
        *pp_btree = Some(p.clone());
    }

    // btree_open_out:
    if rc != SQLITE_OK {
        if let Some(pager) = p_pager_box.take() {
            pager_close(pager, None);
        } else if let Some(bt) = p_bt_new.as_ref() {
            // O BtShared já guardava o pager: fecha-o quando for o único dono.
            let p_pager = bt.borrow_mut().p_pager.take();
            if let Some(pg) = p_pager.and_then(|r| Rc::try_unwrap(r).ok()) {
                pager_close(Box::new(pg.into_inner()), None);
            }
        }
        *pp_btree = None;
    } else {
        // Se o B-Tree foi aberto, põe o tamanho do cache do paginador no valor
        // padrão. Exceto ao abrir sobre um cache de paginador compartilhado
        // existente: aí o tamanho não muda.
        if btree_schema(p.clone(), 0, None).is_none() {
            btree_set_cache_size(&mut p.borrow_mut(), SQLITE_DEFAULT_CACHE_SIZE);
        }
        let p_bt_ref = p_bt.as_ref().expect("BtShared ausente após abrir");
        let p_pager = p_bt_ref.borrow().p_pager.clone().expect("BtShared sem pager");
        let mut pager = p_pager.borrow_mut();
        if let Some(p_file) = pager.fd.as_mut() {
            if p_file.p_methods.is_some() {
                os_file_control_hint(p_file, SQLITE_FCNTL_PDB, None);
            }
        }
    }
    if mutex_open.is_some() {
        mutex_leave(mutex_open.as_deref());
    }
    rc
}


// ---- part_007.rs ----

// Convenções deste trecho: as asserções do C somem (o Debian compila sem
// SQLITE_DEBUG), assim como o laço de verificação de cursores sob SQLITE_DEBUG em
// btree_close. Um `Btree *p` vira `&mut Btree` (como em btree_enter/btree_leave);
// um `BtShared *` vira `&BtSharedRef`. `SQLITE_MAX_MMAP_SIZE>0` vale no Debian, então
// btree_set_mmap_limit existe. `p_tmp_space` nulo é o `Vec` vazio; quando alocado,
// tem `page_size` bytes e o ponteiro lógico `pTmpSpace` do C é o índice 4.

/// Decrementa o contador BtShared.n_ref. Quando chega a zero, tira a estrutura
/// BtShared da lista de compartilhamento. Devolve verdadeiro (1) se o contador chegou
/// a zero e falso (0) se ainda é positivo.
fn remove_from_sharing_list(p_bt: &BtSharedRef) -> i32 {
    let mut removed = 0;
    let p_main_mtx = mutex_alloc(SQLITE_MUTEX_STATIC_MAIN);
    mutex_enter(p_main_mtx.as_deref());
    p_bt.borrow_mut().n_ref -= 1;
    if p_bt.borrow().n_ref <= 0 {
        let head = SHARED_CACHE_LIST.with(|l| l.borrow().clone());
        let head_is_bt = head.as_ref().map_or(false, |h| std::rc::Rc::ptr_eq(h, p_bt));
        if head_is_bt {
            let next = p_bt.borrow().p_next.clone();
            SHARED_CACHE_LIST.with(|l| *l.borrow_mut() = next);
        } else {
            let mut p_list = head;
            while let Some(l) = p_list.clone() {
                let next = l.borrow().p_next.clone();
                if next.as_ref().map_or(false, |n| std::rc::Rc::ptr_eq(n, p_bt)) {
                    break;
                }
                p_list = next;
            }
            if let Some(l) = p_list {
                let next = p_bt.borrow().p_next.clone();
                l.borrow_mut().p_next = next;
            }
        }
        // SQLITE_THREADSAFE=1
        let mtx = p_bt.borrow_mut().mutex.take();
        mutex_free(mtx);
        removed = 1;
    }
    mutex_leave(p_main_mtx.as_deref());
    removed
}

/// Garante que p_bt.p_tmp_space aponta para uma alocação de MX_CELL_SIZE(pBt) bytes
/// com um prefixo de 4 bytes para um ponteiro de filho esquerdo.
fn allocate_temp_space(p_bt: &mut BtShared) -> i32 {
    // Esta rotina só é chamada por btree_cursor() ao alocar o primeiro cursor de
    // escrita do objeto BtShared. A alocação não falha em Rust, então o ramo
    // SQLITE_NOMEM (que desligava o cursor) não existe.
    //
    // Um dos usos de p_tmp_space é formatar células antes de inseri-las numa
    // folha (fill_in_cell()). Uma célula de menos de 4 bytes é arredondada para 4,
    // o que pode deixar bytes não inicializados indo para o disco. Por isso os
    // primeiros 8 bytes são zerados (`vec!` já zera tudo), com 4 bytes
    // inicializados antes do início lógico de p_tmp_space (índice 4) para
    // prefixar o ponteiro de filho esquerdo.
    p_bt.p_tmp_space = vec![0u8; p_bt.page_size as usize];
    SQLITE_OK
}

/// Libera a alocação p_bt.p_tmp_space.
fn free_temp_space(p_bt: &mut BtShared) {
    if !p_bt.p_tmp_space.is_empty() {
        p_bt.p_tmp_space = Vec::new();
    }
}

/// Fecha um banco de dados aberto e invalida todos os cursores.
pub fn btree_close(p: &mut Btree) -> i32 {
    let p_bt: BtSharedRef = p.p_bt.clone().expect("Btree sem BtShared");

    // Fecha todos os cursores abertos por este handle.
    btree_enter(p);

    // Desfaz qualquer transação ativa e libera a estrutura do handle. A chamada a
    // btree_rollback() larga os bloqueios de tabela mantidos por este handle.
    btree_rollback(p, SQLITE_OK, 0);
    btree_leave(p);

    // Se ainda há outras referências à estrutura de btree compartilhada, retorna
    // agora. O resto do procedimento limpa a btree compartilhada.
    if p.sharable == 0 || remove_from_sharing_list(&p_bt) != 0 {
        // p_bt já não está na lista de compartilhamento, então pode ser acessado
        // sem segurar o mutex. Limpa e apaga o objeto BtShared.
        let p_pager = p_bt.borrow_mut().p_pager.take();
        let db = p.db.as_ref().and_then(|w| w.upgrade());
        if let Some(pg) = p_pager.and_then(|r| std::rc::Rc::try_unwrap(r).ok()) {
            pager_close(Box::new(pg.into_inner()), db.as_ref());
        }
        let (x_free_schema, p_schema) = {
            let b = p_bt.borrow();
            (b.x_free_schema, b.p_schema.clone())
        };
        if let (Some(x_free), Some(schema)) = (x_free_schema, p_schema) {
            x_free(schema);
        }
        p_bt.borrow_mut().p_schema = None;
        free_temp_space(&mut p_bt.borrow_mut());
    }

    if let Some(prev) = p.p_prev.as_ref().and_then(|w| w.upgrade()) {
        prev.borrow_mut().p_next = p.p_next.clone();
    }
    if let Some(next) = p.p_next.as_ref() {
        next.borrow_mut().p_prev = p.p_prev.clone();
    }
    SQLITE_OK
}

/// Muda o limite "suave" do número de páginas no cache. Páginas não usadas e não
/// modificadas são recicladas quando o número de páginas no cache passa deste limite.
/// O cache pode crescer além dele se contiver páginas sujas ou ainda em uso ativo.
pub fn btree_set_cache_size(p: &mut Btree, mx_page: i32) -> i32 {
    let p_bt = p.p_bt.clone().expect("Btree sem BtShared");
    btree_enter(p);
    let p_pager = p_bt.borrow().p_pager.clone().expect("BtShared sem pager");
    pager_set_cachesize(&mut p_pager.borrow_mut(), mx_page);
    btree_leave(p);
    SQLITE_OK
}

/// Muda o limite de "spill" do número de páginas no cache. Se o número de páginas
/// passa deste limite durante uma transação de escrita, o paginador pode tentar
/// descarregar ("spill") páginas no journal cedo para liberar memória.
///
/// O valor devolvido é o tamanho de spill atual. Se o argumento é zero, nada muda,
/// então mx_page 0 serve para consultar o tamanho atual.
pub fn btree_set_spill_size(p: &mut Btree, mx_page: i32) -> i32 {
    let p_bt = p.p_bt.clone().expect("Btree sem BtShared");
    btree_enter(p);
    let p_pager = p_bt.borrow().p_pager.clone().expect("BtShared sem pager");
    let res = pager_set_spillsize(&mut p_pager.borrow_mut(), mx_page);
    btree_leave(p);
    res
}

/// Muda o limite da parte do arquivo de banco de dados que pode ser mapeada em memória.
pub fn btree_set_mmap_limit(p: &mut Btree, sz_mmap: i64) -> i32 {
    let p_bt = p.p_bt.clone().expect("Btree sem BtShared");
    btree_enter(p);
    let p_pager = p_bt.borrow().p_pager.clone().expect("BtShared sem pager");
    pager_set_mmap_limit(&mut p_pager.borrow_mut(), sz_mmap);
    btree_leave(p);
    SQLITE_OK
}

/// Muda a forma como os dados são sincronizados com o disco para aumentar ou diminuir
/// a resistência do banco a danos por quedas do SO e falhas de energia. O nível 1 é o
/// mesmo que assíncrono (não há sync e a chance de dano é alta). O nível 2 é o padrão
/// (chance de dano muito baixa, mas não nula). O nível 3 reduz a chance de dano a
/// quase zero, com perda de desempenho de escrita.
pub fn btree_set_pager_flags(p: &mut Btree, pg_flags: u32) -> i32 {
    let p_bt = p.p_bt.clone().expect("Btree sem BtShared");
    btree_enter(p);
    let p_pager = p_bt.borrow().p_pager.clone().expect("BtShared sem pager");
    pager_set_flags(&mut p_pager.borrow_mut(), pg_flags);
    btree_leave(p);
    SQLITE_OK
}

/// Muda o tamanho de página padrão e o número de bytes reservados por página. Ou, se
/// o tamanho de página já foi fixado, devolve SQLITE_READONLY sem mudar nada.
///
/// O tamanho de página deve ser potência de 2 entre 512 e 65536. Se o valor dado não
/// respeita isso, o tamanho de página não muda.
///
/// Os tamanhos são limitados a potências de dois para que a região do arquivo usada
/// para bloqueio (a partir de PENDING_BYTE, o primeiro byte depois da fronteira de
/// 1GB, 0x40000000) caia no começo de uma página.
///
/// Se n_reserve é menor que zero, o número de bytes reservados por página não muda.
///
/// Se i_fix != 0, a flag BTS_PAGESIZE_FIXED é ligada, de modo que o tamanho de página
/// e o modo de autovacuum não possam mais ser mudados.
pub fn btree_set_page_size(p: &mut Btree, mut page_size: i32, mut n_reserve: i32, i_fix: i32) -> i32 {
    let p_bt = p.p_bt.clone().expect("Btree sem BtShared");
    btree_enter(p);
    p_bt.borrow_mut().n_reserve_wanted = n_reserve as u8;
    let x: i32 = (p_bt.borrow().page_size as i32).wrapping_sub(p_bt.borrow().usable_size as i32);
    if n_reserve < x {
        n_reserve = x;
    }
    if (p_bt.borrow().bts_flags & BTS_PAGESIZE_FIXED) != 0 {
        btree_leave(p);
        return SQLITE_READONLY;
    }
    if page_size >= 512 && page_size <= SQLITE_MAX_PAGE_SIZE && ((page_size - 1) & page_size) == 0 {
        if n_reserve > 32 && page_size == 512 {
            page_size = 1024;
        }
        p_bt.borrow_mut().page_size = page_size as u32;
        free_temp_space(&mut p_bt.borrow_mut());
    }
    let p_pager = p_bt.borrow().p_pager.clone().expect("BtShared sem pager");
    let mut new_page_size = p_bt.borrow().page_size;
    let rc = pager_set_pagesize(&mut p_pager.borrow_mut(), &mut new_page_size, n_reserve);
    {
        let mut b = p_bt.borrow_mut();
        b.page_size = new_page_size;
        b.usable_size = b.page_size.wrapping_sub((n_reserve as u16) as u32);
        if i_fix != 0 {
            b.bts_flags |= BTS_PAGESIZE_FIXED;
        }
    }
    btree_leave(p);
    rc
}

/// Devolve o tamanho de página atualmente definido.
pub fn btree_get_page_size(p: &Btree) -> i32 {
    p.p_bt.as_ref().expect("Btree sem BtShared").borrow().page_size as i32
}

/// Parecida com btree_get_reserve(), mas só pode ser chamada quando se garante que o
/// mutex da b-tree já está seguro.
///
/// É útil num caso especial no código da API de backup, em que se sabe que o mutex da
/// b-tree compartilhada está seguro, mas o mutex do handle de banco de dados dono de
/// *p não está. Nesse caso, chamar btree_enter() poderia colidir com outra operação
/// no handle dono de *p, causando comportamento indefinido.
pub fn btree_get_reserve_no_mutex(p: &Btree) -> i32 {
    let b = p.p_bt.as_ref().expect("Btree sem BtShared").borrow();
    b.page_size.wrapping_sub(b.usable_size) as i32
}

/// Devolve o número de bytes de espaço no fim de cada página deixados sem uso de
/// propósito. É o espaço "reservado", às vezes usado por extensões.
///
/// O valor devolvido é o maior entre o tamanho de reserva atual e o último pedido por
/// SQLITE_FILECTRL_RESERVE_BYTES. A reserva só pode crescer, nunca diminuir.
pub fn btree_get_requested_reserve(p: &mut Btree) -> i32 {
    btree_enter(p);
    let n1: i32 = p.p_bt.as_ref().expect("Btree sem BtShared").borrow().n_reserve_wanted as i32;
    let n2: i32 = btree_get_reserve_no_mutex(p);
    btree_leave(p);
    if n1 > n2 { n1 } else { n2 }
}

/// Define a contagem máxima de páginas do banco de dados se mx_page é positivo. Nada
/// muda se mx_page é 0 ou negativo. Qualquer que seja mx_page, devolve a contagem
/// máxima de páginas.
pub fn btree_max_page_count(p: &mut Btree, mx_page: Pgno) -> Pgno {
    let p_bt = p.p_bt.clone().expect("Btree sem BtShared");
    btree_enter(p);
    let p_pager = p_bt.borrow().p_pager.clone().expect("BtShared sem pager");
    let n = pager_max_page_count(&mut p_pager.borrow_mut(), mx_page);
    btree_leave(p);
    n
}

/// Muda os valores das flags BTS_SECURE_DELETE e BTS_OVERWRITE:
///
///    new_flag == 0       BTS_SECURE_DELETE e BTS_OVERWRITE são limpas
///    new_flag == 1       BTS_SECURE_DELETE ligada e BTS_OVERWRITE limpa
///    new_flag == 2       BTS_SECURE_DELETE limpa e BTS_OVERWRITE ligada
///    new_flag == (-1)    nenhuma mudança
///
/// A rotina age como consulta se new_flag é menor que zero.
///
/// Com BTS_OVERWRITE ligada, o conteúdo apagado é sobrescrito com zeros, mas as
/// páginas folha da lista livre não são gravadas de volta no banco. Assim o conteúdo
/// apagado dentro da página é limpo, mas o da lista livre não.
///
/// Com BTS_SECURE_DELETE, a operação é como BTS_OVERWRITE, com o acréscimo de que as
/// páginas folha da lista livre são gravadas de volta no banco, aumentando a E/S.
pub fn btree_secure_delete(p: Option<&mut Btree>, new_flag: i32) -> i32 {
    let p = match p {
        Some(p) => p,
        None => return 0,
    };
    let p_bt = p.p_bt.clone().expect("Btree sem BtShared");
    btree_enter(p);
    if new_flag >= 0 {
        let mut b = p_bt.borrow_mut();
        b.bts_flags &= !BTS_FAST_SECURE;
        b.bts_flags |= BTS_SECURE_DELETE.wrapping_mul(new_flag as u16);
    }
    let b = ((p_bt.borrow().bts_flags & BTS_FAST_SECURE) / BTS_SECURE_DELETE) as i32;
    btree_leave(p);
    b
}


// ---- part_008.rs ----

/// Altera a propriedade de "auto-vacuum" do banco de dados. Se o parâmetro
/// `auto_vacuum` for diferente de zero, o modo auto-vacuum é ativado. Se for
/// zero, é desativado. O valor padrão da propriedade é determinado pela macro
/// `SQLITE_DEFAULT_AUTOVACUUM`.
pub fn btree_set_auto_vacuum(p: &mut Btree, auto_vacuum: i32) -> i32 {
    let p_bt = p.p_bt.clone().expect("Btree.p_bt");
    let mut rc = SQLITE_OK;
    let av = auto_vacuum as u8;

    btree_enter(p);
    {
        let mut bt = p_bt.borrow_mut();
        let av_bit: u8 = if av != 0 { 1 } else { 0 };
        if (bt.bts_flags & BTS_PAGESIZE_FIXED) != 0 && av_bit != bt.auto_vacuum {
            rc = SQLITE_READONLY;
        } else {
            bt.auto_vacuum = av_bit;
            bt.incr_vacuum = if av == 2 { 1 } else { 0 };
        }
    }
    btree_leave(p);
    rc
}

/// Retorna o valor da propriedade de "auto-vacuum". Se o auto-vacuum estiver
/// ativado, retorna 1. Caso contrário, 0.
pub fn btree_get_auto_vacuum(p: &mut Btree) -> i32 {
    btree_enter(p);
    let rc = {
        let bt = p.p_bt.as_ref().expect("Btree.p_bt").borrow();
        if bt.auto_vacuum == 0 {
            BTREE_AUTOVACUUM_NONE as i32
        } else if bt.incr_vacuum == 0 {
            BTREE_AUTOVACUUM_FULL as i32
        } else {
            BTREE_AUTOVACUUM_INCR as i32
        }
    };
    btree_leave(p);
    rc
}

// `setDefaultSyncFlag`: no Debian 13 `SQLITE_DEFAULT_SYNCHRONOUS` e
// `SQLITE_DEFAULT_WAL_SYNCHRONOUS` valem os dois 2, então a condição
// `SQLITE_DEFAULT_SYNCHRONOUS!=SQLITE_DEFAULT_WAL_SYNCHRONOUS` é falsa e a macro
// `setDefaultSyncFlag(pBt,safety_level)` expande para nada: a função não existe
// e as chamadas em `lock_btree` somem (os argumentos nem são avaliados).
//
// A declaração antecipada `static int newDatabase(BtShared*)` também não tem
// equivalente em Rust: `new_database` é definida mais abaixo neste módulo.

/// Obtém uma referência à página 1 do arquivo de banco de dados. Isto também
/// adquire um travamento de leitura no arquivo.
///
/// Retorna `SQLITE_OK` em caso de sucesso. Se o arquivo não for um banco de
/// dados bem formado, retorna `SQLITE_CORRUPT`. Retorna `SQLITE_BUSY` se o
/// banco estiver travado e `SQLITE_NOMEM` se faltar memória.
fn lock_btree(p_bt: &mut BtShared) -> i32 {
    let mut rc: i32; // Código de resultado das subfunções
    let mut p_page_1_slot: Option<MemPageRef> = None; // Página 1 do arquivo de banco de dados
    let mut n_page: u32; // Número de páginas do banco de dados
    let mut n_page_file: u32 = 0; // Número de páginas do arquivo de banco de dados

    debug_assert!(p_bt.p_page_1.is_none());
    let pager = p_bt.p_pager.clone().expect("BtShared.p_pager");
    rc = pager_shared_lock(&mut pager.borrow_mut());
    if rc != SQLITE_OK {
        return rc;
    }
    rc = btree_get_page(p_bt, 1, &mut p_page_1_slot, 0);
    if rc != SQLITE_OK {
        return rc;
    }
    let p_page_1: MemPageRef = p_page_1_slot.expect("btree_get_page devolveu a página 1");

    // Cópia do cabeçalho do arquivo (os 100 primeiros bytes da página 1). O C
    // lê direto de `pPage1->aData`; a página não muda até o fim da função, então
    // a cópia é equivalente e evita manter o empréstimo de `p_page_1` aberto
    // durante as chamadas que o liberam.
    let page1: Vec<u8> = p_page_1.borrow().a_data[..100].to_vec();

    // Faz algumas verificações para ajudar a garantir que o arquivo que abrimos
    // é mesmo um banco de dados válido.
    n_page = get4byte(&page1[28..]);
    let mut n_page_file_int: i32 = n_page_file as i32;
    pager_pagecount(&pager.borrow(), &mut n_page_file_int);
    n_page_file = n_page_file_int as u32;
    if n_page == 0 || page1[24..28] != page1[92..96] {
        n_page = n_page_file;
    }
    let db = p_bt
        .db
        .as_ref()
        .and_then(|w| w.upgrade())
        .expect("BtShared.db");
    if (db.borrow().flags & SQLITE_RESETDATABASE) != 0 {
        n_page = 0;
    }

    'page1_init_failed: {
        if n_page > 0 {
            let page_size: u32;
            let usable_size: u32;
            rc = SQLITE_NOTADB;
            // EVIDENCE-OF: R-43737-39999 Todo arquivo de banco de dados SQLite
            // válido começa com estes 16 bytes (em hexa): 53 51 4c 69 74 65 20
            // 66 6f 72 6d 61 74 20 33 00.
            if page1[0..16] != SQLITE_FILE_HEADER[..] {
                break 'page1_init_failed;
            }

            if page1[18] > 2 {
                p_bt.bts_flags |= BTS_READ_ONLY;
            }
            if page1[19] > 2 {
                break 'page1_init_failed;
            }

            // Se a versão de leitura for 2, este banco deve ser acessado em modo
            // WAL. Se o log ainda não estiver aberto, abre agora. Depois retorna
            // SQLITE_OK sem preencher BtShared.p_page_1. O chamador percebe isso e
            // chama esta função de novo. Isto é necessário porque a versão da
            // página 1 que está no buffer page1 pode não ser a mais recente: pode
            // haver uma mais nova no arquivo de log.
            if page1[19] == 2 && (p_bt.bts_flags & BTS_NO_WAL) == 0 {
                let mut is_open: i32 = 0;
                rc = pager_open_wal(&mut pager.borrow_mut(), Some(&mut is_open));
                if rc != SQLITE_OK {
                    break 'page1_init_failed;
                } else if is_open == 0 {
                    release_page_one(p_page_1);
                    return SQLITE_OK;
                }
                rc = SQLITE_NOTADB;
            }

            // EVIDENCE-OF: R-15465-20813 As frações máxima e mínima de carga
            // embutida e a fração de carga de folha devem ser 64, 32 e 32.
            //
            // O projeto original permitia que esses valores variassem, mas desde
            // a versão 3.6.0 exigimos que sejam fixos.
            if page1[21..24] != [0x40u8, 0x20, 0x20] {
                break 'page1_init_failed;
            }
            // EVIDENCE-OF: R-51873-39618 O tamanho de página de um arquivo de banco
            // de dados é determinado pelo inteiro de 2 bytes no deslocamento 16
            // a partir do início do arquivo.
            page_size = ((page1[16] as u32) << 8) | ((page1[17] as u32) << 16);
            // EVIDENCE-OF: R-25008-21688 O tamanho de uma página é uma potência de
            // dois entre 512 e 65536, inclusive.
            if (page_size.wrapping_sub(1) & page_size) != 0
                || page_size > SQLITE_MAX_PAGE_SIZE as u32
                || page_size <= 256
            {
                break 'page1_init_failed;
            }
            debug_assert!((page_size & 7) == 0);
            // EVIDENCE-OF: R-59310-51205 O tamanho do "espaço reservado", o inteiro
            // de 1 byte no deslocamento 20, é o número de bytes no fim de cada
            // página reservados para extensões.
            //
            // EVIDENCE-OF: R-37497-42412 O tamanho da região reservada é
            // determinado pelo inteiro sem sinal de um byte no deslocamento 20 do
            // cabeçalho do arquivo de banco de dados.
            usable_size = page_size - (page1[20] as u32);
            if page_size != p_bt.page_size {
                // Depois de ler a primeira página do banco supondo o tamanho
                // BtShared.page_size, descobrimos que o tamanho de página é, na
                // verdade, page_size. Destrava o banco, deixa p_bt.p_page_1 em zero
                // e retorna SQLITE_OK. O chamador chamará esta função de novo com o
                // tamanho de página correto.
                release_page_one(p_page_1);
                p_bt.usable_size = usable_size;
                p_bt.page_size = page_size;
                p_bt.bts_flags |= BTS_PAGESIZE_FIXED;
                free_temp_space(p_bt);
                rc = pager_set_pagesize(
                    &mut pager.borrow_mut(),
                    &mut p_bt.page_size,
                    (page_size - usable_size) as i32,
                );
                return rc;
            }
            if n_page > n_page_file {
                if writable_schema(&db.borrow()) == 0 {
                    rc = SQLITE_CORRUPT_BKPT;
                    break 'page1_init_failed;
                } else {
                    n_page = n_page_file;
                }
            }
            // EVIDENCE-OF: R-28312-64704 Porém, o tamanho utilizável não pode ser
            // menor que 480. Em outras palavras, se o tamanho de página é 512, o
            // espaço reservado não pode passar de 32.
            if usable_size < 480 {
                break 'page1_init_failed;
            }
            p_bt.bts_flags |= BTS_PAGESIZE_FIXED;
            p_bt.page_size = page_size;
            p_bt.usable_size = usable_size;
            p_bt.auto_vacuum = if get4byte(&page1[36 + 4 * 4..]) != 0 { 1 } else { 0 };
            p_bt.incr_vacuum = if get4byte(&page1[36 + 7 * 4..]) != 0 { 1 } else { 0 };
        }

        // max_local é a quantidade máxima de carga a guardar localmente numa
        // célula. Garante que seja pequena o bastante para que pelo menos
        // minFanout células caibam em uma página. Supomos um cabeçalho de página
        // de 10 bytes. Além da carga, a célula guarda:
        //     ponteiro de 2 bytes para a célula
        //     ponteiro de filho de 4 bytes
        //     valor nKey de 9 bytes
        //     valor nData de 4 bytes
        //     ponteiro de página de overflow de 4 bytes
        // Então uma célula é um ponteiro de 2 bytes, um cabeçalho de até 17
        // bytes, de 0 a N bytes de carga e um ponteiro opcional de 4 bytes para a
        // página de overflow.
        p_bt.max_local = (p_bt.usable_size.wrapping_sub(12).wrapping_mul(64) / 255).wrapping_sub(23) as u16;
        p_bt.min_local = (p_bt.usable_size.wrapping_sub(12).wrapping_mul(32) / 255).wrapping_sub(23) as u16;
        p_bt.max_leaf = p_bt.usable_size.wrapping_sub(35) as u16;
        p_bt.min_leaf = (p_bt.usable_size.wrapping_sub(12).wrapping_mul(32) / 255).wrapping_sub(23) as u16;
        if p_bt.max_local > 127 {
            p_bt.max1byte_payload = 127;
        } else {
            p_bt.max1byte_payload = p_bt.max_local as u8;
        }
        debug_assert!(p_bt.max_leaf as i32 + 23 <= mx_cell_size(p_bt.page_size));
        p_bt.p_page_1 = Some(p_page_1);
        p_bt.n_page = n_page;
        return SQLITE_OK;
    }

    // page1_init_failed:
    release_page_one(p_page_1);
    p_bt.p_page_1 = None;
    rc
}

// `countValidCursors` só existe sob `#ifndef NDEBUG` e só é chamada de dentro de
// `assert()`. O `sqliteInt.h` define `NDEBUG` quando `SQLITE_DEBUG` não está
// definido, então no Debian 13 a função não é compilada e não tem equivalente.

/// Se não há cursores pendentes e não estamos no meio de uma transação, mas
/// existe um travamento de leitura no banco, esta rotina solta a referência à
/// primeira página do arquivo, o que libera o travamento de leitura.
///
/// Se há uma transação em andamento, esta rotina não faz nada.
fn unlock_btree_if_unused(p_bt: &mut BtShared) {
    if p_bt.in_transaction == TRANS_NONE && p_bt.p_page_1.is_some() {
        let p_page_1 = p_bt.p_page_1.take().expect("BtShared.p_page_1");
        debug_assert!(!p_page_1.borrow().a_data.is_empty());
        release_page_one(p_page_1);
    }
}

/// Se `p_bt` aponta para um arquivo vazio, converte esse arquivo vazio em um
/// banco de dados novo inicializando a primeira página do banco.
fn new_database(p_bt: &mut BtShared) -> i32 {
    if p_bt.n_page > 0 {
        return SQLITE_OK;
    }
    let p_p1 = p_bt.p_page_1.clone().expect("BtShared.p_page_1");
    let p_db_page = p_p1.borrow().p_db_page.clone().expect("MemPage.p_db_page");
    let rc = pager_write(&p_db_page);
    if rc != 0 {
        return rc;
    }
    let mut p1 = p_p1.borrow_mut();
    {
        let data = &mut p1.a_data;
        data[0..16].copy_from_slice(&SQLITE_FILE_HEADER[..]);
        data[16] = ((p_bt.page_size >> 8) & 0xff) as u8;
        data[17] = ((p_bt.page_size >> 16) & 0xff) as u8;
        data[18] = 1;
        data[19] = 1;
        debug_assert!(p_bt.usable_size <= p_bt.page_size && p_bt.usable_size + 255 >= p_bt.page_size);
        data[20] = (p_bt.page_size - p_bt.usable_size) as u8;
        data[21] = 64;
        data[22] = 32;
        data[23] = 32;
        data[24..100].fill(0);
    }
    zero_page(&mut p1, (PTF_INTKEY | PTF_LEAF | PTF_LEAFDATA) as i32);
    p_bt.bts_flags |= BTS_PAGESIZE_FIXED;
    debug_assert!(p_bt.auto_vacuum == 1 || p_bt.auto_vacuum == 0);
    debug_assert!(p_bt.incr_vacuum == 1 || p_bt.incr_vacuum == 0);
    put4byte(&mut p1.a_data[36 + 4 * 4..], p_bt.auto_vacuum as u32);
    put4byte(&mut p1.a_data[36 + 7 * 4..], p_bt.incr_vacuum as u32);
    p_bt.n_page = 1;
    p1.a_data[31] = 1;
    SQLITE_OK
}


// ---- part_009.rs ----

/// Inicializa a primeira página do arquivo de banco de dados (criando um banco
/// de dados de uma única página e sem objetos de esquema). Retorna `SQLITE_OK`
/// em caso de sucesso, ou um código de erro do SQLite caso contrário.
pub fn btree_new_db(p: &mut Btree) -> i32 {
    btree_enter(p);
    let p_bt = p.p_bt.clone().expect("Btree.p_bt");
    let rc = {
        let mut bt = p_bt.borrow_mut();
        bt.n_page = 0;
        new_database(&mut bt)
    };
    btree_leave(p);
    rc
}

/// Tenta iniciar uma transação nova. Uma transação de escrita é iniciada se o
/// segundo argumento for diferente de zero; caso contrário, uma transação de
/// leitura. Se o segundo argumento for 2 ou mais, inicia-se uma transação
/// exclusiva, o que significa que nenhum outro processo pode acessar o banco de
/// dados. Uma transação já existente não pode ser promovida a exclusiva
/// chamando esta rotina uma segunda vez: o flag de exclusividade só vale para
/// uma transação nova.
///
/// Uma transação de escrita precisa ser iniciada antes de qualquer alteração no
/// banco de dados. Nenhuma das rotinas a seguir funciona sem uma transação
/// iniciada antes:
///
///      btree_create_table()
///      btree_create_index()
///      btree_clear_table()
///      btree_drop_table()
///      btree_insert()
///      btree_delete()
///      btree_update_meta()
///
/// Se a tentativa inicial de obter o travamento falhar por contenção e o banco
/// estava destravado, invoca o manipulador de "ocupado", se houver. Mas se já
/// havia um travamento de leitura, não invoca o manipulador: apenas retorna
/// `SQLITE_BUSY`. Isso evita um impasse (A com leitura tentando promover a
/// reservado, B com reservado tentando promover a exclusivo): ao devolver
/// `SQLITE_BUSY` sem chamar o manipulador quando A já tem leitura, incentiva-se
/// A a desistir e deixar B prosseguir.
///
/// O `btreeBeginTrans` estático do C se chama `btree_begin_trans_impl` aqui,
/// porque `btree_begin_trans` é o nome (pela regra) do `sqlite3BtreeBeginTrans`.
fn btree_begin_trans_impl(
    p: &mut Btree,
    wrflag: i32,
    p_schema_version: Option<&mut i32>,
) -> i32 {
    let p_bt_rc = p.p_bt.clone().expect("Btree.p_bt");
    let pager = p_bt_rc.borrow().p_pager.clone().expect("BtShared.p_pager");
    let db = p
        .db
        .as_ref()
        .and_then(|w| w.upgrade())
        .expect("Btree.db");
    let mut rc = SQLITE_OK;

    btree_enter(p);

    {
        let mut bt = p_bt_rc.borrow_mut();

        'trans_begun: {
            // Se a árvore b já está em uma transação de escrita, ou já está em
            // uma transação de leitura e uma de leitura é solicitada, é um no-op.
            if p.in_trans == TRANS_WRITE || (p.in_trans == TRANS_READ && wrflag == 0) {
                break 'trans_begun;
            }

            if (db.borrow().flags & SQLITE_RESETDATABASE) != 0
                && pager_isreadonly(&pager.borrow()) == 0
            {
                bt.bts_flags &= !BTS_READ_ONLY;
            }

            // Transações de escrita não são possíveis num banco somente leitura
            if (bt.bts_flags & BTS_READ_ONLY) != 0 && wrflag != 0 {
                rc = SQLITE_READONLY;
                break 'trans_begun;
            }

            {
                let mut p_block: Option<SqliteRef> = None;
                // Se outro manipulador de banco de dados já abriu uma transação
                // de escrita nesta estrutura b-tree compartilhada e uma segunda
                // transação de escrita é solicitada, retorna SQLITE_LOCKED.
                if (wrflag != 0 && bt.in_transaction == TRANS_WRITE)
                    || (bt.bts_flags & BTS_PENDING) != 0
                {
                    let writer = bt
                        .p_writer
                        .as_ref()
                        .and_then(|w| w.upgrade())
                        .expect("BtShared.p_writer");
                    p_block = writer.borrow().db.as_ref().and_then(|w| w.upgrade());
                } else if wrflag > 1 {
                    let me = p.to_weak();
                    let mut p_iter = bt.p_lock.as_deref();
                    while let Some(lock) = p_iter {
                        let other = match &lock.p_btree {
                            Some(w) => !Weak::ptr_eq(w, &me),
                            None => false,
                        };
                        if other {
                            let other_btree = lock
                                .p_btree
                                .as_ref()
                                .and_then(|w| w.upgrade())
                                .expect("BtLock.p_btree");
                            p_block = other_btree.borrow().db.as_ref().and_then(|w| w.upgrade());
                            break;
                        }
                        p_iter = lock.p_next.as_deref();
                    }
                }
                if let Some(block) = p_block {
                    connection_blocked(&db, &block);
                    rc = SQLITE_LOCKED_SHAREDCACHE;
                    break 'trans_begun;
                }
            }

            // Qualquer transação de leitura ou de leitura e escrita implica um
            // travamento de leitura na página 1. Então, se algum outro cliente
            // de cache compartilhado já tem um travamento de escrita na página
            // 1, a transação não pode ser aberta.
            rc = query_shared_cache_table_lock(p, SCHEMA_ROOT, READ_LOCK);
            if SQLITE_OK != rc {
                break 'trans_begun;
            }

            bt.bts_flags &= !BTS_INITIALLY_EMPTY;
            if bt.n_page == 0 {
                bt.bts_flags |= BTS_INITIALLY_EMPTY;
            }
            loop {
                pager_wal_db(&mut pager.borrow_mut(), Some(db.clone()));

                // Chama lock_btree() até que p_bt.p_page_1 seja preenchido ou
                // lock_btree() retorne algo diferente de SQLITE_OK. O
                // lock_btree() pode retornar SQLITE_OK mas deixar p_page_1 em
                // None se, depois de ler a página 1, descobre que o tamanho de
                // página do arquivo não é p_bt.page_size. Nesse caso o
                // lock_btree() atualiza p_bt.page_size com o do arquivo em disco.
                while bt.p_page_1.is_none() {
                    rc = lock_btree(&mut bt);
                    if rc != SQLITE_OK {
                        break;
                    }
                }

                if rc == SQLITE_OK && wrflag != 0 {
                    if (bt.bts_flags & BTS_READ_ONLY) != 0 {
                        rc = SQLITE_READONLY;
                    } else {
                        rc = pager_begin(
                            &mut pager.borrow_mut(),
                            if wrflag > 1 { 1 } else { 0 },
                            temp_in_memory(&db.borrow()),
                        );
                        if rc == SQLITE_OK {
                            rc = new_database(&mut bt);
                        } else if rc == SQLITE_BUSY_SNAPSHOT && bt.in_transaction == TRANS_NONE {
                            // Se não havia transação aberta quando esta função
                            // foi chamada e SQLITE_BUSY_SNAPSHOT foi devolvido,
                            // troca o código de erro para SQLITE_BUSY.
                            rc = SQLITE_BUSY;
                        }
                    }
                }

                if rc != SQLITE_OK {
                    let _ = pager_wal_write_lock(&mut pager.borrow_mut(), 0);
                    unlock_btree_if_unused(&mut bt);
                }

                if !((rc & 0xFF) == SQLITE_BUSY
                    && bt.in_transaction == TRANS_NONE
                    && btree_invoke_busy_handler(&mut bt) != 0)
                {
                    break;
                }
            }
            pager_wal_db(&mut pager.borrow_mut(), None);

            if rc == SQLITE_OK {
                if p.in_trans == TRANS_NONE {
                    bt.n_transaction += 1;
                    if p.sharable != 0 {
                        // No C, `p->lock` é encadeado na lista por ponteiro.
                        // Aqui a lista é de `Box<BtLock>`, então entra uma cópia
                        // do `p.lock` (o dono do nó é a lista de `bt`).
                        p.lock.e_lock = READ_LOCK;
                        p.lock.p_next = None;
                        let mut node = p.lock.clone();
                        node.p_next = bt.p_lock.take();
                        bt.p_lock = Some(Box::new(node));
                    }
                }
                p.in_trans = if wrflag != 0 { TRANS_WRITE } else { TRANS_READ };
                if p.in_trans > bt.in_transaction {
                    bt.in_transaction = p.in_trans;
                }
                if wrflag != 0 {
                    let p_page_1 = bt.p_page_1.clone().expect("BtShared.p_page_1");
                    bt.p_writer = Some(p.to_weak());
                    bt.bts_flags &= !BTS_EXCLUSIVE;
                    if wrflag > 1 {
                        bt.bts_flags |= BTS_EXCLUSIVE;
                    }

                    // Se o campo de tamanho do banco no cabeçalho estiver
                    // incorreto (como pode estar se um cliente antigo escreveu
                    // no arquivo), atualiza agora. Fazer isto cedo significa
                    // que o tamanho do banco pode ser relido com segurança da
                    // página 1 se um savepoint ou uma transação sofrer rollback
                    // dentro da transação.
                    let size_in_header = get4byte(&p_page_1.borrow().a_data[28..]);
                    if bt.n_page != size_in_header {
                        let p_db_page = p_page_1
                            .borrow()
                            .p_db_page
                            .clone()
                            .expect("MemPage.p_db_page");
                        rc = pager_write(&p_db_page);
                        if rc == SQLITE_OK {
                            put4byte(&mut p_page_1.borrow_mut().a_data[28..], bt.n_page);
                        }
                    }
                }
            }
        }

        // trans_begun:
        if rc == SQLITE_OK {
            if let Some(sv) = p_schema_version {
                let page1 = bt.p_page_1.as_ref().expect("BtShared.p_page_1");
                *sv = get4byte(&page1.borrow().a_data[40..]) as i32;
            }
            if wrflag != 0 {
                // Esta chamada garante que o paginador tenha o número correto
                // de savepoints abertos. Se o segundo parâmetro for maior que 0
                // e o sub-journal ainda não estiver aberto, será aberto aqui.
                rc = pager_open_savepoint(&mut pager.borrow_mut(), db.borrow().n_savepoint);
            }
        }
    }

    btree_leave(p);
    rc
}

pub fn btree_begin_trans(p: &mut Btree, wrflag: i32, p_schema_version: Option<&mut i32>) -> i32 {
    if p.sharable != 0
        || p.in_trans == TRANS_NONE
        || (p.in_trans == TRANS_READ && wrflag != 0)
    {
        return btree_begin_trans_impl(p, wrflag, p_schema_version);
    }
    let p_bt = p.p_bt.clone().expect("Btree.p_bt");
    let bt = p_bt.borrow();
    if let Some(sv) = p_schema_version {
        let page1 = bt.p_page_1.as_ref().expect("BtShared.p_page_1");
        *sv = get4byte(&page1.borrow().a_data[40..]) as i32;
    }
    if wrflag != 0 {
        // Esta chamada garante que o paginador tenha o número correto de
        // savepoints abertos. Se o segundo parâmetro for maior que 0 e o
        // sub-journal ainda não estiver aberto, será aberto aqui.
        let pager = bt.p_pager.clone().expect("BtShared.p_pager");
        let n_savepoint = p
            .db
            .as_ref()
            .and_then(|w| w.upgrade())
            .expect("Btree.db")
            .borrow()
            .n_savepoint;
        let rc = pager_open_savepoint(&mut pager.borrow_mut(), n_savepoint);
        rc
    } else {
        SQLITE_OK
    }
}

/// Define as entradas do mapa de ponteiros para todos os filhos da página
/// `p_page`. Além disso, se `p_page` tiver células que apontam para páginas de
/// overflow, define as entradas do mapa para elas também.
///
/// O C lê `pPage->pBt`; aqui o chamador já segura o `BtShared` e o passa, para
/// não reabrir o `RefCell` que ele mantém emprestado. Como `pSrc==pPage` na
/// chamada a `ptrmapPutOvflPtr`, os dois parâmetros do C viram um só.
fn set_child_ptrmaps(p_bt: &mut BtShared, p_page: &mut MemPage) -> i32 {
    let pgno = p_page.pgno;

    let mut rc = if p_page.is_init != 0 {
        SQLITE_OK
    } else {
        btree_init_page(p_page)
    };
    if rc != SQLITE_OK {
        return rc;
    }
    let n_cell = p_page.n_cell as i32;

    for i in 0..n_cell {
        let p_cell = find_cell(p_page, i);

        ptrmap_put_ovfl_ptr(p_bt, p_page, p_cell, &mut rc);

        if p_page.leaf == 0 {
            let child_pgno = get4byte(&p_page.a_data[p_cell..]);
            ptrmap_put(p_bt, child_pgno, PTRMAP_BTREE, pgno, &mut rc);
        }
    }

    if p_page.leaf == 0 {
        let child_pgno = get4byte(&p_page.a_data[p_page.hdr_offset as usize + 8..]);
        ptrmap_put(p_bt, child_pgno, PTRMAP_BTREE, pgno, &mut rc);
    }

    rc
}

/// Em algum lugar de `p_page` há um ponteiro para a página `i_from`. Modifica
/// este ponteiro para que aponte para `i_to`. O parâmetro `e_type` descreve o
/// tipo de ponteiro a modificar:
///
/// PTRMAP_BTREE:     `p_page` é uma página de b-tree. O ponteiro aponta para uma
///                   página filha de `p_page`.
///
/// PTRMAP_OVERFLOW1: `p_page` é uma página de b-tree. O ponteiro aponta para uma
///                   página de overflow apontada por uma das células de `p_page`.
///
/// PTRMAP_OVERFLOW2: `p_page` é uma página de overflow. O ponteiro aponta para a
///                   próxima página de overflow da lista.
///
/// `usable_size` é o `pPage->pBt->usableSize` do C (o chamador já segura o
/// `BtShared`).
fn modify_page_pointer(
    usable_size: u32,
    p_page: &mut MemPage,
    i_from: Pgno,
    i_to: Pgno,
    e_type: u8,
) -> i32 {
    if e_type == PTRMAP_OVERFLOW2 {
        // O ponteiro é sempre os 4 primeiros bytes da página neste caso.
        if get4byte(&p_page.a_data) != i_from {
            return sqlite_corrupt_page(p_page);
        }
        put4byte(&mut p_page.a_data, i_to);
    } else {
        let rc = if p_page.is_init != 0 {
            SQLITE_OK
        } else {
            btree_init_page(p_page)
        };
        if rc != SQLITE_OK {
            return rc;
        }
        let n_cell = p_page.n_cell as i32;

        let mut i = 0;
        while i < n_cell {
            let p_cell = find_cell(p_page, i);
            if e_type == PTRMAP_OVERFLOW1 {
                let mut info = CellInfo::default();
                (p_page.x_parse_cell)(&*p_page, &p_page.a_data[p_cell..], &mut info);
                if (info.n_local as u32) < info.n_payload {
                    if p_cell + info.n_size as usize > usable_size as usize {
                        return sqlite_corrupt_page(p_page);
                    }
                    let at = p_cell + info.n_size as usize - 4;
                    if i_from == get4byte(&p_page.a_data[at..]) {
                        put4byte(&mut p_page.a_data[at..], i_to);
                        break;
                    }
                }
            } else {
                if p_cell + 4 > usable_size as usize {
                    return sqlite_corrupt_page(p_page);
                }
                if get4byte(&p_page.a_data[p_cell..]) == i_from {
                    put4byte(&mut p_page.a_data[p_cell..], i_to);
                    break;
                }
            }
            i += 1;
        }

        if i == n_cell {
            let at = p_page.hdr_offset as usize + 8;
            if e_type != PTRMAP_BTREE || get4byte(&p_page.a_data[at..]) != i_from {
                return sqlite_corrupt_page(p_page);
            }
            put4byte(&mut p_page.a_data[at..], i_to);
        }
    }
    SQLITE_OK
}


// ---- part_010.rs ----

/// Move a página aberta `p_db_page` do banco de dados para a posição
/// `i_free_page`. A referência a `p_db_page` continua válida.
///
/// O flag `is_commit` indica que não é preciso lembrar que o journal precisa de
/// sync() antes de a página `p_db_page.pgno` poder ser escrita. O chamador já
/// prometeu não escrever nessa página.
fn relocate_page(
    p_bt: &mut BtShared,
    p_db_page: &MemPageRef, // Página aberta a mover
    e_type: u8,             // Entrada 'tipo' do mapa de ponteiros para p_db_page
    i_ptr_page: Pgno,       // Entrada 'número da página' do mapa de ponteiros
    i_free_page: Pgno,      // A posição para onde mover p_db_page
    is_commit: i32,         // Flag is_commit passado a pager_movepage
) -> i32 {
    let i_db_page = p_db_page.borrow().pgno;
    let pager = p_bt.p_pager.clone().expect("BtShared.p_pager");

    debug_assert!(
        e_type == PTRMAP_OVERFLOW2
            || e_type == PTRMAP_OVERFLOW1
            || e_type == PTRMAP_BTREE
            || e_type == PTRMAP_ROOTPAGE
    );
    if i_db_page < 3 {
        return SQLITE_CORRUPT_BKPT;
    }

    // Move a página i_db_page da posição atual para o número i_free_page
    let pg_hdr = p_db_page
        .borrow()
        .p_db_page
        .clone()
        .expect("MemPage.p_db_page");
    let mut rc = pager_movepage(&mut pager.borrow_mut(), &pg_hdr, i_free_page, is_commit);
    if rc != SQLITE_OK {
        return rc;
    }
    p_db_page.borrow_mut().pgno = i_free_page;

    // Se p_db_page era uma página de b-tree, ela pode ter páginas filhas e/ou
    // células que apontam para páginas de overflow. As entradas do mapa de
    // ponteiros de todas essas páginas precisam mudar.
    //
    // Se p_db_page é uma página de overflow, os 4 primeiros bytes podem guardar
    // um ponteiro para a página de overflow seguinte. Nesse caso, o mapa de
    // ponteiros precisa ser atualizado para essa página seguinte.
    if e_type == PTRMAP_BTREE || e_type == PTRMAP_ROOTPAGE {
        rc = set_child_ptrmaps(p_bt, &mut p_db_page.borrow_mut());
        if rc != SQLITE_OK {
            return rc;
        }
    } else {
        let next_ovfl: Pgno = get4byte(&p_db_page.borrow().a_data);
        if next_ovfl != 0 {
            ptrmap_put(p_bt, next_ovfl, PTRMAP_OVERFLOW2, i_free_page, &mut rc);
            if rc != SQLITE_OK {
                return rc;
            }
        }
    }

    // Corrige o ponteiro na página i_ptr_page que apontava para i_db_page para
    // que aponte para i_free_page. Corrige também a entrada do mapa de
    // ponteiros de i_ptr_page.
    if e_type != PTRMAP_ROOTPAGE {
        let mut p_ptr_page_slot: Option<MemPageRef> = None;
        rc = btree_get_page(p_bt, i_ptr_page, &mut p_ptr_page_slot, 0);
        if rc != SQLITE_OK {
            return rc;
        }
        let p_ptr_page: MemPageRef = p_ptr_page_slot.expect("btree_get_page devolveu a página");
        let ptr_hdr = p_ptr_page
            .borrow()
            .p_db_page
            .clone()
            .expect("MemPage.p_db_page");
        rc = pager_write(&ptr_hdr);
        if rc != SQLITE_OK {
            release_page(p_ptr_page);
            return rc;
        }
        rc = modify_page_pointer(
            p_bt.usable_size,
            &mut p_ptr_page.borrow_mut(),
            i_db_page,
            i_free_page,
            e_type,
        );
        release_page(p_ptr_page);
        if rc == SQLITE_OK {
            ptrmap_put(p_bt, i_free_page, e_type, i_ptr_page, &mut rc);
        }
    }
    rc
}

/// Executa um único passo de um vacuum incremental. Se bem-sucedido, retorna
/// `SQLITE_OK`. Se não há trabalho a fazer (e portanto não adianta chamar esta
/// função de novo), retorna `SQLITE_DONE`. Se ocorre um erro, retorna outro
/// código de erro.
///
/// Mais especificamente, esta função tenta reorganizar o banco de dados para
/// que a última página do arquivo em uso deixe de estar em uso.
///
/// O parâmetro `n_fin` é o número de páginas que este banco teria se esta
/// função fosse chamada até retornar `SQLITE_DONE`.
///
/// Se `b_commit` for diferente de zero, a função supõe que o chamador continuará
/// chamando `incr_vacuum_step` até ela retornar `SQLITE_DONE` ou um erro.
/// `b_commit` é verdadeiro numa operação de auto-vacuum no commit e falso num
/// vacuum incremental.
///
/// (A declaração antecipada de `allocateBtreePage` do C não tem equivalente em
/// Rust: `allocate_btree_page` é definida em outra parte deste módulo.)
fn incr_vacuum_step(p_bt: &mut BtShared, n_fin: Pgno, i_last_pg: Pgno, b_commit: i32) -> i32 {
    let mut rc: i32;
    let mut i_last_pg = i_last_pg;

    debug_assert!(i_last_pg > n_fin);

    if !ptrmap_ispage(p_bt, i_last_pg) && i_last_pg != pending_byte_page(p_bt) {
        let mut e_type: u8 = 0;
        let mut i_ptr_page: Pgno = 0;

        let n_free_list: Pgno = get4byte(
            &p_bt
                .p_page_1
                .as_ref()
                .expect("BtShared.p_page_1")
                .borrow()
                .a_data[36..],
        );
        if n_free_list == 0 {
            return SQLITE_DONE;
        }

        rc = ptrmap_get(p_bt, i_last_pg, &mut e_type, &mut i_ptr_page);
        if rc != SQLITE_OK {
            return rc;
        }
        if e_type == PTRMAP_ROOTPAGE {
            return SQLITE_CORRUPT_BKPT;
        }

        if e_type == PTRMAP_FREEPAGE {
            if b_commit == 0 {
                // Remove a página da lista de livres do arquivo. Isto não é
                // necessário se b_commit for diferente de zero: nesse caso a
                // lista será truncada a zero depois que esta função retornar,
                // então não importa se ainda tiver entradas com lixo.
                let mut i_free_pg: Pgno = 0;
                let mut p_free_pg: Option<MemPageRef> = None;
                rc = allocate_btree_page(p_bt, &mut p_free_pg, &mut i_free_pg, i_last_pg, BTALLOC_EXACT);
                if rc != SQLITE_OK {
                    return rc;
                }
                debug_assert!(i_free_pg == i_last_pg);
                if let Some(pg) = p_free_pg {
                    release_page(pg);
                }
            }
        } else {
            let mut i_free_pg: Pgno;       // Índice da página livre para onde mover p_last_pg
            let mut e_mode: u8 = BTALLOC_ANY; // Parâmetro de modo de allocate_btree_page()
            let mut i_near: Pgno = 0;      // Parâmetro "nearby" de allocate_btree_page()

            let mut p_last_pg_slot: Option<MemPageRef> = None;
            rc = btree_get_page(p_bt, i_last_pg, &mut p_last_pg_slot, 0);
            if rc != SQLITE_OK {
                return rc;
            }
            let p_last_pg: MemPageRef = p_last_pg_slot.expect("btree_get_page devolveu a página");

            // Se b_commit é zero, este laço roda exatamente uma vez e a página
            // p_last_pg é trocada com a primeira página livre tirada da lista de
            // livres.
            //
            // Por outro lado, se b_commit é maior que zero, continua o laço até
            // achar uma página livre situada dentro das primeiras n_fin páginas
            // do arquivo.
            if b_commit == 0 {
                e_mode = BTALLOC_LE;
                i_near = n_fin;
            }
            loop {
                let mut p_free_pg: Option<MemPageRef> = None;
                i_free_pg = 0;
                let db_size: Pgno = btree_pagecount(p_bt);
                rc = allocate_btree_page(p_bt, &mut p_free_pg, &mut i_free_pg, i_near, e_mode);
                if rc != SQLITE_OK {
                    release_page(p_last_pg);
                    return rc;
                }
                if let Some(pg) = p_free_pg {
                    release_page(pg);
                }
                if i_free_pg > db_size {
                    release_page(p_last_pg);
                    return SQLITE_CORRUPT_BKPT;
                }
                if !(b_commit != 0 && i_free_pg > n_fin) {
                    break;
                }
            }
            debug_assert!(i_free_pg < i_last_pg);

            rc = relocate_page(p_bt, &p_last_pg, e_type, i_ptr_page, i_free_pg, b_commit);
            release_page(p_last_pg);
            if rc != SQLITE_OK {
                return rc;
            }
        }
    }

    if b_commit == 0 {
        loop {
            i_last_pg = i_last_pg.wrapping_sub(1);
            if !(i_last_pg == pending_byte_page(p_bt) || ptrmap_ispage(p_bt, i_last_pg)) {
                break;
            }
        }
        p_bt.b_do_truncate = 1;
        p_bt.n_page = i_last_pg;
    }
    SQLITE_OK
}

/// O banco aberto pelo primeiro argumento é um banco auto-vacuum de `n_orig`
/// páginas contendo `n_free` páginas livres. Retorna o tamanho esperado do banco,
/// em páginas, depois de uma operação de auto-vacuum.
fn final_db_size(p_bt: &BtShared, n_orig: Pgno, n_free: Pgno) -> Pgno {
    // Número de entradas numa página do mapa de ponteiros
    let n_entry: u32 = p_bt.usable_size / 5;
    // Número de páginas do mapa de ponteiros a liberar
    let n_ptrmap: Pgno = n_free
        .wrapping_sub(n_orig)
        .wrapping_add(ptrmap_pageno(p_bt, n_orig))
        .wrapping_add(n_entry)
        / n_entry;
    // Valor de retorno
    let mut n_fin: Pgno = n_orig.wrapping_sub(n_free).wrapping_sub(n_ptrmap);
    if n_orig > pending_byte_page(p_bt) && n_fin < pending_byte_page(p_bt) {
        n_fin = n_fin.wrapping_sub(1);
    }
    while ptrmap_ispage(p_bt, n_fin) || n_fin == pending_byte_page(p_bt) {
        n_fin = n_fin.wrapping_sub(1);
    }

    n_fin
}

/// Uma transação de escrita precisa estar aberta antes de chamar esta função.
/// Ela executa uma unidade de trabalho de um vacuum incremental.
///
/// Se o vacuum incremental estiver concluído depois da execução desta função,
/// retorna `SQLITE_DONE`. Se não estiver concluído, mas não houve erro, retorna
/// `SQLITE_OK`. Caso contrário, um código de erro do SQLite.
pub fn btree_incr_vacuum(p: &mut Btree) -> i32 {
    let p_bt_rc = p.p_bt.clone().expect("Btree.p_bt");
    let rc: i32;

    btree_enter(p);
    {
        let mut bt = p_bt_rc.borrow_mut();
        debug_assert!(bt.in_transaction == TRANS_WRITE && p.in_trans == TRANS_WRITE);
        if bt.auto_vacuum == 0 {
            rc = SQLITE_DONE;
        } else {
            let n_orig: Pgno = btree_pagecount(&bt);
            let n_free: Pgno = get4byte(
                &bt.p_page_1
                    .as_ref()
                    .expect("BtShared.p_page_1")
                    .borrow()
                    .a_data[36..],
            );
            let n_fin: Pgno = final_db_size(&bt, n_orig, n_free);

            if n_orig < n_fin || n_free >= n_orig {
                rc = SQLITE_CORRUPT_BKPT;
            } else if n_free > 0 {
                let mut r = save_all_cursors(&mut bt, 0, None);
                if r == SQLITE_OK {
                    invalidate_all_overflow_cache(&mut bt);
                    r = incr_vacuum_step(&mut bt, n_fin, n_orig, 0);
                }
                if r == SQLITE_OK {
                    let page1 = bt.p_page_1.clone().expect("BtShared.p_page_1");
                    let pg_hdr = page1.borrow().p_db_page.clone().expect("MemPage.p_db_page");
                    r = pager_write(&pg_hdr);
                    put4byte(&mut page1.borrow_mut().a_data[28..], bt.n_page);
                }
                rc = r;
            } else {
                rc = SQLITE_DONE;
            }
        }
    }
    btree_leave(p);
    rc
}

/// Esta rotina é chamada antes de `pager_commit` quando uma transação é
/// confirmada num banco auto-vacuum.
///
/// Recebe o `BtShared` já aberto pelo chamador (`p_bt`), além do `Btree`
/// (`p`), que só serve para achar o `Db` correspondente em `db.a_db`.
fn auto_vacuum_commit(p: &Btree, p_bt: &mut BtShared) -> i32 {
    let mut rc = SQLITE_OK;
    let pager = p_bt.p_pager.clone().expect("BtShared.p_pager");

    invalidate_all_overflow_cache(p_bt);
    debug_assert!(p_bt.auto_vacuum != 0);
    if p_bt.incr_vacuum == 0 {
        // Número de páginas do banco depois do auto-vacuum
        let n_fin: Pgno;
        // Número de páginas na lista de livres no início
        let n_free: Pgno;
        // Número de páginas a limpar
        let mut n_vac: Pgno;
        // Tamanho do banco antes de liberar
        let n_orig: Pgno = btree_pagecount(p_bt);

        if ptrmap_ispage(p_bt, n_orig) || n_orig == pending_byte_page(p_bt) {
            // Não é possível criar um banco cuja última página seja uma página
            // do mapa de ponteiros ou a página do byte pendente. Se uma delas
            // aparecer, indica corrupção.
            return SQLITE_CORRUPT_BKPT;
        }

        n_free = get4byte(
            &p_bt
                .p_page_1
                .as_ref()
                .expect("BtShared.p_page_1")
                .borrow()
                .a_data[36..],
        );
        let db = p
            .db
            .as_ref()
            .and_then(|w| w.upgrade())
            .expect("Btree.db");
        let callback = db.borrow().x_autovac_pages.clone();
        if let Some(x_autovac_pages) = callback {
            let me = p.to_weak();
            let (p_arg, z_db_s_name) = {
                let d = db.borrow();
                let mut i_db = 0usize;
                while i_db < d.n_db as usize {
                    let same = match &d.a_db[i_db].p_bt {
                        Some(b) => Weak::ptr_eq(&Rc::downgrade(b), &me),
                        None => false,
                    };
                    if same {
                        break;
                    }
                    i_db += 1;
                }
                (d.p_autovac_pages_arg.clone(), d.a_db[i_db].z_db_s_name.clone())
            };
            n_vac = x_autovac_pages(&p_arg, &z_db_s_name, n_orig, n_free, p_bt.page_size);
            if n_vac > n_free {
                n_vac = n_free;
            }
            if n_vac == 0 {
                return SQLITE_OK;
            }
        } else {
            n_vac = n_free;
        }
        n_fin = final_db_size(p_bt, n_orig, n_vac);
        if n_fin > n_orig {
            return SQLITE_CORRUPT_BKPT;
        }
        if n_fin < n_orig {
            rc = save_all_cursors(p_bt, 0, None);
        }
        // Próxima página a ser liberada
        let mut i_free: Pgno = n_orig;
        while i_free > n_fin && rc == SQLITE_OK {
            rc = incr_vacuum_step(p_bt, n_fin, i_free, if n_vac == n_free { 1 } else { 0 });
            i_free -= 1;
        }
        if (rc == SQLITE_DONE || rc == SQLITE_OK) && n_free > 0 {
            let page1 = p_bt.p_page_1.clone().expect("BtShared.p_page_1");
            let pg_hdr = page1.borrow().p_db_page.clone().expect("MemPage.p_db_page");
            rc = pager_write(&pg_hdr);
            if n_vac == n_free {
                put4byte(&mut page1.borrow_mut().a_data[32..], 0);
                put4byte(&mut page1.borrow_mut().a_data[36..], 0);
            }
            put4byte(&mut page1.borrow_mut().a_data[28..], n_fin);
            p_bt.b_do_truncate = 1;
            p_bt.n_page = n_fin;
        }
        if rc != SQLITE_OK {
            let _ = pager_rollback(&mut pager.borrow_mut());
        }
    }

    rc
}

/// Esta rotina executa a primeira fase de um commit em duas fases. Faz com que
/// um journal de rollback seja criado (se ainda não existir) e preenchido com
/// informação suficiente para que, se faltar energia, o banco possa ser
/// restaurado ao estado original reproduzindo o journal. Depois o conteúdo do
/// journal é descarregado no disco. Com o journal seguro no disco, as mudanças
/// no banco são escritas no arquivo do banco e descarregadas. Ao fim desta
/// chamada, o journal de rollback ainda existe em disco e ainda seguramos todos
/// os travamentos, então a transação não foi confirmada. Veja
/// `btree_commit_phase_two()` para a segunda fase do commit.
///
/// Esta chamada é um no-op se não há transação de escrita ativa em `p`.
///
/// Caso contrário, faz sync do arquivo do banco do btree. `z_super_jrnl` é o
/// nome de um arquivo de super-journal a gravar no journal individual, ou `None`,
/// indicando que não há super-journal (transação de um único banco).
///
/// Quando isto é chamado, o super-journal já deve ter sido criado, preenchido com
/// este ponteiro de journal e sincronizado em disco.
///
/// Depois que esta rotina retorna, a única coisa necessária para confirmar a
/// transação de escrita deste arquivo é apagar o journal.
pub fn btree_commit_phase_one(p: &mut Btree, z_super_jrnl: Option<&[u8]>) -> i32 {
    let mut rc = SQLITE_OK;
    if p.in_trans == TRANS_WRITE {
        let p_bt_rc = p.p_bt.clone().expect("Btree.p_bt");
        let pager = p_bt_rc.borrow().p_pager.clone().expect("BtShared.p_pager");
        btree_enter(p);
        let early: Option<i32> = {
            let mut bt = p_bt_rc.borrow_mut();
            let mut early = None;
            if bt.auto_vacuum != 0 {
                rc = auto_vacuum_commit(p, &mut bt);
                if rc != SQLITE_OK {
                    early = Some(rc);
                }
            }
            if early.is_none() && bt.b_do_truncate != 0 {
                pager_truncate_image(&mut pager.borrow_mut(), bt.n_page);
            }
            early
        };
        if let Some(r) = early {
            btree_leave(p);
            return r;
        }
        rc = pager_commit_phase_one(&mut pager.borrow_mut(), z_super_jrnl, 0);
        btree_leave(p);
    }
    rc
}


// ---- part_011.rs ----

/// Esta função é chamada tanto de `btree_commit_phase_two()` quanto de
/// `btree_rollback()` ao fim de uma transação.
fn btree_end_transaction(p: &mut Btree) {
    let p_bt_rc = p.p_bt.clone().expect("Btree.p_bt");
    let db = p
        .db
        .as_ref()
        .and_then(|w| w.upgrade())
        .expect("Btree.db");

    p_bt_rc.borrow_mut().b_do_truncate = 0;
    if p.in_trans > TRANS_NONE && db.borrow().n_vdbe_read > 1 {
        // Se há outros statements ativos que pertencem a este manipulador de
        // banco, rebaixa para uma transação somente leitura. Os outros
        // statements ainda podem estar lendo do banco.
        downgrade_all_shared_cache_table_locks(p);
        p.in_trans = TRANS_READ;
    } else {
        // Se o manipulador tinha qualquer tipo de transação aberta, decrementa
        // a contagem de transações do btree compartilhado. Se a contagem chegar
        // a 0, define o estado compartilhado como TRANS_NONE. A chamada a
        // unlock_btree_if_unused() abaixo destrava o pager.
        if p.in_trans != TRANS_NONE {
            clear_all_shared_cache_table_locks(p);
            let mut bt = p_bt_rc.borrow_mut();
            bt.n_transaction -= 1;
            if 0 == bt.n_transaction {
                bt.in_transaction = TRANS_NONE;
            }
        }

        // Define o estado atual da transação como TRANS_NONE e destrava o pager
        // se esta chamada fechou a única transação de leitura ou escrita.
        p.in_trans = TRANS_NONE;
        unlock_btree_if_unused(&mut p_bt_rc.borrow_mut());
    }
}

/// Confirma a transação em andamento.
///
/// Esta rotina implementa a segunda fase de um commit em 2 fases. A rotina
/// `btree_commit_phase_one()` faz a primeira fase e deve ser invocada antes
/// desta. Ela fez todo o trabalho de escrever as informações no disco e
/// descarregar o conteúdo para que fiquem gravadas. Tudo o que esta rotina
/// precisa fazer é apagar, truncar ou zerar o cabeçalho do journal de rollback
/// (o que faz a transação ser confirmada) e soltar os travamentos.
///
/// Normalmente, se ocorre um erro enquanto a camada do pager tenta finalizar o
/// arquivo de journal, esta função retorna o erro e a camada superior tenta um
/// rollback. Porém, se o segundo argumento for diferente de zero, esta transação
/// b-tree faz parte de uma transação de vários arquivos. Nesse caso a transação
/// já foi confirmada (ao apagar um arquivo de super-journal) e o chamador
/// ignorará o código de retorno desta função. Então, mesmo que ocorra um erro na
/// camada do pager, o estado interno dos objetos b-tree é redefinido para
/// indicar que a transação de escrita foi fechada. Isto é bem seguro, pois o
/// pager terá passado ao estado de erro.
///
/// Isto libera o travamento de escrita no arquivo do banco. Se não houver
/// cursores ativos, libera também o travamento de leitura.
pub fn btree_commit_phase_two(p: &mut Btree, b_cleanup: i32) -> i32 {
    if p.in_trans == TRANS_NONE {
        return SQLITE_OK;
    }
    btree_enter(p);

    // Se o manipulador tem uma transação de escrita aberta, confirma a
    // transação dos btrees compartilhados e define o estado compartilhado como
    // TRANS_READ.
    if p.in_trans == TRANS_WRITE {
        let p_bt_rc = p.p_bt.clone().expect("Btree.p_bt");
        let pager = p_bt_rc.borrow().p_pager.clone().expect("BtShared.p_pager");
        debug_assert!(p_bt_rc.borrow().in_transaction == TRANS_WRITE);
        debug_assert!(p_bt_rc.borrow().n_transaction > 0);
        let rc = pager_commit_phase_two(&mut pager.borrow_mut());
        if rc != SQLITE_OK && b_cleanup == 0 {
            btree_leave(p);
            return rc;
        }
        p.i_b_data_version = p.i_b_data_version.wrapping_sub(1); // Compensa o pPager->iDataVersion++
        let mut bt = p_bt_rc.borrow_mut();
        bt.in_transaction = TRANS_READ;
        btree_clear_has_content(&mut bt);
    }

    btree_end_transaction(p);
    btree_leave(p);
    SQLITE_OK
}

/// Faz as duas fases de um commit.
pub fn btree_commit(p: &mut Btree) -> i32 {
    btree_enter(p);
    let mut rc = btree_commit_phase_one(p, None);
    if rc == SQLITE_OK {
        rc = btree_commit_phase_two(p, 0);
    }
    btree_leave(p);
    rc
}

/// Esta rotina define o estado como CURSOR_FAULT e o código de erro como
/// `err_code` para todo cursor de qualquer BtShared a que `p_btree` se refira.
/// Ou, se `write_only` for 1, só os cursores de escrita são acionados e os de
/// leitura ficam inalterados.
///
/// Todo cursor é candidato a ser acionado, inclusive cursores de outras conexões
/// de banco que compartilham o cache com `p_btree`.
///
/// Esta rotina é chamada quando ocorre um rollback. Se `write_only` for
/// verdadeiro, só os cursores de escrita precisam ser acionados: os cursores
/// somente leitura salvam suas posições atuais para poderem continuar após o
/// rollback. Se `write_only` for falso, todos os cursores são acionados. Em
/// geral `write_only` é falso se a transação revertida modificou o esquema do
/// banco. Nesse caso as páginas raiz das b-trees podem ser movidas ou apagadas,
/// o que torna inseguro para os cursores de leitura continuar.
///
/// Se `write_only` for verdadeiro e ocorrer um erro ao salvar a posição de um
/// cursor somente leitura, todos os cursores, inclusive os de leitura, são
/// acionados.
///
/// Retorna `SQLITE_OK` se bem-sucedido, ou, se ocorrer um erro ao salvar a
/// posição de um cursor, um código de erro do SQLite.
pub fn btree_trip_all_cursors(p_btree: &mut Btree, err_code: i32, write_only: i32) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!((write_only == 0 || write_only == 1) && BTCF_WRITEFLAG == 1);
    btree_enter(p_btree);
    let p_bt_rc = p_btree.p_bt.clone().expect("Btree.p_bt");
    // A lista de cursores é percorrida por clones dos `Rc`, sem manter o
    // `BtShared` emprestado, porque a recursão abaixo o empresta de novo.
    let mut p = p_bt_rc.borrow().p_cursor.clone();
    while let Some(p_cur) = p {
        let flags = p_cur.borrow().cur_flags;
        if write_only != 0 && (flags & BTCF_WRITEFLAG) == 0 {
            let e_state = p_cur.borrow().e_state;
            if e_state == CURSOR_VALID || e_state == CURSOR_SKIPNEXT {
                rc = save_cursor_position(&mut p_cur.borrow_mut());
                if rc != SQLITE_OK {
                    let _ = btree_trip_all_cursors(p_btree, rc, 0);
                    break;
                }
            }
        } else {
            let mut cur = p_cur.borrow_mut();
            btree_clear_cursor(&mut cur);
            cur.e_state = CURSOR_FAULT;
            cur.skip_next = err_code;
        }
        btree_release_all_cursor_pages(&mut p_cur.borrow_mut());
        let p_next = p_cur.borrow().p_next.clone();
        p = p_next;
    }
    btree_leave(p_btree);
    rc
}

/// Define o campo `p_bt.n_page` corretamente, conforme o estado atual do banco.
/// Supõe que `p_bt.p_page_1` é válido.
fn btree_set_n_page(p_bt: &mut BtShared, p_page_1: &MemPageRef) {
    let mut n_page: i32 = get4byte(&p_page_1.borrow().a_data[28..]) as i32;
    if n_page == 0 {
        let pager = p_bt.p_pager.clone().expect("BtShared.p_pager");
        pager_pagecount(&pager.borrow(), &mut n_page);
    }
    p_bt.n_page = n_page as u32;
}

/// Desfaz a transação em andamento.
///
/// Se `trip_code` não for `SQLITE_OK`, os cursores são invalidados (acionados).
/// Só os cursores de escrita são acionados se `write_only` for verdadeiro, mas
/// todos são acionados se `write_only` for falso. Qualquer tentativa de usar um
/// cursor acionado resulta em erro.
///
/// Isto libera o travamento de escrita no arquivo do banco. Se não houver
/// cursores ativos, libera também o travamento de leitura.
pub fn btree_rollback(p: &mut Btree, trip_code: i32, write_only: i32) -> i32 {
    let mut rc: i32;
    let mut trip_code = trip_code;
    let mut write_only = write_only;
    let p_bt_rc = p.p_bt.clone().expect("Btree.p_bt");

    debug_assert!(write_only == 1 || write_only == 0);
    debug_assert!(trip_code == SQLITE_ABORT_ROLLBACK || trip_code == SQLITE_OK);
    btree_enter(p);
    if trip_code == SQLITE_OK {
        rc = save_all_cursors(&mut p_bt_rc.borrow_mut(), 0, None);
        trip_code = rc;
        if rc != 0 {
            write_only = 0;
        }
    } else {
        rc = SQLITE_OK;
    }
    if trip_code != 0 {
        let rc2 = btree_trip_all_cursors(p, trip_code, write_only);
        debug_assert!(rc == SQLITE_OK || (write_only == 0 && rc2 == SQLITE_OK));
        if rc2 != SQLITE_OK {
            rc = rc2;
        }
    }

    if p.in_trans == TRANS_WRITE {
        {
            let mut bt = p_bt_rc.borrow_mut();
            let pager = bt.p_pager.clone().expect("BtShared.p_pager");
            debug_assert!(TRANS_WRITE == bt.in_transaction);
            let rc2 = pager_rollback(&mut pager.borrow_mut());
            if rc2 != SQLITE_OK {
                rc = rc2;
            }

            // O rollback pode ter destruído o valor de p_page_1.a_data. Então
            // chama btree_get_page() na página 1 de novo, para garantir que
            // p_page_1.a_data esteja correto.
            let mut p_page_1_slot: Option<MemPageRef> = None;
            if btree_get_page(&mut bt, 1, &mut p_page_1_slot, 0) == SQLITE_OK {
                let p_page_1 = p_page_1_slot.expect("btree_get_page devolveu a página 1");
                btree_set_n_page(&mut bt, &p_page_1);
                release_page_one(p_page_1);
            }
            bt.in_transaction = TRANS_READ;
            btree_clear_has_content(&mut bt);
        }
    }

    btree_end_transaction(p);
    btree_leave(p);
    rc
}

/// Inicia uma subtransação de statement. A subtransação pode ser desfeita
/// independentemente da transação principal. É preciso iniciar uma transação
/// antes de iniciar uma subtransação. A subtransação termina automaticamente se
/// a transação principal for confirmada ou desfeita.
///
/// Subtransações de statement são usadas em torno de statements SQL individuais
/// contidos num bloco BEGIN...COMMIT. Se ocorre um erro de restrição dentro do
/// statement, o efeito desse único statement pode ser desfeito sem precisar
/// desfazer a transação inteira.
///
/// Uma subtransação de statement é implementada como um savepoint anônimo. O
/// valor passado no segundo parâmetro é o número total de savepoints, incluindo
/// o novo savepoint anônimo, abertos na B-Tree. Isto é, se não há savepoints
/// ativos nem outras transações de statement abertas, `i_statement` é 1. Esse
/// savepoint anônimo pode ser liberado ou desfeito com `btree_savepoint()`.
pub fn btree_begin_stmt(p: &mut Btree, i_statement: i32) -> i32 {
    let p_bt_rc = p.p_bt.clone().expect("Btree.p_bt");
    btree_enter(p);
    debug_assert!(p.in_trans == TRANS_WRITE);
    debug_assert!((p_bt_rc.borrow().bts_flags & BTS_READ_ONLY) == 0);
    debug_assert!(i_statement > 0);
    debug_assert!(p_bt_rc.borrow().in_transaction == TRANS_WRITE);
    // No nível do pager, uma transação de statement é um savepoint com índice
    // maior que o de todos os savepoints criados explicitamente por comandos
    // SQL. É ilegal abrir, liberar ou desfazer qualquer um desses savepoints
    // enquanto o savepoint da transação de statement estiver ativo.
    let pager = p_bt_rc.borrow().p_pager.clone().expect("BtShared.p_pager");
    let rc = pager_open_savepoint(&mut pager.borrow_mut(), i_statement);
    btree_leave(p);
    rc
}

/// O segundo argumento desta função, `op`, é sempre `SAVEPOINT_ROLLBACK` ou
/// `SAVEPOINT_RELEASE`. Esta função libera ou desfaz o savepoint identificado
/// por `i_savepoint`, conforme o valor de `op`.
///
/// Normalmente `i_savepoint` é maior ou igual a zero. Porém, se `op` for
/// `SAVEPOINT_ROLLBACK`, `i_savepoint` também pode ser -1. Nesse caso o conteúdo
/// da transação inteira é desfeito. Isto difere de um rollback normal de
/// transação, pois nenhum travamento é liberado e a transação continua aberta.
pub fn btree_savepoint(p: &mut Btree, op: i32, i_savepoint: i32) -> i32 {
    let mut rc = SQLITE_OK;
    if p.in_trans == TRANS_WRITE {
        let p_bt_rc = p.p_bt.clone().expect("Btree.p_bt");
        debug_assert!(op == SAVEPOINT_RELEASE || op == SAVEPOINT_ROLLBACK);
        debug_assert!(i_savepoint >= 0 || (i_savepoint == -1 && op == SAVEPOINT_ROLLBACK));
        btree_enter(p);
        {
            let mut bt = p_bt_rc.borrow_mut();
            if op == SAVEPOINT_ROLLBACK {
                rc = save_all_cursors(&mut bt, 0, None);
            }
            if rc == SQLITE_OK {
                let pager = bt.p_pager.clone().expect("BtShared.p_pager");
                rc = pager_savepoint(&mut pager.borrow_mut(), op, i_savepoint);
            }
            if rc == SQLITE_OK {
                if i_savepoint < 0 && (bt.bts_flags & BTS_INITIALLY_EMPTY) != 0 {
                    bt.n_page = 0;
                }
                rc = new_database(&mut bt);
                let p_page_1 = bt.p_page_1.clone().expect("BtShared.p_page_1");
                btree_set_n_page(&mut bt, &p_page_1);

                // p_bt.n_page pode ser zero se o banco estava corrompido quando
                // a transação começou. Caso contrário, deve ser pelo menos 1.
            }
        }
        btree_leave(p);
    }
    rc
}

/// Cria um cursor novo para a BTree cuja raiz está na página `i_table`. Se um
/// cursor somente leitura é pedido, supõe-se que o chamador já tem pelo menos
/// uma transação de leitura aberta no banco. Se um cursor de escrita é pedido,
/// supõe-se que o chamador tem uma transação de escrita aberta.
///
/// Se o bit BTREE_WRCSR de `wr_flag` estiver limpo, o cursor só pode ser usado
/// para leitura. Se o bit BTREE_WRCSR estiver definido, o cursor pode ser usado
/// para leitura ou escrita, se as outras condições para escrita também forem
/// atendidas. São estas as condições para a escrita ser permitida:
///
/// 1: O cursor deve ter sido aberto com `wr_flag` contendo BTREE_WRCSR.
///
/// 2: Outras conexões que compartilham o mesmo cache de pager mas não estão no
///    estado READ_UNCOMMITTED não podem ter cursores abertos com `wr_flag==0` na
///    mesma tabela. Senão as mudanças feitas por este cursor de escrita seriam
///    visíveis para os cursores de leitura da outra conexão.
///
/// 3: O banco deve ser gravável (não estar em mídia somente leitura).
///
/// 4: Deve haver uma transação ativa.
///
/// O bit BTREE_FORDELETE de `wr_flag` pode ser definido opcionalmente se
/// BTREE_WRCSR estiver definido. Se FORDELETE estiver definido, é uma dica para a
/// implementação de que este cursor só será usado para buscar e apagar entradas
/// de um índice como parte de um DELETE maior. A dica FORDELETE não é usada por
/// esta implementação. Mas num mecanismo de armazenamento alternativo hipotético,
/// em que as entradas de índice são apagadas automaticamente quando as linhas
/// correspondentes da tabela são apagadas, o flag FORDELETE é uma dica de que
/// todas as operações SEEK e DELETE neste cursor podem ser no-ops e todas as
/// operações READ podem retornar uma linha nula (2 bytes: 0x01 0x00).
///
/// Não há verificação de que a página `i_table` seja de fato a raiz de uma
/// b-tree. Se não for, o cursor obtido não funcionará corretamente.
///
/// Supõe-se que `btree_cursor_zero()` já foi chamada em `p_cur` para inicializar
/// o espaço de memória antes de invocar esta rotina.
fn btree_cursor(
    p: &mut Btree,                          // A btree
    i_table: Pgno,                          // Página raiz da tabela a abrir
    wr_flag: i32,                           // 1 para escrever. 0 somente leitura
    p_key_info: Option<KeyInfoRef>,         // Primeiro argumento da função de comparação
    p_cur: &BtCursorRef,                    // Espaço para o novo cursor
) -> i32 {
    let p_bt_rc = p.p_bt.clone().expect("Btree.p_bt"); // Handle compartilhado da b-tree
    let mut i_table = i_table;

    debug_assert!(
        wr_flag == 0
            || wr_flag == BTREE_WRCSR as i32
            || wr_flag == (BTREE_WRCSR | BTREE_FORDELETE) as i32
    );
    debug_assert!(p.in_trans > TRANS_NONE);
    debug_assert!(wr_flag == 0 || p.in_trans == TRANS_WRITE);

    let mut bt = p_bt_rc.borrow_mut();
    debug_assert!(bt.p_page_1.is_some());
    debug_assert!(wr_flag == 0 || (bt.bts_flags & BTS_READ_ONLY) == 0);

    if i_table <= 1 {
        if i_table < 1 {
            return SQLITE_CORRUPT_BKPT;
        } else if btree_pagecount(&bt) == 0 {
            debug_assert!(wr_flag == 0);
            i_table = 0;
        }
    }

    // Agora que nenhum outro erro pode ocorrer, termina de preencher as
    // variáveis do BtCursor e encadeia o cursor na lista do BtShared.
    {
        let mut cur = p_cur.borrow_mut();
        cur.pgno_root = i_table;
        cur.i_page = -1;
        cur.p_key_info = p_key_info;
        cur.p_btree = Some(p.to_weak());
        cur.p_bt = Some(p_bt_rc.clone());
        cur.cur_flags = 0;
    }
    // Se há dois ou mais cursores na mesma btree, todos esses cursores
    // *precisam* ter o flag BTCF_MULTIPLE definido.
    let mut p_x = bt.p_cursor.clone();
    while let Some(p_x_ref) = p_x {
        if p_x_ref.borrow().pgno_root == i_table {
            p_x_ref.borrow_mut().cur_flags |= BTCF_MULTIPLE;
            p_cur.borrow_mut().cur_flags = BTCF_MULTIPLE;
        }
        let p_next = p_x_ref.borrow().p_next.clone();
        p_x = p_next;
    }
    {
        let mut cur = p_cur.borrow_mut();
        cur.e_state = CURSOR_INVALID;
        cur.p_next = bt.p_cursor.take();
    }
    bt.p_cursor = Some(p_cur.clone());
    if wr_flag != 0 {
        {
            let mut cur = p_cur.borrow_mut();
            cur.cur_flags |= BTCF_WRITEFLAG;
            cur.cur_pager_flags = 0;
        }
        if bt.p_tmp_space.is_none() {
            return allocate_temp_space(&mut bt);
        }
    } else {
        p_cur.borrow_mut().cur_pager_flags = PAGER_GET_READONLY;
    }
    SQLITE_OK
}


// ---- part_012.rs ----

// Modelo adotado nesta parte (o integrador precisa honrar):
//  - `BtCursor.p_bt: Option<BtSharedRef>`, `p_btree: Option<BtreeRef>`, `p_page: Option<MemPageRef>`,
//    `ap_page: Vec<Option<MemPageRef>>`, `ai_idx: Vec<u16>`, `a_overflow: Vec<u32>`, `p_key: Vec<u8>`.
//  - `BtCursor.p_self: Weak<RefCell<BtCursor>>` identifica o cursor na lista encadeada de `BtShared`.
//  - `CellInfo.p_payload` é um índice `usize` dentro de `MemPage.a_data` (aritmética de ponteiro vira índice).
//  - `Btree.sharable` é `bool`.
//  - A rotina estática `btreeCursor` vira `btree_cursor_impl`, porque `btree_cursor` é o nome de
//    `sqlite3BtreeCursor` (colisão no mesmo módulo).

/// Abre um cursor da árvore-B com o mutex do Btree adquirido e depois liberado.
/// Rotina auxiliar de `btree_cursor`.
fn btree_cursor_with_lock(
    p: &BtreeRef,
    i_table: u32,
    wr_flag: i32,
    p_key_info: Option<std::rc::Rc<KeyInfo>>,
    p_cur: &mut BtCursor,
) -> i32 {
    btree_enter(p);
    let rc = btree_cursor_impl(p, i_table, wr_flag, p_key_info, p_cur);
    btree_leave(p);
    rc
}

/// Abre um cursor da árvore-B para a tabela fornecida. Se o Btree é compartilhável,
/// o cursor é aberto dentro do mutex.
pub fn btree_cursor(
    p: &BtreeRef,
    i_table: u32,
    wr_flag: i32,
    p_key_info: Option<std::rc::Rc<KeyInfo>>,
    p_cur: &mut BtCursor,
) -> i32 {
    if p.borrow().sharable {
        btree_cursor_with_lock(p, i_table, wr_flag, p_key_info, p_cur)
    } else {
        btree_cursor_impl(p, i_table, wr_flag, p_key_info, p_cur)
    }
}

/// Retorna o tamanho de um objeto BtCursor em bytes.
pub fn btree_cursor_size() -> i32 {
    round8(std::mem::size_of::<BtCursor>() as i32)
}

/// Inicializa a memória que será convertida em um BtCursor: zera os campos
/// anteriores a BTCURSOR_FIRST_UNINIT (`p_bt`) e deixa os demais como estão.
pub fn btree_cursor_zero(p: &mut BtCursor) {
    p.e_state = 0;
    p.cur_flags = 0;
    p.cur_pager_flags = 0;
    p.hints = 0;
    p.skip_next = 0;
    p.p_btree = None;
    p.a_overflow = Vec::new();
    p.p_key = Vec::new();
}

/// Fecha um cursor. O travamento de leitura no arquivo é liberado quando o
/// último cursor é fechado.
pub fn btree_close_cursor(p_cur: &mut BtCursor) -> i32 {
    let p_btree = match p_cur.p_btree.clone() {
        Some(b) => b,
        None => return SQLITE_OK,
    };
    let p_bt = p_cur.p_bt.clone().unwrap();
    btree_enter(&p_btree);
    let self_ref = p_cur.p_self.upgrade().unwrap();
    let first = p_bt.borrow().p_cursor.clone();
    debug_assert!(first.is_some());
    let first = first.unwrap();
    if std::rc::Rc::ptr_eq(&first, &self_ref) {
        p_bt.borrow_mut().p_cursor = p_cur.p_next.take();
    } else {
        let mut p_prev = first;
        loop {
            let next = p_prev.borrow().p_next.clone();
            match next {
                Some(n) => {
                    if std::rc::Rc::ptr_eq(&n, &self_ref) {
                        p_prev.borrow_mut().p_next = p_cur.p_next.take();
                        break;
                    }
                    p_prev = n;
                }
                None => break,
            }
        }
    }
    btree_release_all_cursor_pages(p_cur);
    unlock_btree_if_unused(&p_bt);
    p_cur.a_overflow = Vec::new();
    p_cur.p_key = Vec::new();
    let single = (p_bt.borrow().open_flags & BTREE_SINGLE) != 0 && p_bt.borrow().p_cursor.is_none();
    if single {
        // Como o BtShared não é compartilhável, não há necessidade de se
        // preocupar com a chamada btree_leave() faltante aqui.
        debug_assert!(!p_btree.borrow().sharable);
        btree_close(&p_btree);
    } else {
        btree_leave(&p_btree);
    }
    p_cur.p_btree = None;
    SQLITE_OK
}

/// Compara dois CellInfo (usado só dentro de asserções).
fn cell_info_equal(a: &CellInfo, b: &CellInfo) -> bool {
    a.n_key == b.n_key
        && a.p_payload == b.p_payload
        && a.n_payload == b.n_payload
        && a.n_local == b.n_local
        && a.n_size == b.n_size
}

/// Verifica que o cache `info` do cursor confere com a célula atual (só em asserções).
fn assert_cell_info(p_cur: &BtCursor) {
    let mut info = CellInfo::default();
    let page = p_cur.p_page.clone().unwrap();
    btree_parse_cell(&page.borrow(), p_cur.ix as i32, &mut info);
    debug_assert!(corrupt_db() || cell_info_equal(&info, &p_cur.info));
}

/// Garante que `BtCursor.info` do cursor é válido: se ainda não for, chama
/// `btree_parse_cell()` para preenchê-lo. `info` é um cache da célula atual.
fn get_cell_info(p_cur: &mut BtCursor) {
    if p_cur.info.n_size == 0 {
        p_cur.cur_flags |= BTCF_VALID_NKEY;
        let page = p_cur.p_page.clone().unwrap();
        btree_parse_cell(&page.borrow(), p_cur.ix as i32, &mut p_cur.info);
    } else if cfg!(debug_assertions) {
        assert_cell_info(p_cur);
    }
}

/// Retorna verdadeiro se o cursor é válido: aponta para uma linha de uma tabela
/// não vazia. Usada só dentro de asserções.
pub fn btree_cursor_is_valid(p_cur: Option<&BtCursor>) -> i32 {
    match p_cur {
        Some(cur) => (cur.e_state == CURSOR_VALID) as i32,
        None => 0,
    }
}

/// Como `btree_cursor_is_valid`, assumindo cursor não nulo.
pub fn btree_cursor_is_valid_nn(p_cur: &BtCursor) -> i32 {
    (p_cur.e_state == CURSOR_VALID) as i32
}

/// Retorna a chave inteira ("rowid") de uma b-tree de tabela. Indefinido se o
/// cursor aponta para um índice ou é inválido.
pub fn btree_integer_key(p_cur: &mut BtCursor) -> i64 {
    debug_assert!(p_cur.e_state == CURSOR_VALID);
    debug_assert!(p_cur.cur_int_key != 0);
    get_cell_info(p_cur);
    p_cur.info.n_key
}

/// Fixa o cursor.
pub fn btree_cursor_pin(p_cur: &mut BtCursor) {
    debug_assert!((p_cur.cur_flags & BTCF_PINNED) == 0);
    p_cur.cur_flags |= BTCF_PINNED;
}

/// Desfixa o cursor.
pub fn btree_cursor_unpin(p_cur: &mut BtCursor) {
    debug_assert!((p_cur.cur_flags & BTCF_PINNED) != 0);
    p_cur.cur_flags &= !BTCF_PINNED;
}

/// Retorna o deslocamento no arquivo de banco de dados do início da carga útil
/// para a qual o cursor aponta.
pub fn btree_offset(p_cur: &mut BtCursor) -> i64 {
    debug_assert!(p_cur.e_state == CURSOR_VALID);
    get_cell_info(p_cur);
    let page_size = p_cur.p_bt.as_ref().unwrap().borrow().page_size as i64;
    let pgno = p_cur.p_page.as_ref().unwrap().borrow().pgno as i64;
    page_size * (pgno - 1) + (p_cur.info.p_payload as i64)
}

/// Retorna o número de bytes de carga útil da entrada para a qual o cursor aponta.
/// Em b-trees de tabela é o tamanho dos dados; em índices, o da chave.
pub fn btree_payload_size(p_cur: &mut BtCursor) -> u32 {
    debug_assert!(p_cur.e_state == CURSOR_VALID);
    get_cell_info(p_cur);
    p_cur.info.n_payload
}

/// Retorna um limite superior para o tamanho de qualquer registro da tabela
/// do cursor: o tamanho do arquivo de banco de dados subjacente.
pub fn btree_max_record_size(p_cur: &BtCursor) -> i64 {
    debug_assert!(p_cur.e_state == CURSOR_VALID);
    let bt = p_cur.p_bt.as_ref().unwrap().borrow();
    (bt.page_size as i64) * (bt.n_page as i64)
}

/// Dada a página de transbordamento `ovfl`, encontra o número da próxima página
/// da lista encadeada. Se possível usa o mapa de ponteiros do auto-vacuum em vez
/// de ler a página. O número vai para `*p_pgno_next` (zero se for a última).
/// Se `pp_page` é `Some`, recebe a referência à MemPage obtida (o chamador chama
/// `release_page`); se a referência não foi obtida, recebe `None`.
fn get_overflow_page(
    p_bt: &BtSharedRef,
    ovfl: u32,
    pp_page: Option<&mut Option<MemPageRef>>,
    p_pgno_next: &mut u32,
) -> i32 {
    let mut next: u32 = 0;
    let mut p_page: Option<MemPageRef> = None;
    let mut rc: i32 = SQLITE_OK;

    // Tenta achar a próxima página da lista usando o mapa de ponteiros do
    // auto-vacuum. Chuta que a próxima é ovfl+1; se errar, lê os dados de ovfl.
    if p_bt.borrow().auto_vacuum != 0 {
        let mut pgno: u32 = 0;
        let mut e_type: u8 = 0;
        let mut i_guess: u32 = ovfl + 1;

        while ptrmap_ispage(&p_bt.borrow(), i_guess) || i_guess == pending_byte_page(&p_bt.borrow()) {
            i_guess += 1;
        }

        if i_guess <= btree_pagecount(p_bt) {
            rc = ptrmap_get(p_bt, i_guess, &mut e_type, Some(&mut pgno));
            if rc == SQLITE_OK && e_type == PTRMAP_OVERFLOW2 && pgno == ovfl {
                next = i_guess;
                rc = SQLITE_DONE;
            }
        }
    }

    debug_assert!(next == 0 || rc == SQLITE_DONE);
    let want_page = pp_page.is_some();
    if rc == SQLITE_OK {
        rc = btree_get_page(p_bt, ovfl, &mut p_page, if !want_page { PAGER_GET_READONLY } else { 0 });
        debug_assert!(rc == SQLITE_OK || p_page.is_none());
        if rc == SQLITE_OK {
            next = get4byte(&p_page.as_ref().unwrap().borrow().a_data, 0);
        }
    }

    *p_pgno_next = next;
    match pp_page {
        Some(pp) => *pp = p_page,
        None => release_page(p_page),
    }
    if rc == SQLITE_DONE { SQLITE_OK } else { rc }
}

/// Copia `n_byte` bytes entre `p_payload` e `p_buf` (só a cópia, sem o
/// `pager_write`): `e_op` falso copia da página para o buffer, verdadeiro copia
/// do buffer para a página.
fn copy_payload_bytes(p_payload: &mut [u8], p_buf: &mut [u8], n_byte: usize, e_op: i32) {
    if e_op != 0 {
        p_payload[..n_byte].copy_from_slice(&p_buf[..n_byte]);
    } else {
        p_buf[..n_byte].copy_from_slice(&p_payload[..n_byte]);
    }
}

/// Copia dados entre o buffer e uma MemPage (a partir do índice `pos` de
/// `a_data`). Em escrita chama antes `pager_write` na página do pager.
fn copy_payload(p_page: &MemPageRef, pos: usize, p_buf: &mut [u8], n_byte: usize, e_op: i32) -> i32 {
    if e_op != 0 {
        // Copia dados do buffer para a página (operação de escrita).
        let db_page = p_page.borrow().p_db_page.clone();
        let rc = pager_write(&db_page);
        if rc != SQLITE_OK {
            return rc;
        }
    }
    if e_op != 0 {
        let mut pg = p_page.borrow_mut();
        copy_payload_bytes(&mut pg.a_data[pos..], p_buf, n_byte, e_op);
    } else {
        // Leitura: empresta só para leitura, porque o chamador pode ter
        // outros empréstimos da mesma página.
        let pg = p_page.borrow();
        p_buf[..n_byte].copy_from_slice(&pg.a_data[pos..pos + n_byte]);
    }
    SQLITE_OK
}

/// Lê ou sobrescreve a carga útil da entrada para a qual o cursor aponta:
/// `e_op` 0 lê, 1 escreve, ambos povoando o cache de transbordamento. Transfere
/// `amt` bytes a partir de `offset` entre a carga útil e `p_buf`. O conteúdo pode
/// estar na página principal ou espalhado por páginas de transbordamento. O cache
/// `BtCursor.a_overflow` é alocado e povoado de forma preguiçosa.
/// (O caminho SQLITE_DIRECT_OVERFLOW_READ não faz parte das opções do Debian e fica de fora.)
fn access_payload(
    p_cur: &mut BtCursor,
    mut offset: u32,
    mut amt: u32,
    p_buf: &mut [u8],
    e_op: i32,
) -> i32 {
    let mut rc: i32 = SQLITE_OK;
    let mut i_idx: usize = 0;
    let p_page = p_cur.p_page.clone().unwrap();
    let p_bt = p_cur.p_bt.clone().unwrap();

    debug_assert!(e_op == 0 || e_op == 1);
    debug_assert!(p_cur.e_state == CURSOR_VALID);
    if p_cur.ix >= p_page.borrow().n_cell {
        return sqlite_corrupt_page(&p_page.borrow());
    }

    get_cell_info(p_cur);
    let a_payload: usize = p_cur.info.p_payload;
    debug_assert!(offset + amt <= p_cur.info.n_payload);

    debug_assert!(a_payload > 0);
    let usable_size = p_bt.borrow().usable_size;
    if a_payload > (usable_size.wrapping_sub(p_cur.info.n_local as u32)) as usize {
        // Tentar ler ou escrever além do fim dos dados é um erro. A condição
        // acima é reescrita assim para evitar overflow de inteiro.
        return sqlite_corrupt_page(&p_page.borrow());
    }

    let mut p_buf: &mut [u8] = p_buf;

    // Verifica se dados precisam ser lidos/escritos da/para a própria página da b-tree.
    if offset < p_cur.info.n_local as u32 {
        let mut a = amt;
        if a + offset > p_cur.info.n_local as u32 {
            a = p_cur.info.n_local as u32 - offset;
        }
        rc = copy_payload(&p_page, a_payload + offset as usize, p_buf, a as usize, e_op);
        offset = 0;
        let tmp = std::mem::take(&mut p_buf);
        p_buf = &mut tmp[a as usize..];
        amt -= a;
    } else {
        offset -= p_cur.info.n_local as u32;
    }

    if rc == SQLITE_OK && amt > 0 {
        let ovfl_size: u32 = usable_size - 4; // Bytes de conteúdo por página de transbordamento
        let mut next_page: u32;

        next_page = get4byte(&p_page.borrow().a_data, a_payload + p_cur.info.n_local as usize);

        // Se BtCursor.a_overflow[] não foi alocado, aloca agora. Há uma entrada
        // por página da cadeia de transbordamento; 0 significa "ainda não sabido".
        if (p_cur.cur_flags & BTCF_VALID_OVFL) == 0 {
            let n_ovfl =
                ((p_cur.info.n_payload - p_cur.info.n_local as u32 + ovfl_size - 1) / ovfl_size) as usize;
            if p_cur.a_overflow.is_empty() || n_ovfl > p_cur.a_overflow.len() {
                // O C usa sqlite3Realloc(nOvfl*2*sizeof(Pgno)); o Vec cresce igual.
                p_cur.a_overflow.resize(n_ovfl * 2, 0);
            }
            for x in p_cur.a_overflow[..n_ovfl].iter_mut() {
                *x = 0;
            }
            p_cur.cur_flags |= BTCF_VALID_OVFL;
        } else {
            // Verificação de sanidade do cache de páginas de transbordamento.
            debug_assert!(
                p_cur.a_overflow[0] == next_page || p_cur.a_overflow[0] == 0 || corrupt_db()
            );
            debug_assert!(
                p_cur.a_overflow[0] != 0 || p_cur.a_overflow[(offset / ovfl_size) as usize] == 0
            );

            // Se o cache existe e a entrada da primeira página requerida é
            // válida, pula direto para ela.
            if p_cur.a_overflow[(offset / ovfl_size) as usize] != 0 {
                i_idx = (offset / ovfl_size) as usize;
                next_page = p_cur.a_overflow[i_idx];
                offset %= ovfl_size;
            }
        }

        debug_assert!(rc == SQLITE_OK && amt > 0);
        while next_page != 0 {
            // Se necessário, povoa o cache da lista de páginas de transbordamento.
            if next_page > p_bt.borrow().n_page {
                return sqlite_corrupt_bkpt();
            }
            if i_idx >= p_cur.a_overflow.len() {
                return sqlite_corrupt_bkpt();
            }
            debug_assert!(
                p_cur.a_overflow[i_idx] == 0 || p_cur.a_overflow[i_idx] == next_page || corrupt_db()
            );
            p_cur.a_overflow[i_idx] = next_page;

            if offset >= ovfl_size {
                // O único motivo para ler esta página é obter o número da
                // próxima da cadeia. Tenta o cache e, se faltar, get_overflow_page().
                debug_assert!((p_cur.cur_flags & BTCF_VALID_OVFL) != 0);
                let cached = p_cur.a_overflow.get(i_idx + 1).copied().unwrap_or(0);
                if cached != 0 {
                    next_page = cached;
                } else {
                    let mut nx: u32 = 0;
                    rc = get_overflow_page(&p_bt, next_page, None, &mut nx);
                    next_page = nx;
                }
                offset -= ovfl_size;
            } else {
                // Precisa ler esta página de verdade: ela contém parte do
                // intervalo lido (e_op==0) ou escrito (e_op!=0).
                let mut a = amt;
                if a + offset > ovfl_size {
                    a = ovfl_size - offset;
                }

                let mut p_db_page: Option<PgHdrRef> = None;
                let p_pager = p_bt.borrow().p_pager.clone();
                rc = pager_get(
                    &p_pager,
                    next_page,
                    &mut p_db_page,
                    if e_op == 0 { PAGER_GET_READONLY } else { 0 },
                );
                if rc == SQLITE_OK {
                    let db_page = p_db_page.take().unwrap();
                    next_page = get4byte(&db_page.borrow().p_data, 0);
                    let mut rc2 = SQLITE_OK;
                    if e_op != 0 {
                        rc2 = pager_write(&db_page);
                    }
                    if rc2 == SQLITE_OK {
                        let mut d = db_page.borrow_mut();
                        copy_payload_bytes(&mut d.p_data[offset as usize + 4..], p_buf, a as usize, e_op);
                    }
                    rc = rc2;
                    pager_unref(db_page);
                    offset = 0;
                }
                amt -= a;
                if amt == 0 {
                    return rc;
                }
                let tmp = std::mem::take(&mut p_buf);
                p_buf = &mut tmp[a as usize..];
            }
            if rc != 0 {
                break;
            }
            i_idx += 1;
        }
    }

    if rc == SQLITE_OK && amt > 0 {
        // A cadeia de transbordamento termina prematuramente.
        return sqlite_corrupt_page(&p_page.borrow());
    }
    rc
}


// ---- part_013.rs ----

/// Lê parte da carga útil da linha para a qual o cursor aponta: `amt` bytes são
/// transferidos para `p_buf[]` a partir de `offset`. O cursor pode apontar para
/// uma b-tree de tabela (lê a seção de conteúdo) ou de índice (lê a chave).
/// O chamador garante que o cursor é válido. Retorna SQLITE_OK ou um código de erro.
pub fn btree_payload(p_cur: &mut BtCursor, offset: u32, amt: u32, p_buf: &mut [u8]) -> i32 {
    debug_assert!(p_cur.e_state == CURSOR_VALID);
    debug_assert!(p_cur.i_page >= 0 && p_cur.p_page.is_some());
    access_payload(p_cur, offset, amt, p_buf, 0)
}

/// Variante de `btree_payload` que funciona mesmo com o cursor fora de
/// CURSOR_VALID. Usada apenas por `sqlite3_blob_read()`.
fn access_payload_checked(p_cur: &mut BtCursor, offset: u32, amt: u32, p_buf: &mut [u8]) -> i32 {
    if p_cur.e_state == CURSOR_INVALID {
        return SQLITE_ABORT;
    }
    let rc = btree_restore_cursor_position(p_cur);
    if rc != 0 {
        rc
    } else {
        access_payload(p_cur, offset, amt, p_buf, 0)
    }
}

/// Ver `access_payload_checked`.
pub fn btree_payload_checked(p_cur: &mut BtCursor, offset: u32, amt: u32, p_buf: &mut [u8]) -> i32 {
    if p_cur.e_state == CURSOR_VALID {
        access_payload(p_cur, offset, amt, p_buf, 0)
    } else {
        access_payload_checked(p_cur, offset, amt, p_buf)
    }
}

/// Retorna o índice (dentro de `a_data` da página atual) da carga útil local da
/// entrada para a qual o cursor aponta: o início da chave em b-trees de índice
/// e os dados em b-trees de tabela. O número de bytes disponíveis vai para
/// `*p_amt`; se for 0 o valor retornado não é um índice válido. É uma
/// otimização para o caso comum sem páginas de transbordamento. O índice vale
/// só até a próxima chamada a qualquer rotina da btree. (Funde `fetchPayload`
/// e `sqlite3BtreePayloadFetch`, que só repassava a chamada.)
pub fn btree_payload_fetch(p_cur: &BtCursor, p_amt: &mut u32) -> usize {
    debug_assert!(p_cur.i_page >= 0 && p_cur.p_page.is_some());
    debug_assert!(p_cur.e_state == CURSOR_VALID);
    let page = p_cur.p_page.as_ref().unwrap().borrow();
    debug_assert!(p_cur.ix < page.n_cell || corrupt_db());
    debug_assert!(p_cur.info.n_size > 0);
    debug_assert!(p_cur.info.p_payload > 0 || corrupt_db());
    debug_assert!(p_cur.info.p_payload < page.a_data_end || corrupt_db());
    let mut amt: i32 = p_cur.info.n_local as i32;
    let room = page.a_data_end as i64 - p_cur.info.p_payload as i64;
    if amt as i64 > room {
        // Há pouco espaço na página para a quantidade esperada de conteúdo
        // local: o banco de dados está corrompido.
        debug_assert!(corrupt_db());
        amt = std::cmp::max(0, room as i32);
    }
    *p_amt = amt as u32;
    p_cur.info.p_payload
}

/// Move o cursor para uma nova página filha `new_pgno`. Retorna SQLITE_CORRUPT
/// se as bandeiras do cabeçalho da filha não casam com as do pai (página intkey
/// como pai de página não intkey, ou o contrário).
fn move_to_child(p_cur: &mut BtCursor, new_pgno: u32) -> i32 {
    debug_assert!(p_cur.e_state == CURSOR_VALID);
    debug_assert!((p_cur.i_page as i32) < BTCURSOR_MAX_DEPTH);
    debug_assert!(p_cur.i_page >= 0);
    if p_cur.i_page as i32 >= (BTCURSOR_MAX_DEPTH - 1) {
        return sqlite_corrupt_bkpt();
    }
    p_cur.info.n_size = 0;
    p_cur.cur_flags &= !(BTCF_VALID_NKEY | BTCF_VALID_OVFL);
    p_cur.ai_idx[p_cur.i_page as usize] = p_cur.ix;
    p_cur.ap_page[p_cur.i_page as usize] = p_cur.p_page.clone();
    p_cur.ix = 0;
    p_cur.i_page += 1;
    let p_bt = p_cur.p_bt.clone().unwrap();
    let mut rc = get_and_init_page(&p_bt, new_pgno, &mut p_cur.p_page, p_cur.cur_pager_flags as i32);
    debug_assert!(p_cur.p_page.is_some() || rc != SQLITE_OK);
    if rc == SQLITE_OK {
        let bad = {
            let pg = p_cur.p_page.as_ref().unwrap().borrow();
            pg.n_cell < 1 || pg.int_key != p_cur.cur_int_key
        };
        if bad {
            release_page(p_cur.p_page.take());
            rc = sqlite_corrupt_pgno(new_pgno);
        }
    }
    if rc != 0 {
        p_cur.i_page -= 1;
        p_cur.p_page = p_cur.ap_page[p_cur.i_page as usize].clone();
    }
    rc
}

/// A página `p_parent` é interna (não folha). Faz a asserção de que o número de
/// página `i_child` é o filho esquerdo da célula `i_idx` de `p_parent` ou, se
/// `i_idx` é igual ao número de células, o filho direito (só SQLITE_DEBUG).
fn assert_parent_index(p_parent: &MemPage, i_idx: i32, i_child: u32) {
    if corrupt_db() {
        return;
    }
    debug_assert!(i_idx <= p_parent.n_cell as i32);
    if i_idx == p_parent.n_cell as i32 {
        debug_assert!(get4byte(&p_parent.a_data, p_parent.hdr_offset as usize + 8) == i_child);
    } else {
        debug_assert!(get4byte(&p_parent.a_data, find_cell(p_parent, i_idx)) == i_child);
    }
}

/// Move o cursor para a página pai. `ix` passa a ser o índice da célula que
/// contém o ponteiro para a página de onde viemos (um a mais que o maior índice
/// de célula se viemos da filha mais à direita).
fn move_to_parent(p_cur: &mut BtCursor) {
    debug_assert!(p_cur.e_state == CURSOR_VALID);
    debug_assert!(p_cur.i_page > 0);
    debug_assert!(p_cur.p_page.is_some());
    if cfg!(debug_assertions) {
        let parent = p_cur.ap_page[p_cur.i_page as usize - 1].clone().unwrap();
        let pgno = p_cur.p_page.as_ref().unwrap().borrow().pgno;
        assert_parent_index(
            &parent.borrow(),
            p_cur.ai_idx[p_cur.i_page as usize - 1] as i32,
            pgno,
        );
    }
    p_cur.info.n_size = 0;
    p_cur.cur_flags &= !(BTCF_VALID_NKEY | BTCF_VALID_OVFL);
    p_cur.ix = p_cur.ai_idx[p_cur.i_page as usize - 1];
    let p_leaf = p_cur.p_page.take();
    p_cur.i_page -= 1;
    p_cur.p_page = p_cur.ap_page[p_cur.i_page as usize].clone();
    release_page_not_null(p_leaf.unwrap());
}

/// Move o cursor para a página raiz da sua b-tree. Se a tabela tem página raiz
/// virtual (raiz real sem células e com uma única filha, só a tabela da página 1),
/// o cursor vai para ela. Se a b-tree está vazia, o estado vira CURSOR_INVALID e
/// retorna SQLITE_EMPTY; senão aponta para a primeira célula e o estado vira
/// CURSOR_VALID. Em sucesso, as bandeiras da raiz são do tipo esperado (tabela
/// 0x05/0x0D sem KeyInfo, índice 0x02/0x0A com KeyInfo).
fn move_to_root(p_cur: &mut BtCursor) -> i32 {
    let mut rc = SQLITE_OK;
    let mut skip_init = false;

    debug_assert!(CURSOR_INVALID < CURSOR_REQUIRESEEK);
    debug_assert!(CURSOR_VALID < CURSOR_REQUIRESEEK);
    debug_assert!(CURSOR_FAULT > CURSOR_REQUIRESEEK);
    debug_assert!(p_cur.e_state < CURSOR_REQUIRESEEK || p_cur.i_page < 0);
    debug_assert!(p_cur.pgno_root > 0 || p_cur.i_page < 0);

    if p_cur.i_page >= 0 {
        if p_cur.i_page != 0 {
            release_page_not_null(p_cur.p_page.take().unwrap());
            loop {
                p_cur.i_page -= 1;
                if p_cur.i_page == 0 {
                    break;
                }
                release_page_not_null(p_cur.ap_page[p_cur.i_page as usize].clone().unwrap());
            }
            p_cur.p_page = p_cur.ap_page[0].clone();
            skip_init = true;
        }
    } else if p_cur.pgno_root == 0 {
        p_cur.e_state = CURSOR_INVALID;
        return SQLITE_EMPTY;
    } else {
        debug_assert!(p_cur.i_page == -1);
        if p_cur.e_state >= CURSOR_REQUIRESEEK {
            if p_cur.e_state == CURSOR_FAULT {
                debug_assert!(p_cur.skip_next != SQLITE_OK);
                return p_cur.skip_next;
            }
            btree_clear_cursor(p_cur);
        }
        let p_bt = p_cur.p_bt.clone().unwrap();
        rc = get_and_init_page(&p_bt, p_cur.pgno_root, &mut p_cur.p_page, p_cur.cur_pager_flags as i32);
        if rc != SQLITE_OK {
            p_cur.e_state = CURSOR_INVALID;
            return rc;
        }
        p_cur.i_page = 0;
        p_cur.cur_int_key = p_cur.p_page.as_ref().unwrap().borrow().int_key;
    }
    let p_root = p_cur.p_page.clone().unwrap();
    debug_assert!(p_root.borrow().pgno == p_cur.pgno_root || corrupt_db());

    // Se p_key_info não é nulo, quem abriu o cursor esperava uma b-tree de
    // índice; se é nulo, uma de tabela. Se não for o caso, SQLITE_CORRUPT. (Em
    // bancos corrompidos a raiz pode estar ligada a uma segunda tabela, então o
    // teste vale mesmo com a raiz já carregada.)
    if !skip_init {
        let bad = {
            let r = p_root.borrow();
            debug_assert!(r.int_key == 1 || r.int_key == 0);
            r.is_init == 0 || (p_cur.p_key_info.is_none() as u8) != r.int_key
        };
        if bad {
            return sqlite_corrupt_page(&p_cur.p_page.as_ref().unwrap().borrow());
        }
    }

    // skip_init:
    p_cur.ix = 0;
    p_cur.info.n_size = 0;
    p_cur.cur_flags &= !(BTCF_AT_LAST | BTCF_VALID_NKEY | BTCF_VALID_OVFL);

    let (n_cell, leaf, pgno) = {
        let r = p_root.borrow();
        (r.n_cell, r.leaf, r.pgno)
    };
    if n_cell > 0 {
        p_cur.e_state = CURSOR_VALID;
    } else if leaf == 0 {
        if pgno != 1 {
            return sqlite_corrupt_bkpt();
        }
        let subpage = {
            let r = p_root.borrow();
            get4byte(&r.a_data, r.hdr_offset as usize + 8)
        };
        p_cur.e_state = CURSOR_VALID;
        rc = move_to_child(p_cur, subpage);
    } else {
        p_cur.e_state = CURSOR_INVALID;
        rc = SQLITE_EMPTY;
    }
    rc
}

/// Move o cursor para a folha mais à esquerda abaixo da entrada para a qual
/// aponta: a de menor chave, a primeira em ordem ascendente.
fn move_to_leftmost(p_cur: &mut BtCursor) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(p_cur.e_state == CURSOR_VALID);
    while rc == SQLITE_OK {
        let p_page = p_cur.p_page.clone().unwrap();
        let pgno = {
            let pg = p_page.borrow();
            if pg.leaf != 0 {
                break;
            }
            debug_assert!(p_cur.ix < pg.n_cell);
            get4byte(&pg.a_data, find_cell(&pg, p_cur.ix as i32))
        };
        rc = move_to_child(p_cur, pgno);
    }
    rc
}

/// Move o cursor para a folha mais à direita abaixo da página para a qual
/// aponta. Diferente de `move_to_leftmost`, que acha a entrada mais à esquerda
/// abaixo da *entrada*, este acha a mais à direita abaixo da *página*: a de
/// maior chave, a última em ordem ascendente.
fn move_to_rightmost(p_cur: &mut BtCursor) -> i32 {
    debug_assert!(p_cur.e_state == CURSOR_VALID);
    let mut p_page;
    loop {
        p_page = p_cur.p_page.clone().unwrap();
        let (pgno, n_cell) = {
            let pg = p_page.borrow();
            if pg.leaf != 0 {
                break;
            }
            (get4byte(&pg.a_data, pg.hdr_offset as usize + 8), pg.n_cell)
        };
        p_cur.ix = n_cell;
        let rc = move_to_child(p_cur, pgno);
        if rc != 0 {
            return rc;
        }
    }
    p_cur.ix = p_page.borrow().n_cell.wrapping_sub(1);
    debug_assert!(p_cur.info.n_size == 0);
    debug_assert!((p_cur.cur_flags & BTCF_VALID_NKEY) == 0);
    SQLITE_OK
}


// ---- part_014.rs ----

/// Move o cursor para a primeira entrada da tabela. Retorna SQLITE_OK em
/// sucesso. `*p_res` vira 0 se o cursor aponta para algo, ou 1 se a tabela está vazia.
pub fn btree_first(p_cur: &mut BtCursor, p_res: &mut i32) -> i32 {
    let mut rc = move_to_root(p_cur);
    if rc == SQLITE_OK {
        debug_assert!(p_cur.p_page.as_ref().unwrap().borrow().n_cell > 0);
        *p_res = 0;
        rc = move_to_leftmost(p_cur);
    } else if rc == SQLITE_EMPTY {
        debug_assert!(
            p_cur.pgno_root == 0
                || (p_cur.p_page.is_some() && p_cur.p_page.as_ref().unwrap().borrow().n_cell == 0)
        );
        *p_res = 1;
        rc = SQLITE_OK;
    }
    rc
}

/// O cursor está em CURSOR_VALID e com BTCF_AT_LAST ligado. Verifica que as
/// bandeiras são verdadeiras num banco consistente (só dentro de asserções).
fn cursor_is_at_last_entry(p_cur: &BtCursor) -> bool {
    for ii in 0..p_cur.i_page as usize {
        if p_cur.ai_idx[ii] != p_cur.ap_page[ii].as_ref().unwrap().borrow().n_cell {
            return false;
        }
    }
    let pg = p_cur.p_page.as_ref().unwrap().borrow();
    p_cur.ix as i32 == pg.n_cell as i32 - 1 && pg.leaf != 0
}

/// Move o cursor para a última entrada da tabela (`btreeLast` do C; o sufixo
/// evita a colisão com `btree_last`). `*p_res` vira 0 se aponta para algo, ou 1
/// se a tabela está vazia.
fn btree_last_impl(p_cur: &mut BtCursor, p_res: &mut i32) -> i32 {
    let mut rc = move_to_root(p_cur);
    if rc == SQLITE_OK {
        debug_assert!(p_cur.e_state == CURSOR_VALID);
        *p_res = 0;
        rc = move_to_rightmost(p_cur);
        if rc == SQLITE_OK {
            p_cur.cur_flags |= BTCF_AT_LAST;
        } else {
            p_cur.cur_flags &= !BTCF_AT_LAST;
        }
    } else if rc == SQLITE_EMPTY {
        debug_assert!(p_cur.pgno_root == 0 || p_cur.p_page.as_ref().unwrap().borrow().n_cell == 0);
        *p_res = 1;
        rc = SQLITE_OK;
    }
    rc
}

/// Move o cursor para a última entrada da tabela. Retorna SQLITE_OK em sucesso.
/// `*p_res` vira 0 se aponta para algo, ou 1 se a tabela está vazia.
pub fn btree_last(p_cur: &mut BtCursor, p_res: &mut i32) -> i32 {
    // Se o cursor já aponta para a última entrada, não há o que fazer.
    if CURSOR_VALID == p_cur.e_state && (p_cur.cur_flags & BTCF_AT_LAST) != 0 {
        debug_assert!(cursor_is_at_last_entry(p_cur) || corrupt_db());
        *p_res = 0;
        return SQLITE_OK;
    }
    btree_last_impl(p_cur, p_res)
}

/// Resultado da busca binária dentro de uma página de tabela.
enum TableStep {
    /// Coincidência exata numa folha: o cursor já foi posicionado.
    Found,
    /// Desce para a filha: `lwr` é o índice da célula (ou n_cell para a filha direita).
    NextLayer(i32),
    /// Sem coincidência: `lwr`, `idx` e o resultado `c` da última comparação.
    Miss(i32, i32, i32),
}

/// Move o cursor para uma entrada de tabela (INTKEY) perto da chave `int_key`.
/// Sem coincidência exata o cursor fica numa folha que conteria a entrada, em
/// uma entrada anterior ou posterior à chave. `*p_res`: <0 o cursor aponta para
/// entrada menor que `int_key` (ou a tabela está vazia e não aponta para nada);
/// 0 coincidência exata; >0 entrada maior que `int_key`.
pub fn btree_table_moveto(p_cur: &mut BtCursor, int_key: i64, bias_right: i32, p_res: &mut i32) -> i32 {
    let mut rc: i32;

    debug_assert!(p_cur.p_key_info.is_none());
    debug_assert!(p_cur.e_state != CURSOR_VALID || p_cur.cur_int_key != 0);

    // Se o cursor já está na posição desejada, retorna sem trabalho.
    if p_cur.e_state == CURSOR_VALID && (p_cur.cur_flags & BTCF_VALID_NKEY) != 0 {
        if p_cur.info.n_key == int_key {
            *p_res = 0;
            return SQLITE_OK;
        }
        if p_cur.info.n_key < int_key {
            if (p_cur.cur_flags & BTCF_AT_LAST) != 0 {
                debug_assert!(cursor_is_at_last_entry(p_cur) || corrupt_db());
                *p_res = -1;
                return SQLITE_OK;
            }
            // Se a chave pedida é uma a mais que a anterior, tenta chegar com
            // btree_next() em vez de busca binária completa. É só otimização.
            if p_cur.info.n_key + 1 == int_key {
                *p_res = 0;
                rc = btree_next(p_cur, 0);
                if rc == SQLITE_OK {
                    get_cell_info(p_cur);
                    if p_cur.info.n_key == int_key {
                        return SQLITE_OK;
                    }
                } else if rc != SQLITE_DONE {
                    return rc;
                }
            }
        }
    }

    rc = move_to_root(p_cur);
    if rc != 0 {
        if rc == SQLITE_EMPTY {
            debug_assert!(p_cur.pgno_root == 0 || p_cur.p_page.as_ref().unwrap().borrow().n_cell == 0);
            *p_res = -1;
            return SQLITE_OK;
        }
        return rc;
    }
    debug_assert!(p_cur.p_page.is_some());
    debug_assert!(p_cur.p_page.as_ref().unwrap().borrow().is_init != 0);
    debug_assert!(p_cur.e_state == CURSOR_VALID);
    debug_assert!(p_cur.p_page.as_ref().unwrap().borrow().n_cell > 0);
    debug_assert!(p_cur.cur_int_key != 0);

    loop {
        let p_page = p_cur.p_page.clone().unwrap();
        // n_cell é maior que zero: se esta é a raiz o cursor teria sido INVALID
        // acima, e se não é, move_to_child() já teria detectado a corrupção.
        let step = {
            let pg = p_page.borrow();
            debug_assert!(pg.n_cell > 0);
            debug_assert!(pg.int_key != 0);
            let mut lwr: i32 = 0;
            let mut upr: i32 = pg.n_cell as i32 - 1;
            debug_assert!(bias_right == 0 || bias_right == 1);
            let mut idx: i32 = upr >> (1 - bias_right); // idx = bias_right ? upr : (lwr+upr)/2
            loop {
                let mut p_cell = find_cell_past_ptr(&pg, idx);
                if pg.int_key_leaf != 0 {
                    loop {
                        let b = pg.a_data[p_cell];
                        p_cell += 1;
                        if b < 0x80 {
                            break;
                        }
                        if p_cell >= pg.a_data_end {
                            return sqlite_corrupt_page(&pg);
                        }
                    }
                }
                let mut n_cell_key_u: u64 = 0;
                get_varint(&pg.a_data[p_cell..], &mut n_cell_key_u);
                let n_cell_key = n_cell_key_u as i64;
                if n_cell_key < int_key {
                    lwr = idx + 1;
                    if lwr > upr {
                        break TableStep::Miss(lwr, idx, -1);
                    }
                } else if n_cell_key > int_key {
                    upr = idx - 1;
                    if lwr > upr {
                        break TableStep::Miss(lwr, idx, 1);
                    }
                } else {
                    debug_assert!(n_cell_key == int_key);
                    p_cur.ix = idx as u16;
                    if pg.leaf == 0 {
                        break TableStep::NextLayer(idx);
                    } else {
                        p_cur.cur_flags |= BTCF_VALID_NKEY;
                        p_cur.info.n_key = n_cell_key;
                        p_cur.info.n_size = 0;
                        *p_res = 0;
                        break TableStep::Found;
                    }
                }
                debug_assert!(lwr + upr >= 0);
                idx = (lwr + upr) >> 1; // idx = (lwr+upr)/2
            }
        };
        let lwr = match step {
            TableStep::Found => return SQLITE_OK,
            TableStep::NextLayer(lwr) => lwr,
            TableStep::Miss(lwr, idx, c) => {
                let (leaf, is_init) = {
                    let pg = p_page.borrow();
                    (pg.leaf, pg.is_init)
                };
                debug_assert!(lwr == p_page.borrow().n_cell as i32 || lwr <= p_page.borrow().n_cell as i32);
                debug_assert!(is_init != 0);
                if leaf != 0 {
                    p_cur.ix = idx as u16;
                    *p_res = c;
                    rc = SQLITE_OK;
                    // moveto_table_finish
                    p_cur.info.n_size = 0;
                    debug_assert!((p_cur.cur_flags & BTCF_VALID_OVFL) == 0);
                    return rc;
                }
                lwr
            }
        };
        // moveto_table_next_layer:
        let chld_pg = {
            let pg = p_page.borrow();
            if lwr >= pg.n_cell as i32 {
                get4byte(&pg.a_data, pg.hdr_offset as usize + 8)
            } else {
                get4byte(&pg.a_data, find_cell(&pg, lwr))
            }
        };
        p_cur.ix = lwr as u16;
        rc = move_to_child(p_cur, chld_pg);
        if rc != 0 {
            break;
        }
    }
    // moveto_table_finish:
    p_cur.info.n_size = 0;
    debug_assert!((p_cur.cur_flags & BTCF_VALID_OVFL) == 0);
    rc
}

/// Compara a célula `idx` da página atual do cursor com `p_idx_key` usando
/// `x_record_compare`. Retorna negativo ou zero se a célula é menor ou igual a
/// `p_idx_key`, e positivo se nada se sabe (é sempre seguro retornar positivo:
/// só faz a otimização ser pulada).
fn index_cell_compare(
    p_cur: &BtCursor,
    idx: i32,
    p_idx_key: &mut UnpackedRecord,
    x_record_compare: RecordCompare,
) -> i32 {
    let p_page = p_cur.p_page.as_ref().unwrap().borrow();
    let p_cell = find_cell_past_ptr(&p_page, idx);
    let d = &p_page.a_data;

    let mut n_cell: i32 = d[p_cell] as i32;
    if n_cell <= p_page.max1_byte_payload as i32 {
        // O campo de tamanho do registro é um varint de 1 byte e o registro
        // cabe inteiro na página principal.
        x_record_compare(n_cell, &d[p_cell + 1..], p_idx_key)
    } else if (d[p_cell + 1] & 0x80) == 0 && {
        n_cell = ((n_cell & 0x7f) << 7) + d[p_cell + 1] as i32;
        n_cell <= p_page.max_local as i32
    } {
        // Varint de 2 bytes e o registro cabe inteiro na página principal.
        x_record_compare(n_cell, &d[p_cell + 2..], p_idx_key)
    } else {
        // Se o registro vai para páginas de transbordamento, não tenta a otimização.
        99
    }
}

/// Retorna verdadeiro (não zero) se o cursor aponta para a última página de uma tabela.
fn cursor_on_last_page(p_cur: &BtCursor) -> i32 {
    debug_assert!(p_cur.e_state == CURSOR_VALID);
    for i in 0..p_cur.i_page as usize {
        let p_page = p_cur.ap_page[i].as_ref().unwrap();
        if p_cur.ai_idx[i] < p_page.borrow().n_cell {
            return 0;
        }
    }
    1
}

/// Move o cursor para uma entrada de índice perto da chave `p_idx_key`. Sem
/// coincidência exata o cursor fica numa folha que conteria a entrada. `*p_res`:
/// <0 aponta para entrada menor que `p_idx_key` (ou tabela vazia); 0
/// coincidência exata; >0 entrada maior. `p_idx_key.eq_seen` vira 1 se existe
/// na tabela uma entrada que coincide exatamente com `p_idx_key`.
pub fn btree_index_moveto(p_cur: &mut BtCursor, p_idx_key: &mut UnpackedRecord, p_res: &mut i32) -> i32 {
    let mut rc: i32;
    let mut bypass_moveto_root = false;

    debug_assert!(p_cur.p_key_info.is_some());

    let x_record_compare: RecordCompare = vdbe_find_compare(p_idx_key);
    p_idx_key.err_code = 0;
    debug_assert!(p_idx_key.default_rc == 1 || p_idx_key.default_rc == 0 || p_idx_key.default_rc == -1);

    // Verifica se dá para pular muito trabalho. Dois casos:
    //  (1) o cursor já aponta para a última célula da tabela e a chave é maior
    //      ou igual a ela: nenhum movimento é necessário;
    //  (2) o cursor está na última página e a primeira célula dela é menor ou
    //      igual à chave: a busca pode começar na página atual, sem voltar à raiz.
    if p_cur.e_state == CURSOR_VALID
        && p_cur.p_page.as_ref().unwrap().borrow().leaf != 0
        && cursor_on_last_page(p_cur) != 0
    {
        let n_cell_pg = p_cur.p_page.as_ref().unwrap().borrow().n_cell as i32;
        if p_cur.ix as i32 == n_cell_pg - 1 {
            let c = index_cell_compare(p_cur, p_cur.ix as i32, p_idx_key, x_record_compare);
            if c <= 0 && p_idx_key.err_code == SQLITE_OK {
                *p_res = c;
                return SQLITE_OK; // Cursor já aponta para o lugar certo
            }
        }
        if p_cur.i_page > 0
            && index_cell_compare(p_cur, 0, p_idx_key, x_record_compare) <= 0
            && p_idx_key.err_code == SQLITE_OK
        {
            p_cur.cur_flags &= !BTCF_VALID_OVFL;
            if p_cur.p_page.as_ref().unwrap().borrow().is_init == 0 {
                return sqlite_corrupt_bkpt();
            }
            bypass_moveto_root = true; // Começa a busca na página atual
        } else {
            p_idx_key.err_code = SQLITE_OK;
        }
    }

    if !bypass_moveto_root {
        rc = move_to_root(p_cur);
        if rc != 0 {
            if rc == SQLITE_EMPTY {
                debug_assert!(p_cur.pgno_root == 0 || p_cur.p_page.as_ref().unwrap().borrow().n_cell == 0);
                *p_res = -1;
                return SQLITE_OK;
            }
            return rc;
        }
    }

    // bypass_moveto_root:
    debug_assert!(p_cur.p_page.is_some());
    debug_assert!(p_cur.p_page.as_ref().unwrap().borrow().is_init != 0);
    debug_assert!(p_cur.e_state == CURSOR_VALID);
    debug_assert!(p_cur.p_page.as_ref().unwrap().borrow().n_cell > 0);
    debug_assert!(p_cur.cur_int_key == 0);
    let p_bt = p_cur.p_bt.clone().unwrap();
    rc = SQLITE_OK;
    'search: loop {
        let p_page = p_cur.p_page.clone().unwrap();
        // n_cell > 0 e a página é do tipo certo (índice): move_to_child() ou
        // move_to_root() já teriam detectado a corrupção.
        let (n_cell_pg, max1, max_local, child_ptr_size, leaf, hdr_offset) = {
            let pg = p_page.borrow();
            debug_assert!(pg.n_cell > 0);
            debug_assert!(pg.int_key == 0);
            (pg.n_cell as i32, pg.max1_byte_payload as i32, pg.max_local as i32, pg.child_ptr_size as usize, pg.leaf, pg.hdr_offset as usize)
        };
        let mut lwr: i32 = 0;
        let mut upr: i32 = n_cell_pg - 1;
        let mut idx: i32 = upr >> 1; // idx = (lwr+upr)/2
        let mut c: i32;
        loop {
            let p_cell = find_cell_past_ptr(&p_page.borrow(), idx);

            // O tamanho máximo de página é 65536, então o máximo de bytes de
            // registro numa página de índice é menor que 16384 e cabe num
            // varint de 2 bytes. Isso evita processar a célula inteira nos
            // casos em que o registro está todo na página.
            let b0 = p_page.borrow().a_data[p_cell];
            let mut n_cell: i32 = b0 as i32;
            if n_cell <= max1 {
                // Varint de 1 byte e o registro cabe inteiro na página principal.
                let pg = p_page.borrow();
                c = x_record_compare(n_cell, &pg.a_data[p_cell + 1..], p_idx_key);
            } else if {
                let pg = p_page.borrow();
                (pg.a_data[p_cell + 1] & 0x80) == 0 && {
                    n_cell = ((n_cell & 0x7f) << 7) + pg.a_data[p_cell + 1] as i32;
                    n_cell <= max_local
                }
            } {
                // Varint de 2 bytes e o registro cabe inteiro na página principal.
                let pg = p_page.borrow();
                c = x_record_compare(n_cell, &pg.a_data[p_cell + 2..], p_idx_key);
            } else {
                // O registro transborda para páginas de transbordamento: a
                // célula inteira é processada, um buffer é alocado e
                // access_payload() recupera o registro antes de
                // vdbe_record_compare(). Se o registro é corrupto, a rotina de
                // comparação pode ler até dois varints além do fim do buffer;
                // 18 bytes de enchimento cobrem isso.
                const N_OVERRUN: usize = 18;
                let p_cell_body = p_cell - child_ptr_size;
                {
                    let pg = p_page.borrow();
                    let f = pg.x_parse_cell;
                    f(&pg, p_cell_body, &mut p_cur.info);
                }
                n_cell = p_cur.info.n_key as i32;
                let usable = p_bt.borrow().usable_size;
                let n_page = p_bt.borrow().n_page;
                if n_cell < 2 || (n_cell as u32) / usable > n_page {
                    rc = sqlite_corrupt_page(&p_page.borrow());
                    break 'search;
                }
                let mut p_cell_key: Vec<u8> = vec![0u8; n_cell as usize + N_OVERRUN];
                p_cur.ix = idx as u16;
                rc = access_payload(p_cur, 0, n_cell as u32, &mut p_cell_key[..n_cell as usize], 0);
                for x in p_cell_key[n_cell as usize..].iter_mut() {
                    *x = 0; // Evita aviso de memória não inicializada
                }
                p_cur.cur_flags &= !BTCF_VALID_OVFL;
                if rc != 0 {
                    break 'search;
                }
                c = vdbe_record_compare(n_cell, &p_cell_key, p_idx_key);
            }
            if c < 0 {
                lwr = idx + 1;
            } else if c > 0 {
                upr = idx - 1;
            } else {
                debug_assert!(c == 0);
                *p_res = 0;
                rc = SQLITE_OK;
                p_cur.ix = idx as u16;
                if p_idx_key.err_code != 0 {
                    rc = sqlite_corrupt_bkpt();
                }
                break 'search;
            }
            if lwr > upr {
                break;
            }
            debug_assert!(lwr + upr >= 0);
            idx = (lwr + upr) >> 1; // idx = (lwr+upr)/2
        }
        debug_assert!(lwr == upr + 1);
        debug_assert!(p_page.borrow().is_init != 0);
        if leaf != 0 {
            debug_assert!((p_cur.ix as i32) < n_cell_pg || corrupt_db());
            p_cur.ix = idx as u16;
            *p_res = c;
            rc = SQLITE_OK;
            break 'search;
        }
        let chld_pg = {
            let pg = p_page.borrow();
            if lwr >= n_cell_pg {
                get4byte(&pg.a_data, hdr_offset + 8)
            } else {
                get4byte(&pg.a_data, find_cell(&pg, lwr))
            }
        };

        // Este bloco é uma versão em linha de:
        //    p_cur.ix = lwr; rc = move_to_child(p_cur, chld_pg); if rc { break }
        p_cur.info.n_size = 0;
        p_cur.cur_flags &= !(BTCF_VALID_NKEY | BTCF_VALID_OVFL);
        if p_cur.i_page as i32 >= (BTCURSOR_MAX_DEPTH - 1) {
            return sqlite_corrupt_bkpt();
        }
        p_cur.ai_idx[p_cur.i_page as usize] = lwr as u16;
        p_cur.ap_page[p_cur.i_page as usize] = p_cur.p_page.clone();
        p_cur.ix = 0;
        p_cur.i_page += 1;
        rc = get_and_init_page(&p_bt, chld_pg, &mut p_cur.p_page, p_cur.cur_pager_flags as i32);
        if rc == SQLITE_OK {
            let bad = {
                let pg = p_cur.p_page.as_ref().unwrap().borrow();
                pg.n_cell < 1 || pg.int_key != p_cur.cur_int_key
            };
            if bad {
                release_page(p_cur.p_page.take());
                rc = sqlite_corrupt_pgno(chld_pg);
            }
        }
        if rc != 0 {
            p_cur.i_page -= 1;
            p_cur.p_page = p_cur.ap_page[p_cur.i_page as usize].clone();
            break 'search;
        }
        // Fim da chamada move_to_child() em linha.
    }
    // moveto_index_finish:
    p_cur.info.n_size = 0;
    debug_assert!((p_cur.cur_flags & BTCF_VALID_OVFL) == 0);
    rc
}


// ---- part_015.rs ----

/// Retorna verdadeiro se o cursor não aponta para uma entrada da tabela. É
/// verdadeiro depois de `btree_next()` passar da última entrada, ou de
/// `btree_previous()` passar da primeira, e também se a tabela está vazia.
pub fn btree_eof(p_cur: &BtCursor) -> i32 {
    // TODO do SQLite: e se o cursor está em CURSOR_REQUIRESEEK mas todas as
    // entradas foram apagadas? Esta API terá de retornar também um código de erro.
    (CURSOR_VALID != p_cur.e_state) as i32
}

/// Retorna uma estimativa do número de linhas da tabela para a qual o cursor
/// aponta, ou um número negativo se não há estimativa disponível.
pub fn btree_row_count_est(p_cur: &BtCursor) -> i64 {
    // Hoje só o opcode OP_IfSizeBetween e o OP_Count com P3=1 chamam isto. Em
    // ambos o cursor é sempre válido, a menos que a btree esteja vazia.
    if p_cur.e_state != CURSOR_VALID {
        return 0;
    }
    if p_cur.p_page.as_ref().unwrap().borrow().leaf == 0 {
        return -1;
    }
    let mut n: i64 = p_cur.p_page.as_ref().unwrap().borrow().n_cell as i64;
    for i in 0..p_cur.i_page as usize {
        n = n.wrapping_mul(p_cur.ap_page[i].as_ref().unwrap().borrow().n_cell as i64);
    }
    n
}

/// Avança o cursor para a próxima entrada (`btreeNext` do C; o sufixo evita a
/// colisão com `btree_next`). Retorna SQLITE_OK, SQLITE_DONE (o cursor já estava
/// no último elemento) ou outro código em caso de erro. É chamada quando é
/// preciso trocar de página ou restaurar o cursor.
fn btree_next_internal(p_cur: &mut BtCursor) -> i32 {
    let mut rc: i32;

    if p_cur.e_state != CURSOR_VALID {
        debug_assert!((p_cur.cur_flags & BTCF_VALID_OVFL) == 0);
        rc = restore_cursor_position(p_cur);
        if rc != SQLITE_OK {
            return rc;
        }
        if CURSOR_INVALID == p_cur.e_state {
            return SQLITE_DONE;
        }
        if p_cur.e_state == CURSOR_SKIPNEXT {
            p_cur.e_state = CURSOR_VALID;
            if p_cur.skip_next > 0 {
                return SQLITE_OK;
            }
        }
    }

    let mut p_page = p_cur.p_page.clone().unwrap();
    p_cur.ix = p_cur.ix.wrapping_add(1);
    let idx = p_cur.ix as i32;
    if p_page.borrow().is_init == 0 {
        return sqlite_corrupt_bkpt();
    }

    if idx >= p_page.borrow().n_cell as i32 {
        if p_page.borrow().leaf == 0 {
            let child = {
                let pg = p_page.borrow();
                get4byte(&pg.a_data, pg.hdr_offset as usize + 8)
            };
            rc = move_to_child(p_cur, child);
            if rc != 0 {
                return rc;
            }
            return move_to_leftmost(p_cur);
        }
        loop {
            if p_cur.i_page == 0 {
                p_cur.e_state = CURSOR_INVALID;
                return SQLITE_DONE;
            }
            move_to_parent(p_cur);
            p_page = p_cur.p_page.clone().unwrap();
            if (p_cur.ix as i32) < p_page.borrow().n_cell as i32 {
                break;
            }
        }
        if p_page.borrow().int_key != 0 {
            return btree_next(p_cur, 0);
        } else {
            return SQLITE_OK;
        }
    }
    if p_page.borrow().leaf != 0 {
        SQLITE_OK
    } else {
        move_to_leftmost(p_cur)
    }
}

/// Avança o cursor para a próxima entrada. Otimizada para o caso comum de só
/// incrementar `ix`; o auxiliar `btree_next_internal` cuida das trocas de página.
/// O bit 0x01 de `flags` indica cursor de índice SQL (dica usada só pelo COMDB2).
pub fn btree_next(p_cur: &mut BtCursor, flags: i32) -> i32 {
    debug_assert!(flags == 0 || flags == 1);
    p_cur.info.n_size = 0;
    p_cur.cur_flags &= !(BTCF_VALID_NKEY | BTCF_VALID_OVFL);
    if p_cur.e_state != CURSOR_VALID {
        return btree_next_internal(p_cur);
    }
    let p_page = p_cur.p_page.clone().unwrap();
    p_cur.ix = p_cur.ix.wrapping_add(1);
    if p_cur.ix >= p_page.borrow().n_cell {
        p_cur.ix -= 1;
        return btree_next_internal(p_cur);
    }
    if p_page.borrow().leaf != 0 {
        SQLITE_OK
    } else {
        move_to_leftmost(p_cur)
    }
}

/// Recua o cursor para a entrada anterior (`btreePrevious` do C; o sufixo evita
/// a colisão com `btree_previous`). Retorna SQLITE_OK, SQLITE_DONE (o cursor já
/// estava no primeiro elemento) ou outro código em caso de erro.
fn btree_previous_internal(p_cur: &mut BtCursor) -> i32 {
    let mut rc: i32;

    debug_assert!((p_cur.cur_flags & (BTCF_AT_LAST | BTCF_VALID_OVFL | BTCF_VALID_NKEY)) == 0);
    debug_assert!(p_cur.info.n_size == 0);
    if p_cur.e_state != CURSOR_VALID {
        rc = restore_cursor_position(p_cur);
        if rc != SQLITE_OK {
            return rc;
        }
        if CURSOR_INVALID == p_cur.e_state {
            return SQLITE_DONE;
        }
        if CURSOR_SKIPNEXT == p_cur.e_state {
            p_cur.e_state = CURSOR_VALID;
            if p_cur.skip_next < 0 {
                return SQLITE_OK;
            }
        }
    }

    let mut p_page = p_cur.p_page.clone().unwrap();
    if p_page.borrow().is_init == 0 {
        return sqlite_corrupt_bkpt();
    }
    if p_page.borrow().leaf == 0 {
        let idx = p_cur.ix as i32;
        let child = {
            let pg = p_page.borrow();
            get4byte(&pg.a_data, find_cell(&pg, idx))
        };
        rc = move_to_child(p_cur, child);
        if rc != 0 {
            return rc;
        }
        rc = move_to_rightmost(p_cur);
    } else {
        while p_cur.ix == 0 {
            if p_cur.i_page == 0 {
                p_cur.e_state = CURSOR_INVALID;
                return SQLITE_DONE;
            }
            move_to_parent(p_cur);
        }
        debug_assert!(p_cur.info.n_size == 0);
        debug_assert!((p_cur.cur_flags & BTCF_VALID_OVFL) == 0);

        p_cur.ix -= 1;
        p_page = p_cur.p_page.clone().unwrap();
        let (int_key, leaf) = {
            let pg = p_page.borrow();
            (pg.int_key, pg.leaf)
        };
        if int_key != 0 && leaf == 0 {
            rc = btree_previous(p_cur, 0);
        } else {
            rc = SQLITE_OK;
        }
    }
    rc
}

/// Recua o cursor para a entrada anterior. Otimizada para o caso comum de só
/// decrementar `ix`; o auxiliar `btree_previous_internal` cuida do resto.
pub fn btree_previous(p_cur: &mut BtCursor, flags: i32) -> i32 {
    debug_assert!(flags == 0 || flags == 1);
    p_cur.cur_flags &= !(BTCF_AT_LAST | BTCF_VALID_OVFL | BTCF_VALID_NKEY);
    p_cur.info.n_size = 0;
    if p_cur.e_state != CURSOR_VALID
        || p_cur.ix == 0
        || p_cur.p_page.as_ref().unwrap().borrow().leaf == 0
    {
        return btree_previous_internal(p_cur);
    }
    p_cur.ix -= 1;
    SQLITE_OK
}

/// Copia 4 bytes de `a_data[src..src+4]` de uma página para `a_data[dst..dst+4]` de outra.
fn copy4(from: &MemPageRef, src: usize, to: &MemPageRef, dst: usize) {
    let bytes: [u8; 4] = from.borrow().a_data[src..src + 4].try_into().unwrap();
    to.borrow_mut().a_data[dst..dst + 4].copy_from_slice(&bytes);
}

/// Aloca uma nova página do arquivo de banco de dados. A página é marcada como
/// suja (`pager_write` já foi chamado) e referenciada: o chamador deve fazer
/// o unref quando terminar. Retorna SQLITE_OK em sucesso; em erro `*pp_page` é
/// None. Se `nearby` não é 0, tenta achar uma página próxima a ele. Com
/// BTALLOC_EXACT e a página `nearby` em qualquer ponto da lista livre, ela é
/// garantidamente a retornada; com BTALLOC_LE a retornada é menor ou igual a
/// `nearby` se existir alguma; com BTALLOC_ANY não há restrição.
fn allocate_btree_page(
    p_bt: &BtSharedRef,
    pp_page: &mut Option<MemPageRef>,
    p_pgno: &mut u32,
    nearby: u32,
    e_mode: u8,
) -> i32 {
    let mut rc: i32;
    let mut p_trunk: Option<MemPageRef> = None;
    let mut p_prev_trunk: Option<MemPageRef> = None;

    debug_assert!(e_mode == BTALLOC_ANY || (nearby > 0 && p_bt.borrow().auto_vacuum != 0));
    let p_page1 = p_bt.borrow().p_page1.clone().unwrap();
    let mx_page: u32 = btree_pagecount(p_bt); // Tamanho total do arquivo de banco de dados
    // EVIDENCE-OF: R-21003-45125 O inteiro big-endian de 4 bytes no offset 36
    // guarda o número total de páginas da lista livre.
    let n: u32 = get4byte(&p_page1.borrow().a_data, 36);
    if n >= mx_page {
        return sqlite_corrupt_bkpt();
    }
    'end_allocate_page: {
        if n > 0 {
            // Há páginas na lista livre. Reutiliza uma delas.
            let mut search_list = false; // Se a lista livre deve ser percorrida atrás de 'nearby'
            let mut n_search: u32 = 0; // Contagem de tentativas de busca

            // Com BTALLOC_EXACT, se o mapa de ponteiros mostra que 'nearby'
            // está na lista livre, a lista inteira é percorrida atrás dessa página.
            if e_mode == BTALLOC_EXACT {
                if nearby <= mx_page {
                    let mut e_type: u8 = 0;
                    debug_assert!(nearby > 0);
                    debug_assert!(p_bt.borrow().auto_vacuum != 0);
                    rc = ptrmap_get(p_bt, nearby, &mut e_type, None);
                    if rc != 0 {
                        return rc;
                    }
                    if e_type == PTRMAP_FREEPAGE {
                        search_list = true;
                    }
                }
            } else if e_mode == BTALLOC_LE {
                search_list = true;
            }

            // Decrementa em 1 a contagem da lista livre.
            let page1_db = p_page1.borrow().p_db_page.clone();
            rc = pager_write(&page1_db);
            if rc != 0 {
                return rc;
            }
            put4byte(&mut p_page1.borrow_mut().a_data, 36, n - 1);

            // O código deste laço roda uma só vez se `search_list` é falso. Do
            // contrário roda uma vez por página tronco da lista livre até achar
            // 'nearby' (BTALLOC_EXACT) ou uma página menor ou igual a ele (BTALLOC_LE).
            loop {
                p_prev_trunk = p_trunk.take();
                let i_trunk: u32 = if let Some(pt) = &p_prev_trunk {
                    // EVIDENCE-OF: R-01506-11053 O primeiro inteiro de uma página
                    // tronco é o número da próxima página tronco, ou zero se esta
                    // é a última.
                    get4byte(&pt.borrow().a_data, 0)
                } else {
                    // EVIDENCE-OF: R-59841-13798 O inteiro no offset 32 guarda o
                    // número da primeira página da lista livre, ou zero se vazia.
                    get4byte(&p_page1.borrow().a_data, 32)
                };
                let too_many = {
                    let r = n_search > n;
                    n_search = n_search.wrapping_add(1);
                    r
                };
                if i_trunk > mx_page || too_many {
                    let pg = match &p_prev_trunk {
                        Some(pt) => pt.borrow().pgno,
                        None => 1,
                    };
                    rc = sqlite_corrupt_pgno(pg);
                } else {
                    rc = btree_get_unused_page(p_bt, i_trunk, &mut p_trunk, 0);
                }
                if rc != 0 {
                    p_trunk = None;
                    break 'end_allocate_page;
                }
                let trunk = p_trunk.clone().unwrap();
                let trunk_db = trunk.borrow().p_db_page.clone();
                // EVIDENCE-OF: R-13523-04394 O segundo inteiro de uma página
                // tronco é o número de ponteiros de folha que seguem.
                let k: u32 = get4byte(&trunk.borrow().a_data, 4);
                let usable_size = p_bt.borrow().usable_size;
                if k == 0 && !search_list {
                    // O tronco não tem folhas e a lista não está sendo
                    // percorrida: extrai a própria página tronco e a usa como a
                    // página recém-alocada.
                    debug_assert!(p_prev_trunk.is_none());
                    rc = pager_write(&trunk_db);
                    if rc != 0 {
                        break 'end_allocate_page;
                    }
                    *p_pgno = i_trunk;
                    copy4(&trunk, 0, &p_page1, 32);
                    *pp_page = p_trunk.take();
                } else if k > usable_size / 4 - 2 {
                    // Valor de k fora da faixa: banco corrompido.
                    rc = sqlite_corrupt_pgno(i_trunk);
                    break 'end_allocate_page;
                } else if search_list && (nearby == i_trunk || (i_trunk < nearby && e_mode == BTALLOC_LE)) {
                    // A lista está sendo percorrida e esta página tronco é a
                    // página a alocar, tenha folhas ou não.
                    *p_pgno = i_trunk;
                    *pp_page = p_trunk.clone();
                    search_list = false;
                    rc = pager_write(&trunk_db);
                    if rc != 0 {
                        break 'end_allocate_page;
                    }
                    if k == 0 {
                        if p_prev_trunk.is_none() {
                            copy4(&trunk, 0, &p_page1, 32);
                        } else {
                            let prev = p_prev_trunk.clone().unwrap();
                            let prev_db = prev.borrow().p_db_page.clone();
                            rc = pager_write(&prev_db);
                            if rc != SQLITE_OK {
                                break 'end_allocate_page;
                            }
                            copy4(&trunk, 0, &prev, 0);
                        }
                    } else {
                        // O chamador precisa desta página tronco mas ela contém
                        // ponteiros para folhas da lista livre: a primeira folha
                        // vira página tronco neste caso.
                        let i_new_trunk: u32 = get4byte(&trunk.borrow().a_data, 8);
                        if i_new_trunk > mx_page {
                            rc = sqlite_corrupt_pgno(i_trunk);
                            break 'end_allocate_page;
                        }
                        let mut p_new_trunk: Option<MemPageRef> = None;
                        rc = btree_get_unused_page(p_bt, i_new_trunk, &mut p_new_trunk, 0);
                        if rc != SQLITE_OK {
                            break 'end_allocate_page;
                        }
                        let new_trunk = p_new_trunk.clone().unwrap();
                        let new_db = new_trunk.borrow().p_db_page.clone();
                        rc = pager_write(&new_db);
                        if rc != SQLITE_OK {
                            release_page(p_new_trunk);
                            break 'end_allocate_page;
                        }
                        copy4(&trunk, 0, &new_trunk, 0);
                        put4byte(&mut new_trunk.borrow_mut().a_data, 4, k - 1);
                        let len = ((k - 1) * 4) as usize;
                        let src: Vec<u8> = trunk.borrow().a_data[12..12 + len].to_vec();
                        new_trunk.borrow_mut().a_data[8..8 + len].copy_from_slice(&src);
                        release_page(p_new_trunk);
                        if p_prev_trunk.is_none() {
                            debug_assert!(pager_iswriteable(&page1_db));
                            put4byte(&mut p_page1.borrow_mut().a_data, 32, i_new_trunk);
                        } else {
                            let prev = p_prev_trunk.clone().unwrap();
                            let prev_db = prev.borrow().p_db_page.clone();
                            rc = pager_write(&prev_db);
                            if rc != 0 {
                                break 'end_allocate_page;
                            }
                            put4byte(&mut prev.borrow_mut().a_data, 0, i_new_trunk);
                        }
                    }
                    p_trunk = None;
                } else if k > 0 {
                    // Extrai uma folha do tronco.
                    let mut closest: u32;
                    let mut i_page: u32;
                    if nearby > 0 {
                        closest = 0;
                        if e_mode == BTALLOC_LE {
                            for i in 0..k {
                                i_page = get4byte(&trunk.borrow().a_data, 8 + i as usize * 4);
                                if i_page <= nearby {
                                    closest = i;
                                    break;
                                }
                            }
                        } else {
                            let mut dist: i32 = abs_int32(
                                get4byte(&trunk.borrow().a_data, 8).wrapping_sub(nearby) as i32,
                            );
                            for i in 1..k {
                                let d2: i32 = abs_int32(
                                    get4byte(&trunk.borrow().a_data, 8 + i as usize * 4).wrapping_sub(nearby)
                                        as i32,
                                );
                                if d2 < dist {
                                    closest = i;
                                    dist = d2;
                                }
                            }
                        }
                    } else {
                        closest = 0;
                    }

                    i_page = get4byte(&trunk.borrow().a_data, 8 + closest as usize * 4);
                    if i_page > mx_page || i_page < 2 {
                        rc = sqlite_corrupt_pgno(i_trunk);
                        break 'end_allocate_page;
                    }
                    if !search_list || (i_page == nearby || (i_page < nearby && e_mode == BTALLOC_LE)) {
                        *p_pgno = i_page;
                        rc = pager_write(&trunk_db);
                        if rc != 0 {
                            break 'end_allocate_page;
                        }
                        if closest < k - 1 {
                            let last = 4 + k as usize * 4;
                            let dst = 8 + closest as usize * 4;
                            let bytes: [u8; 4] = trunk.borrow().a_data[last..last + 4].try_into().unwrap();
                            trunk.borrow_mut().a_data[dst..dst + 4].copy_from_slice(&bytes);
                        }
                        put4byte(&mut trunk.borrow_mut().a_data, 4, k - 1);
                        let no_content = if btree_get_has_content(p_bt, *p_pgno) == 0 {
                            PAGER_GET_NOCONTENT
                        } else {
                            0
                        };
                        rc = btree_get_unused_page(p_bt, *p_pgno, pp_page, no_content);
                        if rc == SQLITE_OK {
                            let new_db = pp_page.as_ref().unwrap().borrow().p_db_page.clone();
                            rc = pager_write(&new_db);
                            if rc != SQLITE_OK {
                                release_page(pp_page.take());
                            }
                        }
                        search_list = false;
                    }
                }
                release_page(p_prev_trunk.take());
                if !search_list {
                    break;
                }
            }
        } else {
            // Não há páginas na lista livre: acrescenta uma nova página à
            // imagem do banco de dados.
            //
            // Normalmente as páginas novas alocadas aqui podem ser pedidas ao
            // pager com a bandeira 'no-content', que evita ler o conteúdo do
            // disco. Mas se a transação atual já executou passos de
            // incremental-vacuum, a página pode conter conteúdo necessário num
            // rollback. Nesse caso a bandeira não é ligada, e o pager carrega e
            // grava no journal o conteúdo atual antes de sobrescrevê-lo. O pager
            // nunca tenta carregar nem gravar no journal páginas que realmente
            // estão além do fim do arquivo em disco.
            let b_no_content = if p_bt.borrow().b_do_truncate == 0 { PAGER_GET_NOCONTENT } else { 0 };

            let page1_db = p_bt.borrow().p_page1.as_ref().unwrap().borrow().p_db_page.clone();
            rc = pager_write(&page1_db);
            if rc != 0 {
                return rc;
            }
            {
                let mut bt = p_bt.borrow_mut();
                bt.n_page += 1;
                if bt.n_page == pending_byte_page(&bt) {
                    bt.n_page += 1;
                }
            }

            if p_bt.borrow().auto_vacuum != 0 && ptrmap_ispage(&p_bt.borrow(), p_bt.borrow().n_page) {
                // Se *p_pgno é uma página do mapa de ponteiros, aloca duas
                // páginas novas no fim do arquivo: a primeira vira o novo mapa
                // de ponteiros e a segunda é usada pelo chamador.
                let mut p_pg: Option<MemPageRef> = None;
                debug_assert!(p_bt.borrow().n_page != pending_byte_page(&p_bt.borrow()));
                let n_page = p_bt.borrow().n_page;
                rc = btree_get_unused_page(p_bt, n_page, &mut p_pg, b_no_content);
                if rc == SQLITE_OK {
                    let pg_db = p_pg.as_ref().unwrap().borrow().p_db_page.clone();
                    rc = pager_write(&pg_db);
                    release_page(p_pg);
                }
                if rc != 0 {
                    return rc;
                }
                let mut bt = p_bt.borrow_mut();
                bt.n_page += 1;
                if bt.n_page == pending_byte_page(&bt) {
                    bt.n_page += 1;
                }
            }
            let n_page = p_bt.borrow().n_page;
            put4byte(&mut p_bt.borrow().p_page1.as_ref().unwrap().borrow_mut().a_data, 28, n_page);
            *p_pgno = n_page;

            debug_assert!(*p_pgno != pending_byte_page(&p_bt.borrow()));
            rc = btree_get_unused_page(p_bt, *p_pgno, pp_page, b_no_content);
            if rc != 0 {
                return rc;
            }
            let new_db = pp_page.as_ref().unwrap().borrow().p_db_page.clone();
            rc = pager_write(&new_db);
            if rc != SQLITE_OK {
                release_page(pp_page.take());
            }
        }

        debug_assert!(corrupt_db() || *p_pgno != pending_byte_page(&p_bt.borrow()));
    }
    // end_allocate_page:
    release_page(p_trunk);
    release_page(p_prev_trunk);
    debug_assert!(
        rc != SQLITE_OK
            || pager_page_refcount(&pp_page.as_ref().unwrap().borrow().p_db_page) <= 1
    );
    debug_assert!(rc != SQLITE_OK || pp_page.as_ref().unwrap().borrow().is_init == 0);
    rc
}


// ---- part_016.rs ----

// Contrato assumido com as outras partes (nomes pela regra determinística):
//   btree_get_page(&BtSharedRef, u32, &mut Option<MemPageRef>, i32) -> i32
//   btree_page_lookup(&BtSharedRef, u32) -> Option<MemPageRef>
//   btree_pagecount(&BtSharedRef) -> u32
//   get_overflow_page(&BtSharedRef, u32, Option<&mut Option<MemPageRef>>, &mut u32) -> i32
//   allocate_btree_page(&BtSharedRef, &mut Option<MemPageRef>, &mut u32, u32, u8) -> i32
//   btree_set_has_content(&mut BtShared, u32) -> i32
//   ptrmap_put(&BtShared, u32, u8, u32, &mut i32)
//   pager_ref(&mut PgHdr), pager_write(&PgHdrRef) -> i32, pager_dont_write(&PgHdrRef),
//   pager_page_refcount(&PgHdrRef) -> i32, pager_unref(Option<&PgHdrRef>)
// `get4byte`/`put4byte` vêm de util.c (não são redefinidos aqui).

/// Adiciona a página `i_page` à lista de páginas livres do arquivo de banco de dados.
/// Assume-se que a página ainda não faz parte da lista livre.
///
/// O segundo argumento é opcional. Se o chamador por acaso tiver à mão o objeto MemPage
/// correspondente à página `i_page`, pode passá-lo; caso contrário, passa `None`.
///
/// Se um MemPage for passado como segundo argumento, a contagem de referências dele não é
/// alterada por esta função.
fn free_page2(p_bt: &BtSharedRef, p_mem_page: Option<&MemPageRef>, i_page: u32) -> i32 {
    let mut p_trunk: Option<MemPageRef> = None; // Página tronco da lista livre
    let mut i_trunk: u32 = 0; // Número da página tronco da lista livre
    let p_page1: MemPageRef = p_bt.borrow().p_page_1.clone().unwrap(); // Referência local à página 1
    let mut p_page: Option<MemPageRef>; // Página sendo liberada, pode ser None
    let mut rc: i32; // Código de retorno
    let n_free: u32; // Número inicial de páginas na lista livre

    let db_page_of = |p: &MemPageRef| -> PgHdrRef { p.borrow().p_db_page.clone().unwrap() };

    if i_page < 2 || i_page > p_bt.borrow().n_page {
        return sqlite_corrupt_bkpt(line!() as i32);
    }
    if let Some(mp) = p_mem_page {
        p_page = Some(mp.clone());
        let db = db_page_of(mp);
        pager_ref(&mut db.borrow_mut());
    } else {
        p_page = btree_page_lookup(p_bt, i_page);
    }

    'freepage_out: {
        // Incrementa a contagem de páginas livres na página 1
        rc = pager_write(&db_page_of(&p_page1));
        if rc != 0 {
            break 'freepage_out;
        }
        n_free = get4byte(&p_page1.borrow().a_data[36..]);
        put4byte(&mut p_page1.borrow_mut().a_data[36..], n_free.wrapping_add(1));

        if (p_bt.borrow().bts_flags & BTS_SECURE_DELETE) != 0 {
            // Se a opção secure_delete está ligada, sempre sobrescreve por completo a
            // informação apagada com zeros.
            if p_page.is_none() {
                rc = btree_get_page(p_bt, i_page, &mut p_page, 0);
                if rc != 0 {
                    break 'freepage_out;
                }
            }
            rc = pager_write(&db_page_of(p_page.as_ref().unwrap()));
            if rc != 0 {
                break 'freepage_out;
            }
            let page_size = p_bt.borrow().page_size as usize;
            p_page.as_ref().unwrap().borrow_mut().a_data[..page_size].fill(0);
        }

        // Se o banco suporta auto-vacuum, escreve uma entrada no mapa de ponteiros para
        // indicar que a página está livre.
        if is_autovacuum(&p_bt.borrow()) {
            ptrmap_put(&p_bt.borrow(), i_page, PTRMAP_FREEPAGE, 0, &mut rc);
            if rc != 0 {
                break 'freepage_out;
            }
        }

        // Agora manipula a estrutura real da lista livre do arquivo. Há duas possibilidades.
        // Se a lista livre está vazia, ou se a primeira página tronco da lista está cheia,
        // esta página vira uma nova página tronco. Caso contrário, vira uma folha da primeira
        // página tronco da lista atual. Este bloco testa se é possível acrescentar a página
        // como uma nova folha da lista livre.
        if n_free != 0 {
            let n_leaf: u32; // Número inicial de células folha na página tronco

            i_trunk = get4byte(&p_page1.borrow().a_data[32..]);
            if i_trunk > btree_pagecount(p_bt) {
                rc = sqlite_corrupt_bkpt(line!() as i32);
                break 'freepage_out;
            }
            rc = btree_get_page(p_bt, i_trunk, &mut p_trunk, 0);
            if rc != SQLITE_OK {
                break 'freepage_out;
            }

            let trunk = p_trunk.as_ref().unwrap().clone();
            n_leaf = get4byte(&trunk.borrow().a_data[4..]);
            let usable_size = p_bt.borrow().usable_size;
            if n_leaf > usable_size / 4 - 2 {
                rc = sqlite_corrupt_bkpt(line!() as i32);
                break 'freepage_out;
            }
            if n_leaf < usable_size / 4 - 8 {
                // Neste caso há espaço na página tronco para inserir a página liberada
                // como uma nova folha.
                //
                // Note que a página tronco só está realmente cheia quando contém
                // usableSize/4 - 2 entradas, não usableSize/4 - 8 como está codificado.
                // Mas, por causa de um erro de codificação em versões do SQLite anteriores
                // à 3.6.0, bancos com páginas tronco da lista livre com mais de
                // usableSize/4 - 8 entradas são relatados como corrompidos. Para manter a
                // compatibilidade com versões antigas, continuamos restringindo o número de
                // entradas a usableSize/4 - 8 por enquanto. Em algum momento do futuro (quando
                // todos tiverem atualizado para a 3.6.0 ou posterior) deveríamos considerar
                // corrigir o condicional acima para "usableSize/4-2".
                //
                // EVIDENCE-OF: R-19920-11576 Porém versões novas do SQLite ainda evitam usar
                // as últimas seis entradas do vetor da página tronco da lista livre, para que
                // arquivos criados por versões novas possam ser lidos por versões antigas.
                rc = pager_write(&db_page_of(&trunk));
                if rc == SQLITE_OK {
                    {
                        let mut t = trunk.borrow_mut();
                        put4byte(&mut t.a_data[4..], n_leaf + 1);
                        put4byte(&mut t.a_data[(8 + n_leaf * 4) as usize..], i_page);
                    }
                    if p_page.is_some() && (p_bt.borrow().bts_flags & BTS_SECURE_DELETE) == 0 {
                        pager_dont_write(&db_page_of(p_page.as_ref().unwrap()));
                    }
                    rc = btree_set_has_content(&mut p_bt.borrow_mut(), i_page);
                }
                break 'freepage_out;
            }
        }

        // Se o fluxo chegou aqui, não foi possível acrescentar a página liberada como folha
        // da primeira página tronco da lista livre, talvez porque a lista esteja vazia, talvez
        // porque a primeira tronco esteja cheia. De qualquer forma, a página liberada vira a
        // nova primeira página tronco da lista livre.
        if p_page.is_none() {
            rc = btree_get_page(p_bt, i_page, &mut p_page, 0);
            if rc != SQLITE_OK {
                break 'freepage_out;
            }
        }
        rc = pager_write(&db_page_of(p_page.as_ref().unwrap()));
        if rc != SQLITE_OK {
            break 'freepage_out;
        }
        {
            let mut pg = p_page.as_ref().unwrap().borrow_mut();
            put4byte(&mut pg.a_data[0..], i_trunk);
            put4byte(&mut pg.a_data[4..], 0);
        }
        put4byte(&mut p_page1.borrow_mut().a_data[32..], i_page);
    }

    // freepage_out:
    if let Some(pg) = p_page.as_ref() {
        pg.borrow_mut().is_init = 0;
    }
    release_page(p_page.as_ref());
    release_page(p_trunk.as_ref());
    rc
}

/// Libera a página `p_page` (usa `free_page2` com o `p_bt` e o `pgno` da própria página).
fn free_page(p_page: &MemPageRef, p_rc: &mut i32) {
    if *p_rc == SQLITE_OK {
        let p_bt = p_page.borrow().p_bt.as_ref().unwrap().upgrade().unwrap();
        let pgno = p_page.borrow().pgno;
        *p_rc = free_page2(&p_bt, Some(p_page), pgno);
    }
}

/// Libera as páginas de overflow associadas à célula dada.
///
/// `p_cell` é a célula a partir do primeiro byte dela, indo até o fim do buffer de dados da
/// página dona (a distância ao fim de `a_data` dá o deslocamento da célula na página).
#[inline(never)]
fn clear_cell_overflow(p_page: &MemPage, p_cell: &[u8], p_info: &CellInfo) -> i32 {
    // Deslocamento da célula dentro de a_data (p_cell é o final do buffer da página).
    let cell_off = p_page.a_data.len().saturating_sub(p_cell.len());
    if cell_off + p_info.n_size as usize > p_page.a_data_end {
        // A célula passa do fim da página
        return sqlite_corrupt_bkpt(line!() as i32);
    }
    let mut ovfl_pgno: u32 = get4byte(&p_cell[p_info.n_size as usize - 4..]);
    let p_bt: BtSharedRef = p_page.p_bt.as_ref().unwrap().upgrade().unwrap();
    let ovfl_page_size: u32 = p_bt.borrow().usable_size - 4;
    let mut n_ovfl: i32 = p_info
        .n_payload
        .wrapping_sub(p_info.n_local as u32)
        .wrapping_add(ovfl_page_size)
        .wrapping_sub(1)
        .wrapping_div(ovfl_page_size) as i32;
    let mut rc: i32;
    while n_ovfl != 0 {
        n_ovfl = n_ovfl.wrapping_sub(1);
        let mut i_next: u32 = 0;
        let mut p_ovfl: Option<MemPageRef> = None;
        if ovfl_pgno < 2 || ovfl_pgno > btree_pagecount(&p_bt) {
            // 0 não é número de página legal e a página 1 não pode ser de overflow. Logo,
            // se ovflPgno<2 ou passa do fim do arquivo, o banco está corrompido.
            return sqlite_corrupt_bkpt(line!() as i32);
        }
        if n_ovfl != 0 {
            rc = get_overflow_page(&p_bt, ovfl_pgno, Some(&mut p_ovfl), &mut i_next);
            if rc != 0 {
                return rc;
            }
        }

        if p_ovfl.is_none() {
            p_ovfl = btree_page_lookup(&p_bt, ovfl_pgno);
        }
        let ovfl_db: Option<PgHdrRef> = p_ovfl.as_ref().map(|o| o.borrow().p_db_page.clone().unwrap());
        if ovfl_db.is_some() && pager_page_refcount(ovfl_db.as_ref().unwrap()) != 1 {
            // Nenhum cursor deveria ter referência pendente para uma página de overflow de
            // uma célula que está sendo apagada ou atualizada. Se existe mais de uma
            // referência, a página não é de fato de overflow e o banco está corrompido. É
            // útil detectar isso antes de chamar free_page2(), que pode zerar o conteúdo da
            // página no modo secure-delete. Se essa página de 'overflow' for uma página que o
            // chamador está percorrendo ou usando de outro modo, isso seria problemático.
            rc = sqlite_corrupt_bkpt(line!() as i32);
        } else {
            rc = free_page2(&p_bt, p_ovfl.as_ref(), ovfl_pgno);
        }

        if ovfl_db.is_some() {
            pager_unref(ovfl_db.as_ref());
        }
        if rc != 0 {
            return rc;
        }
        ovfl_pgno = i_next;
    }
    SQLITE_OK
}

/// Chama `x_parse_cell` para calcular o tamanho de uma célula. Se a célula tem overflow,
/// chama `clear_cell_overflow` para limpá-lo. Devolve o código de resultado (macro
/// BTREE_CLEAR_CELL do C, que o C expande em linha por desempenho).
#[inline]
fn btree_clear_cell(p_page: &MemPage, p_cell: &[u8], s_info: &mut CellInfo) -> i32 {
    (p_page.x_parse_cell)(p_page, p_cell, s_info);
    if s_info.n_local as u32 != s_info.n_payload {
        clear_cell_overflow(p_page, p_cell, s_info)
    } else {
        SQLITE_OK
    }
}

/// Cria a sequência de bytes que representa uma célula na página `p_page` e a escreve em
/// `p_cell[]`. Páginas de overflow são alocadas e preenchidas conforme necessário. O
/// chamador é responsável por garantir espaço suficiente em `p_cell[]`.
///
/// Note que `p_cell` não precisa apontar para a área `p_page.a_data`: pode ser um
/// armazenamento temporário. A célula é montada nessa área e depois copiada para
/// `p_page.a_data`.
fn fill_in_cell(p_page: &MemPage, p_cell: &mut [u8], p_x: &BtreePayload, pn_size: &mut i32) -> i32 {
    let n_payload: i32;
    let p_src: &[u8];
    let mut n_src: i32;
    let mut n: i32;
    let mut rc: i32;
    let mn: i32;
    let mut space_left: i32;
    let mut p_to_release: Option<MemPageRef>;
    let mut p_prior: usize;
    let mut p_payload: usize;
    let p_bt: BtSharedRef;
    let mut pgno_ovfl: u32;
    let mut n_header: i32;

    // Preenche o cabeçalho.
    n_header = p_page.child_ptr_size as i32;
    if p_page.int_key != 0 {
        n_payload = p_x.n_data + p_x.n_zero;
        p_src = p_x.p_data.as_deref().unwrap_or(&[]);
        n_src = p_x.n_data;
        // fillInCell() só é chamada para folhas
        n_header += put_varint32(&mut p_cell[n_header as usize..], n_payload as u32) as i32;
        n_header += put_varint(&mut p_cell[n_header as usize..], p_x.n_key as u64) as i32;
    } else {
        n_payload = p_x.n_key as i32;
        n_src = n_payload;
        p_src = p_x.p_key.as_deref().unwrap_or(&[]);
        n_header += put_varint32(&mut p_cell[n_header as usize..], n_payload as u32) as i32;
    }
    let mut src_off: usize = 0; // posição atual em p_src

    // Preenche a carga útil
    p_payload = n_header as usize;
    if n_payload <= p_page.max_local as i32 {
        // Caso comum: tudo cabe na página b-tree e nenhuma página de overflow é necessária.
        n = n_header + n_payload;
        if n < 4 {
            n = 4;
            p_cell[p_payload + n_payload as usize] = 0;
        }
        *pn_size = n;
        let ns = n_src as usize;
        p_cell[p_payload..p_payload + ns].copy_from_slice(&p_src[..ns]);
        p_cell[p_payload + ns..p_payload + n_payload as usize].fill(0);
        return SQLITE_OK;
    }

    // Se chegamos aqui, parte do conteúdo precisa ir para páginas de overflow.
    p_bt = p_page.p_bt.as_ref().unwrap().upgrade().unwrap();
    let usable_size = p_bt.borrow().usable_size as i32;
    mn = p_page.min_local as i32;
    n = mn + (n_payload - mn) % (usable_size - 4);
    if n > p_page.max_local as i32 {
        n = mn;
    }
    space_left = n;
    *pn_size = n + n_header + 4;
    p_prior = (n_header + n) as usize;
    p_to_release = None;
    pgno_ovfl = 0;

    // Neste ponto as variáveis estão assim:
    //
    //   n_payload    tamanho total da carga útil em bytes
    //   p_payload    onde começar a escrever a carga útil
    //   space_left   espaço disponível em p_payload; se n_payload>space_left, o conteúdo
    //                precisa transbordar para páginas de overflow
    //   *pn_size     tamanho da célula local (sem contar as páginas de overflow)
    //   p_prior      onde escrever o pgno da primeira página de overflow
    //
    // Enquanto p_to_release é None, p_payload e p_prior indexam p_cell; depois que a
    // primeira página de overflow existe, indexam o a_data de p_to_release.
    let mut n_payload = n_payload;

    // Escreve a carga útil na célula local e o excedente nas páginas de overflow
    loop {
        n = n_payload;
        if n > space_left {
            n = space_left;
        }

        {
            let mut guard;
            let dest: &mut [u8] = match &p_to_release {
                Some(o) => {
                    guard = o.borrow_mut();
                    &mut guard.a_data[..]
                }
                None => &mut p_cell[..],
            };
            if n_src >= n {
                let nn = n as usize;
                dest[p_payload..p_payload + nn].copy_from_slice(&p_src[src_off..src_off + nn]);
            } else if n_src > 0 {
                n = n_src;
                let nn = n as usize;
                dest[p_payload..p_payload + nn].copy_from_slice(&p_src[src_off..src_off + nn]);
            } else {
                let nn = n as usize;
                dest[p_payload..p_payload + nn].fill(0);
            }
        }
        n_payload -= n;
        if n_payload <= 0 {
            break;
        }
        p_payload += n as usize;
        src_off += n as usize;
        n_src -= n;
        space_left -= n;
        if space_left == 0 {
            let mut p_ovfl: Option<MemPageRef> = None;
            let pgno_ptrmap: u32 = pgno_ovfl; // Página da entrada do mapa de ponteiros de overflow
            let auto_vacuum = p_bt.borrow().auto_vacuum != 0;
            if auto_vacuum {
                let b = p_bt.borrow();
                loop {
                    pgno_ovfl += 1;
                    if !(ptrmap_ispage(&b, pgno_ovfl) || pgno_ovfl == pending_byte_page(&b)) {
                        break;
                    }
                }
            }
            let i_near = pgno_ovfl;
            rc = allocate_btree_page(&p_bt, &mut p_ovfl, &mut pgno_ovfl, i_near, 0);
            // Se o banco suporta auto-vacuum e a segunda ou posterior página de overflow
            // está sendo alocada, acrescenta agora uma entrada no mapa de ponteiros para
            // essa página.
            //
            // Se é a primeira página de overflow, escreve uma entrada parcial no mapa de
            // ponteiros. Se nada for escrito nesse slot, o processamento otimista da cadeia
            // de overflow em clearCell() pode interpretar mal os valores não inicializados e
            // apagar as páginas erradas do banco.
            if auto_vacuum && rc == SQLITE_OK {
                let e_type = if pgno_ptrmap != 0 { PTRMAP_OVERFLOW2 } else { PTRMAP_OVERFLOW1 };
                ptrmap_put(&p_bt.borrow(), pgno_ovfl, e_type, pgno_ptrmap, &mut rc);
                if rc != 0 {
                    release_page(p_ovfl.as_ref());
                }
            }
            if rc != 0 {
                release_page(p_to_release.as_ref());
                return rc;
            }

            {
                let mut guard;
                let dest: &mut [u8] = match &p_to_release {
                    Some(o) => {
                        guard = o.borrow_mut();
                        &mut guard.a_data[..]
                    }
                    None => &mut p_cell[..],
                };
                put4byte(&mut dest[p_prior..], pgno_ovfl);
            }
            release_page(p_to_release.as_ref());
            p_to_release = p_ovfl;
            p_prior = 0;
            put4byte(&mut p_to_release.as_ref().unwrap().borrow_mut().a_data[0..], 0);
            p_payload = 4;
            space_left = usable_size - 4;
        }
    }
    release_page(p_to_release.as_ref());
    SQLITE_OK
}


// ---- part_017.rs ----

// Contrato assumido com as outras partes:
//   free_space(&mut MemPage, u16, u16) -> i32
//   allocate_space(&mut MemPage, usize, &mut usize) -> i32
//   ptrmap_put_ovfl_ptr(&MemPage, &MemPage, &[u8], &mut i32)
//   pager_write(&PgHdrRef) -> i32
// Sem ponteiros: `u8 *pCell` vira `&[u8]` / `&mut [u8]` (a célula a partir do primeiro byte), e
// `pPage->apOvfl[j] = pCell` vira uma cópia dona (`Box<[u8]>`) do conteúdo da célula.

/// Remove a i-ésima célula de `p_page`. Esta rotina afeta somente a página. O conteúdo da
/// célula não é liberado nem desalocado: assume-se que ele foi copiado para outro lugar. A rotina
/// apenas remove da página a referência à célula.
///
/// `sz` precisa ser o número de bytes da célula.
fn drop_cell(p_page: &mut MemPage, idx: i32, sz: i32, p_rc: &mut i32) {
    if *p_rc != 0 {
        return;
    }
    debug_assert!(idx >= 0);
    debug_assert!(idx < p_page.n_cell as i32);
    debug_assert!(p_page.n_free >= 0);
    let usable_size: u32 = p_page.p_bt.as_ref().unwrap().upgrade().unwrap().borrow().usable_size;
    let ptr = p_page.a_cell_idx + 2 * idx as usize;
    let pc: u32 = get2byte(&p_page.a_data[ptr..]) as u32; // Deslocamento do conteúdo da célula apagada
    let hdr = p_page.hdr_offset as usize; // Início do cabeçalho: 0 na maioria das páginas, 100 na página 1
    if pc.wrapping_add(sz as u32) > usable_size {
        *p_rc = sqlite_corrupt_bkpt(line!() as i32);
        return;
    }
    let rc = free_space(p_page, pc as u16, sz as u16);
    if rc != 0 {
        *p_rc = rc;
        return;
    }
    p_page.n_cell -= 1;
    if p_page.n_cell == 0 {
        p_page.a_data[hdr + 1..hdr + 5].fill(0);
        p_page.a_data[hdr + 7] = 0;
        put2byte(&mut p_page.a_data[hdr + 5..], usable_size as u16);
        p_page.n_free = (usable_size as i32)
            - p_page.hdr_offset as i32
            - p_page.child_ptr_size as i32
            - 8;
    } else {
        let n = 2 * (p_page.n_cell as usize - idx as usize);
        p_page.a_data.copy_within(ptr + 2..ptr + 2 + n, ptr);
        let n_cell = p_page.n_cell;
        put2byte(&mut p_page.a_data[hdr + 3..], n_cell);
        p_page.n_free += 2;
    }
}

/// Insere uma nova célula em `p_page` no índice de célula `i`. `p_cell` é o conteúdo da célula.
///
/// Se o conteúdo cabe na página, ele é colocado lá. Se não cabe, faz uma cópia do conteúdo em
/// `p_temp` quando `p_temp` não é `None`. Independentemente de `p_temp`, aloca uma nova entrada
/// em `p_page.ap_ovfl[]` que guarda o conteúdo da célula (em `p_temp` ou o original) e registra
/// também o índice. Alocar uma nova entrada implica incrementar `p_page.n_overflow`.
///
/// A rotina `insert_cell_fast()` abaixo funciona exatamente como `insert_cell()`, exceto que
/// não tem os parâmetros `p_temp` e `i_child`, que se assumem zero. Fora isso, as duas rotinas
/// são iguais.
///
/// Correções ou melhorias nesta rotina devem ser refletidas em `insert_cell_fast()`!
fn insert_cell(
    p_page: &mut MemPage,
    i: i32,
    p_cell: &mut [u8],
    sz: i32,
    p_temp: Option<&mut [u8]>,
    i_child: u32,
) -> i32 {
    let mut idx: usize = 0; // Onde escrever o conteúdo da nova célula em data[]
    let j: usize; // Contador de laço

    debug_assert!(i >= 0 && i <= p_page.n_cell as i32 + p_page.n_overflow as i32);
    debug_assert!(p_page.n_overflow as usize <= p_page.ap_ovfl.len());
    debug_assert!(p_page.n_free >= 0);
    debug_assert!(i_child > 0);
    if p_page.n_overflow != 0 || sz + 2 > p_page.n_free {
        let szu = sz as usize;
        let boxed: Box<[u8]> = if let Some(t) = p_temp {
            t[..szu].copy_from_slice(&p_cell[..szu]);
            put4byte(t, i_child);
            t[..szu].to_vec().into_boxed_slice()
        } else {
            put4byte(p_cell, i_child);
            p_cell[..szu].to_vec().into_boxed_slice()
        };
        j = p_page.n_overflow as usize;
        p_page.n_overflow += 1;
        // Comparação com ArraySize-1 porque guardamos um slot extra como contingência. Em
        // outras palavras, nunca se precisa de mais de 3 slots de overflow, mas 4 são alocados
        // por segurança.
        debug_assert!(j < p_page.ap_ovfl.len() - 1);
        p_page.ap_ovfl[j] = Some(boxed);
        p_page.ai_ovfl[j] = i as u16;

        // Quando ocorrem vários overflows, eles são sempre sequenciais e em ordem. Essas
        // invariantes surgem porque vários overflows só ocorrem ao inserir células divisoras
        // na página pai durante o balanceamento, e as divisoras são adjacentes e ordenadas.
        debug_assert!(j == 0 || p_page.ai_ovfl[j - 1] < i as u16); // Overflows em ordem
        debug_assert!(j == 0 || i == p_page.ai_ovfl[j - 1] as i32 + 1); // Overflows sequenciais
    } else {
        let rc = pager_write(p_page.p_db_page.as_ref().unwrap());
        if rc != SQLITE_OK {
            return rc;
        }
        let hdr = p_page.hdr_offset as usize;
        debug_assert!(p_page.cell_offset as usize == p_page.a_cell_idx);
        let rc = allocate_space(p_page, sz as usize, &mut idx);
        if rc != 0 {
            return rc;
        }
        // allocate_space() garante as propriedades abaixo quando retorna com sucesso
        debug_assert!(idx as i32 + sz <= p_page.p_bt.as_ref().unwrap().upgrade().unwrap().borrow().usable_size as i32);
        p_page.n_free -= (2 + sz) as u16 as i32;
        // Num banco corrompido em que uma entrada da seção de índice de células tem valor 3
        // ou menos, o valor de pCell pode apontar até 4 bytes antes do início do buffer aData
        // da página de origem. Evita-se o problema não lendo os 4 primeiros bytes.
        let szu = sz as usize;
        p_page.a_data[idx + 4..idx + szu].copy_from_slice(&p_cell[4..szu]);
        put4byte(&mut p_page.a_data[idx..], i_child);
        let p_ins = p_page.a_cell_idx + i as usize * 2;
        let n_move = 2 * (p_page.n_cell as usize - i as usize);
        p_page.a_data.copy_within(p_ins..p_ins + n_move, p_ins + 2);
        put2byte(&mut p_page.a_data[p_ins..], idx as u16);
        p_page.n_cell += 1;
        // incrementa a contagem de células
        p_page.a_data[hdr + 4] = p_page.a_data[hdr + 4].wrapping_add(1);
        if p_page.a_data[hdr + 4] == 0 {
            p_page.a_data[hdr + 3] = p_page.a_data[hdr + 3].wrapping_add(1);
        }
        debug_assert!(get2byte(&p_page.a_data[hdr + 3..]) == p_page.n_cell);
        if p_page.p_bt.as_ref().unwrap().upgrade().unwrap().borrow().auto_vacuum != 0 {
            let mut rc2: i32 = SQLITE_OK;
            // A célula pode conter um ponteiro para página de overflow. Se contiver, escreve
            // no mapa de ponteiros a entrada da página de overflow.
            ptrmap_put_ovfl_ptr(&*p_page, &*p_page, p_cell, &mut rc2);
            if rc2 != 0 {
                return rc2;
            }
        }
    }
    SQLITE_OK
}

/// Esta variante de `insert_cell()` assume que `p_temp` e `i_child` são ambos zero. Use esta
/// variante em `btree_insert()` por desempenho, e também para que ela seja chamada só desse
/// lugar, seja portanto expandida em linha e, assim, rode bem mais rápido.
///
/// Correções ou melhorias nesta rotina devem ser refletidas em `insert_cell()`.
fn insert_cell_fast(p_page: &mut MemPage, i: i32, p_cell: &[u8], sz: i32) -> i32 {
    let mut idx: usize = 0; // Onde escrever o conteúdo da nova célula em data[]
    let j: usize; // Contador de laço

    debug_assert!(i >= 0 && i <= p_page.n_cell as i32 + p_page.n_overflow as i32);
    debug_assert!(p_page.n_overflow as usize <= p_page.ap_ovfl.len());
    debug_assert!(p_page.n_free >= 0);
    debug_assert!(p_page.n_overflow == 0);
    if sz + 2 > p_page.n_free {
        j = p_page.n_overflow as usize;
        p_page.n_overflow += 1;
        // Comparação com ArraySize-1 porque guardamos um slot extra como contingência. Em
        // outras palavras, nunca se precisa de mais de 3 slots de overflow, mas 4 são alocados
        // por segurança.
        debug_assert!(j < p_page.ap_ovfl.len() - 1);
        p_page.ap_ovfl[j] = Some(p_cell[..sz as usize].to_vec().into_boxed_slice());
        p_page.ai_ovfl[j] = i as u16;

        // Quando ocorrem vários overflows, eles são sempre sequenciais e em ordem. Essas
        // invariantes surgem porque vários overflows só ocorrem ao inserir células divisoras
        // na página pai durante o balanceamento, e as divisoras são adjacentes e ordenadas.
        debug_assert!(j == 0 || p_page.ai_ovfl[j - 1] < i as u16); // Overflows em ordem
        debug_assert!(j == 0 || i == p_page.ai_ovfl[j - 1] as i32 + 1); // Overflows sequenciais
    } else {
        let rc = pager_write(p_page.p_db_page.as_ref().unwrap());
        if rc != SQLITE_OK {
            return rc;
        }
        let hdr = p_page.hdr_offset as usize;
        debug_assert!(p_page.cell_offset as usize == p_page.a_cell_idx);
        let rc = allocate_space(p_page, sz as usize, &mut idx);
        if rc != 0 {
            return rc;
        }
        // allocate_space() garante as propriedades abaixo quando retorna com sucesso
        debug_assert!(idx as i32 + sz <= p_page.p_bt.as_ref().unwrap().upgrade().unwrap().borrow().usable_size as i32);
        p_page.n_free -= (2 + sz) as u16 as i32;
        let szu = sz as usize;
        p_page.a_data[idx..idx + szu].copy_from_slice(&p_cell[..szu]);
        let p_ins = p_page.a_cell_idx + i as usize * 2;
        let n_move = 2 * (p_page.n_cell as usize - i as usize);
        p_page.a_data.copy_within(p_ins..p_ins + n_move, p_ins + 2);
        put2byte(&mut p_page.a_data[p_ins..], idx as u16);
        p_page.n_cell += 1;
        // incrementa a contagem de células
        p_page.a_data[hdr + 4] = p_page.a_data[hdr + 4].wrapping_add(1);
        if p_page.a_data[hdr + 4] == 0 {
            p_page.a_data[hdr + 3] = p_page.a_data[hdr + 3].wrapping_add(1);
        }
        debug_assert!(get2byte(&p_page.a_data[hdr + 3..]) == p_page.n_cell);
        if p_page.p_bt.as_ref().unwrap().upgrade().unwrap().borrow().auto_vacuum != 0 {
            let mut rc2: i32 = SQLITE_OK;
            // A célula pode conter um ponteiro para página de overflow. Se contiver, escreve
            // no mapa de ponteiros a entrada da página de overflow.
            ptrmap_put_ovfl_ptr(&*p_page, &*p_page, p_cell, &mut rc2);
            if rc2 != 0 {
                return rc2;
            }
        }
    }
    SQLITE_OK
}

/// Os parâmetros a seguir determinam quantas páginas adjacentes entram numa operação de
/// balanceamento. NN é o número de vizinhas de cada lado da página que participam do
/// balanceamento. NB é o número total de páginas que participam, incluindo a página alvo e as NN
/// vizinhas de cada lado.
///
/// O valor mínimo de NN é 1 (claro). Aumentar NN acima de 1 (para 2 ou 3) dá um ganho modesto
/// em SELECT e DELETE em troca de uma perda maior em INSERT e UPDATE. O valor de NN parece dar os
/// melhores resultados no geral.
///
/// (Mais tarde:) A descrição acima faz parecer que esses valores são ajustáveis, como se desse
/// para mudá-los e recompilar e tudo funcionar. Mas isso é improvável. NB vale 3 desde o início
/// do SQLite e nunca se testou outro valor.
pub const NN: usize = 1; // Número de vizinhas de cada lado de p_page
pub const NB: usize = 3; // (NN*2+1): total de páginas envolvidas no balanceamento

/// Referência a uma célula de b-tree: onde, em que buffer e em que deslocamento ela mora. É o
/// `u8 *` do C sem ponteiros: `pgno` é a página dona do buffer (0 para memória fora de qualquer
/// página), `off` o deslocamento da célula em `buf`, e `buf` o buffer de origem inteiro.
#[derive(Clone)]
pub struct CellPtr {
    pub pgno: u32,
    pub off: usize,
    pub buf: std::rc::Rc<Vec<u8>>,
}

/// Um objeto CellArray contém um cache de ponteiros e tamanhos para uma sequência consecutiva
/// de células que pode estar espalhada por várias páginas.
///
/// As células deste vetor são a célula divisora (ou células divisoras) da página `p_parent` mais
/// até três páginas filhas. Há um total de `n_cell` células.
///
/// `p_ref` é uma das páginas que contribuem com células. É usada para acessar informações como
/// `MemPage.int_key` e `MemPage.p_bt.page_size`, que devem ser comuns a todas as páginas que
/// contribuem células para este vetor.
///
/// `ap_cell[]` e `sz_cell[]` guardam, respectivamente, a referência ao início de cada célula e o
/// tamanho de cada uma. Algumas das referências de `ap_cell[]` podem ser de células de overflow.
/// Em outras palavras, algumas referências podem não apontar para a área de conteúdo das páginas.
///
/// Um `sz_cell[]` igual a zero significa que o tamanho dessa célula ainda não foi calculado.
///
/// As células vêm de até quatro páginas diferentes:
///
/// ```text
///             -----------
///             | Parent  |
///             -----------
///            /     |     \
///           /      |      \
///  ---------   ---------   ---------
///  |Child-1|   |Child-2|   |Child-3|
///  ---------   ---------   ---------
/// ```
///
/// A ordem das células no vetor para uma b-tree de índice é:
///
///   1. Todas as células de Child-1, em ordem
///   2. A primeira célula divisora de Parent
///   3. Todas as células de Child-2, em ordem
///   4. A segunda célula divisora de Parent
///   5. Todas as células de Child-3, em ordem
///
/// Para uma b-tree de tabela (com rowids), os itens 2 e 4 são vazios porque o conteúdo existe
/// só nas folhas e não há células divisoras.
///
/// Para uma b-tree de índice, o vetor `ap_end[]` guarda o fim da página para Child-1, Parent,
/// Child-2, Parent (de novo) e Child-3, respectivamente. O vetor `ix_nx[]` guarda o número de
/// células contidas em cada um desses 5 estágios, e em todos os estágios à esquerda. Logo:
///
///    ix_nx[0] = número de células em Child-1.
///    ix_nx[1] = número de células em Child-1 mais 1 da primeira divisora.
///    ix_nx[2] = número de células em Child-1 e Child-2 mais 1 da primeira divisora.
///    ix_nx[3] = número de células em Child-1 e Child-2 mais as duas divisoras.
///    ix_nx[4] = número total de células.
///
/// Para uma b-tree de tabela, o conceito é parecido, mas só `ap_end[0]`..`ap_end[2]` são usados,
/// apontam só para as páginas folha, e os valores de `ix_nx` são:
///
///    ix_nx[0] = número de células em Child-1.
///    ix_nx[1] = número de células em Child-1 e Child-2.
///    ix_nx[2] = número total de células.
///
/// Às vezes, ao apagar, uma página filha pode ficar com zero células. Nesses casos, as entradas
/// de `ix_nx[]` de índice maior, e as entradas correspondentes de `ap_end[]`, deslocam-se para
/// baixo. O resultado final é que cada entrada de `ix_nx[]` deve ser maior que a anterior.
///
/// `ap_end[k]` é o fim, em índice, do buffer de origem das células da faixa k.
pub struct CellArray {
    /// Número de células em ap_cell[]
    pub n_cell: i32,
    /// Página de referência
    pub p_ref: std::rc::Rc<std::cell::RefCell<MemPage>>,
    /// Todas as células em balanceamento
    pub ap_cell: Vec<CellPtr>,
    /// Tamanho local de todas as células de ap_cell[]
    pub sz_cell: Vec<u16>,
    /// Valores de MemPage.a_data_end
    pub ap_end: [usize; NB * 2],
    /// Índice em que se passa para o próximo ap_end[]
    pub ix_nx: [i32; NB * 2],
}

/// Garante que os tamanhos das células idx, idx+1, ..., idx+N-1 foram calculados.
fn populate_cell_cache(p: &mut CellArray, idx: i32, n: i32) {
    let p_ref = p.p_ref.clone();
    let p_ref = p_ref.borrow();
    let mut idx = idx as usize;
    let mut n = n;
    debug_assert!(idx as i32 + n <= p.n_cell);
    while n > 0 {
        if p.sz_cell[idx] == 0 {
            let sz = {
                let c = &p.ap_cell[idx];
                (p_ref.x_cell_size)(&*p_ref, &c.buf[c.off..])
            };
            p.sz_cell[idx] = sz;
        }
        idx += 1;
        n -= 1;
    }
}


// ---- part_018.rs ----

// Contrato assumido com a parte 017 (CellArray, NB, populate_cell_cache), que ainda não existe
// no momento desta tradução. Sem ponteiros, o `u8 *apCell[i]` do C vira um `CellPtr` com a
// página dona (`pgno`, 0 para memória fora de qualquer página), o deslocamento dentro do buffer
// de origem (`off`) e o buffer de origem inteiro (`buf`). Os campos usados aqui:
//
//   CellArray { n_cell: i32, p_ref: Rc<RefCell<MemPage>>, ap_cell: Vec<CellPtr>,
//               sz_cell: Vec<u16>, ap_end: [usize; NB * 2], ix_nx: [i32; NB * 2] }
//   CellPtr   { pgno: u32, off: usize, buf: Rc<Vec<u8>> }
//   NB: usize (valor 3)
//
// `SQLITE_WITHIN(pCell, aData+j, pEnd)` vira `pgno == pPg.pgno && sqlite_within(off, j, fim)`:
// só um ponteiro para dentro do aData da própria página casa. `ap_end[k]` é o fim, em índice,
// do buffer de origem das células da faixa k.

/// Tamanho da N-ésima célula do vetor de células (cálculo na primeira vez).
#[inline(never)]
pub fn compute_cell_size(p: &mut CellArray, n: i32) -> u16 {
    let n = n as usize;
    debug_assert!(n < p.n_cell as usize);
    debug_assert!(p.sz_cell[n] == 0);
    let sz = {
        let p_ref = p.p_ref.borrow();
        let cell = &p.ap_cell[n];
        (p_ref.x_cell_size)(&*p_ref, &cell.buf[cell.off..])
    };
    p.sz_cell[n] = sz;
    sz
}

pub fn cached_cell_size(p: &mut CellArray, n: i32) -> u16 {
    debug_assert!(n >= 0 && n < p.n_cell);
    if p.sz_cell[n as usize] != 0 {
        return p.sz_cell[n as usize];
    }
    compute_cell_size(p, n)
}

/// O vetor `ap_cell` guarda as `n_cell` células de uma página b-tree e `sz_cell` o tamanho de
/// cada uma. Esta função substitui o conteúdo atual da página `p_pg` pelo conteúdo do vetor.
///
/// Algumas células de `ap_cell` podem estar hoje dentro de `p_pg`. Para contornar o problema,
/// esta função faz uma cópia dessas células antes de sobrescrever os dados da página.
///
/// O campo `MemPage.n_free` fica inválido: é responsabilidade do chamador acertá-lo.
pub fn rebuild_page(p_c_array: &CellArray, i_first: i32, n_cell: i32, p_pg: &mut MemPage) -> i32 {
    let hdr = p_pg.hdr_offset as usize;
    let usable_size = p_pg
        .p_bt
        .as_ref()
        .and_then(|b| b.upgrade())
        .map(|b| b.borrow().usable_size)
        .unwrap_or(0) as usize;
    let p_end = usable_size;
    let mut i = i_first as usize;
    let i_end = (i_first + n_cell) as usize;
    let mut p_cellptr = p_pg.a_cell_idx;

    debug_assert!(n_cell > 0);
    debug_assert!(i < i_end);
    let mut j = get2byte(&p_pg.a_data[hdr + 5..]) as usize;
    if j > usable_size {
        j = 0;
    }
    // Cópia do conteúdo antigo da página (o espaço temporário do pager no C): só a faixa
    // j..usable_size é lida depois.
    let mut p_tmp = vec![0u8; usable_size];
    p_tmp[j..].copy_from_slice(&p_pg.a_data[j..usable_size]);

    let mut k = 0usize;
    while k < NB * 2 && p_c_array.ix_nx[k] <= i as i32 {
        k += 1;
    }
    let mut p_src_end = p_c_array.ap_end[k];

    let mut p_data = p_end;
    loop {
        let cell = &p_c_array.ap_cell[i];
        let sz = p_c_array.sz_cell[i] as usize;
        debug_assert!(sz > 0);
        let in_page = cell.pgno == p_pg.pgno && sqlite_within(cell.off, j, p_end);
        if in_page {
            if cell.off + sz > p_end {
                return sqlite_corrupt_bkpt(line!() as i32);
            }
        } else if cell.off + sz > p_src_end && cell.off < p_src_end {
            return sqlite_corrupt_bkpt(line!() as i32);
        }

        let p_data_i = p_data as isize - sz as isize;
        put2byte(&mut p_pg.a_data[p_cellptr..], p_data_i as u16);
        p_cellptr += 2;
        if p_data_i < p_cellptr as isize {
            return sqlite_corrupt_bkpt(line!() as i32);
        }
        p_data = p_data_i as usize;
        if in_page {
            p_pg.a_data[p_data..p_data + sz].copy_from_slice(&p_tmp[cell.off..cell.off + sz]);
        } else {
            p_pg.a_data[p_data..p_data + sz].copy_from_slice(&cell.buf[cell.off..cell.off + sz]);
        }
        i += 1;
        if i >= i_end {
            break;
        }
        if p_c_array.ix_nx[k] <= i as i32 {
            k += 1;
            p_src_end = p_c_array.ap_end[k];
        }
    }

    // O campo n_free agora está incorreto. O chamador vai acertá-lo.
    p_pg.n_cell = n_cell as u16;
    p_pg.n_overflow = 0;

    put2byte(&mut p_pg.a_data[hdr + 1..], 0);
    let n_cell_now = p_pg.n_cell;
    put2byte(&mut p_pg.a_data[hdr + 3..], n_cell_now);
    put2byte(&mut p_pg.a_data[hdr + 5..], p_data as u16);
    p_pg.a_data[hdr + 7] = 0x00;
    SQLITE_OK
}

/// O objeto `p_c_array` contém as células b-tree e seus tamanhos. Esta função tenta
/// acrescentar à página `p_pg` as células guardadas no vetor. Se não consegue (porque a
/// página precisa ser desfragmentada antes de as células caberem), devolve diferente de zero.
/// Caso contrário, as células são acrescentadas e devolve zero.
///
/// `p_cellptr` é o índice da primeira entrada do vetor de ponteiros de célula (parte de
/// `p_pg`) a preencher. Depois que a célula `ap_cell[0]` é escrita no corpo da página, um
/// deslocamento de 16 bits é escrito em `p_cellptr`, e assim por diante. É responsabilidade do
/// chamador garantir que sobrescrever essa parte do vetor é seguro.
///
/// Na entrada, `*pp_data` é o início da área de conteúdo da página. Se a área de conteúdo
/// crescer, `*pp_data` é atualizado antes de retornar.
///
/// `p_begin` é o índice do byte logo depois do fim do espaço que a página exige para a área de
/// ponteiros de célula (de todas as células, não só as inseridas nesta chamada). Se a área de
/// conteúdo precisar chegar antes desse ponto, as células não cabem e devolve diferente de zero.
pub fn page_insert_array(
    p_pg: &mut MemPage,
    p_begin: usize,
    pp_data: &mut usize,
    p_cellptr: usize,
    i_first: i32,
    n_cell: i32,
    p_c_array: &CellArray,
) -> i32 {
    let mut i = i_first as usize;
    let mut p_data = *pp_data;
    let i_end = i_first + n_cell;
    let mut p_cellptr = p_cellptr;
    if i_end <= i_first {
        return 0;
    }
    let mut k = 0usize;
    while k < NB * 2 && p_c_array.ix_nx[k] <= i as i32 {
        k += 1;
    }
    let mut p_end = p_c_array.ap_end[k];
    loop {
        let mut rc = 0i32;
        debug_assert!(p_c_array.sz_cell[i] != 0);
        let sz = p_c_array.sz_cell[i] as usize;
        let slot = if p_pg.a_data[1] == 0 && p_pg.a_data[2] == 0 {
            0
        } else {
            page_find_slot(p_pg, sz, &mut rc)
        };
        let p_slot = if slot == 0 {
            if ((p_data as isize) - (p_begin as isize)) < sz as isize {
                return 1;
            }
            p_data -= sz;
            p_data
        } else {
            slot
        };
        // p_slot e a célula de origem nunca se sobrepõem num banco bem formado, mas podem num
        // banco corrompido: a cópia precisa tolerar sobreposição (memmove).
        let cell = &p_c_array.ap_cell[i];
        if cell.off + sz > p_end && cell.off < p_end {
            let _ = sqlite_corrupt_bkpt(line!() as i32);
            return 1;
        }
        p_pg.a_data[p_slot..p_slot + sz].copy_from_slice(&cell.buf[cell.off..cell.off + sz]);
        put2byte(&mut p_pg.a_data[p_cellptr..], p_slot as u16);
        p_cellptr += 2;
        i += 1;
        if i as i32 >= i_end {
            break;
        }
        if p_c_array.ix_nx[k] <= i as i32 {
            k += 1;
            p_end = p_c_array.ap_end[k];
        }
    }
    *pp_data = p_data;
    0
}

/// O objeto `p_c_array` contém as células b-tree e seus tamanhos.
///
/// Esta função acrescenta à lista livre de `p_pg` o espaço de cada célula do vetor que hoje
/// está no corpo da página. Os ponteiros de célula e os outros campos da página não são
/// atualizados.
///
/// Devolve o número total de células acrescentadas à lista livre.
pub fn page_free_array(p_pg: &mut MemPage, i_first: i32, n_cell: i32, p_c_array: &CellArray) -> i32 {
    let p_end = p_pg
        .p_bt
        .as_ref()
        .and_then(|b| b.upgrade())
        .map(|b| b.borrow().usable_size)
        .unwrap_or(0) as usize;
    let p_start = p_pg.hdr_offset as usize + 8 + p_pg.child_ptr_size as usize;
    let mut n_ret = 0i32;
    let i_end = i_first + n_cell;
    let mut n_free = 0usize;
    let mut a_ofst = [0i32; 10];
    let mut a_after = [0i32; 10];

    for i in i_first..i_end {
        let cell = &p_c_array.ap_cell[i as usize];
        if cell.pgno == p_pg.pgno && sqlite_within(cell.off, p_start, p_end) {
            // Não precisa de cached_cell_size() aqui: os tamanhos de todas as células a
            // liberar já foram calculados na hora de decidir quais células liberar.
            let sz = p_c_array.sz_cell[i as usize] as i32;
            debug_assert!(sz > 0);
            let i_ofst = (cell.off as u16) as i32;
            let i_after = i_ofst + sz;
            let mut j = 0usize;
            while j < n_free {
                if a_ofst[j] == i_after {
                    a_ofst[j] = i_ofst;
                    break;
                } else if a_after[j] == i_ofst {
                    a_after[j] = i_after;
                    break;
                }
                j += 1;
            }
            if j >= n_free {
                if n_free >= a_ofst.len() {
                    for j in 0..n_free {
                        let _ = free_space(p_pg, a_ofst[j] as u16, (a_after[j] - a_ofst[j]) as u16);
                    }
                    n_free = 0;
                }
                a_ofst[n_free] = i_ofst;
                a_after[n_free] = i_after;
                if i_after as usize > p_end {
                    return 0;
                }
                n_free += 1;
            }
            n_ret += 1;
        }
    }
    for j in 0..n_free {
        let _ = free_space(p_pg, a_ofst[j] as u16, (a_after[j] - a_ofst[j]) as u16);
    }
    n_ret
}

/// `p_c_array` contém ponteiros e tamanhos de todas as células da página em balanceamento. A
/// página atual, `p_pg`, tem `p_pg.n_cell` células a partir de `ap_cell[i_old]`. Depois do
/// balanceamento, esta página deve guardar `n_new` células a partir de `ap_cell[i_new]`.
///
/// Esta rotina faz os ajustes necessários em `p_pg` para que ela contenha as células certas
/// depois de balanceada.
///
/// O campo `p_pg.n_free` fica inválido ao retornar. É responsabilidade do chamador acertá-lo.
pub fn edit_page(
    p_pg: &mut MemPage,
    i_old: i32,
    i_new: i32,
    n_new: i32,
    p_c_array: &mut CellArray,
) -> i32 {
    let hdr = p_pg.hdr_offset as usize;
    let p_begin = p_pg.a_cell_idx + (n_new as usize) * 2;
    let mut n_cell = p_pg.n_cell as i32;
    let i_old_end = i_old + p_pg.n_cell as i32 + p_pg.n_overflow as i32;
    let i_new_end = i_new + n_new;

    'editpage_fail: {
        // Remove células do início e do fim da página.
        debug_assert!(n_cell >= 0);
        if i_old < i_new {
            let n_shift = page_free_array(p_pg, i_old, i_new - i_old, p_c_array);
            if n_shift > n_cell {
                return sqlite_corrupt_bkpt(line!() as i32);
            }
            let idx = p_pg.a_cell_idx;
            let src = idx + (n_shift as usize) * 2;
            p_pg.a_data.copy_within(src..src + (n_cell as usize) * 2, idx);
            n_cell -= n_shift;
        }
        if i_new_end < i_old_end {
            let n_tail = page_free_array(p_pg, i_new_end, i_old_end - i_new_end, p_c_array);
            debug_assert!(n_cell >= n_tail);
            n_cell -= n_tail;
        }

        let mut p_data = get2byte(&p_pg.a_data[hdr + 5..]) as usize;
        if p_data < p_begin {
            break 'editpage_fail;
        }
        if p_data > p_pg.a_data_end {
            break 'editpage_fail;
        }

        // Acrescenta células ao início da página.
        if i_new < i_old {
            let n_add = std::cmp::min(n_new, i_old - i_new);
            debug_assert!(n_add >= 0);
            let p_cellptr = p_pg.a_cell_idx;
            p_pg.a_data.copy_within(
                p_cellptr..p_cellptr + (n_cell as usize) * 2,
                p_cellptr + (n_add as usize) * 2,
            );
            if page_insert_array(p_pg, p_begin, &mut p_data, p_cellptr, i_new, n_add, p_c_array) != 0 {
                break 'editpage_fail;
            }
            n_cell += n_add;
        }

        // Acrescenta as células de overflow.
        for i in 0..p_pg.n_overflow as usize {
            let i_cell = (i_old + p_pg.ai_ovfl[i] as i32) - i_new;
            if i_cell >= 0 && i_cell < n_new {
                let p_cellptr = p_pg.a_cell_idx + (i_cell as usize) * 2;
                if n_cell > i_cell {
                    p_pg.a_data.copy_within(
                        p_cellptr..p_cellptr + ((n_cell - i_cell) as usize) * 2,
                        p_cellptr + 2,
                    );
                }
                n_cell += 1;
                cached_cell_size(p_c_array, i_cell + i_new);
                if page_insert_array(p_pg, p_begin, &mut p_data, p_cellptr, i_cell + i_new, 1, p_c_array)
                    != 0
                {
                    break 'editpage_fail;
                }
            }
        }

        // Acrescenta células ao fim da página.
        debug_assert!(n_cell >= 0);
        let p_cellptr = p_pg.a_cell_idx + (n_cell as usize) * 2;
        if page_insert_array(
            p_pg,
            p_begin,
            &mut p_data,
            p_cellptr,
            i_new + n_cell,
            n_new - n_cell,
            p_c_array,
        ) != 0
        {
            break 'editpage_fail;
        }

        p_pg.n_cell = n_new as u16;
        p_pg.n_overflow = 0;

        let n_cell_now = p_pg.n_cell;
        put2byte(&mut p_pg.a_data[hdr + 3..], n_cell_now);
        put2byte(&mut p_pg.a_data[hdr + 5..], p_data as u16);

        return SQLITE_OK;
    }
    // editpage_fail: não deu para editar a página, reconstrói do zero.
    if n_new < 1 {
        return sqlite_corrupt_bkpt(line!() as i32);
    }
    populate_cell_cache(p_c_array, i_new, n_new);
    rebuild_page(p_c_array, i_new, n_new, p_pg)
}


// ---- part_019.rs ----

// Contrato assumido com as outras partes:
//   allocate_btree_page(&BtSharedRef, &mut Option<MemPageRef>, &mut u32, u32, u8) -> i32
//   zero_page(&mut MemPage, u8)
//   find_cell(&MemPage, usize) -> usize        (deslocamento da célula em a_data)
//   insert_cell(&mut MemPage, i32, &mut [u8], i32, Option<&mut [u8]>, u32) -> i32   (parte 017)
//   rebuild_page(&CellArray, i32, i32, &mut MemPage) -> i32                          (parte 018)
//   ptrmap_put(&BtShared, u32, u8, u32, &mut i32), ptrmap_put_ovfl_ptr(&MemPage, &MemPage, &[u8], &mut i32)
//   btree_init_page(&mut MemPage) -> i32, btree_compute_free_space(&mut MemPage) -> i32
//   set_child_ptrmaps(&MemPageRef) -> i32
//   release_page(Option<&MemPageRef>)
//
// `balance_nonroot` NÃO está neste arquivo: a assinatura, as declarações e o começo do laço
// estão em btree_c.019.c, mas o corpo continua em btree_c.020.c (a função cruza a fronteira dos
// trechos, e uma função Rust não pode ser dividida entre arquivos). A tradução inteira precisa
// ser feita de uma vez, lendo os dois trechos, usando `CellArray`/`CellPtr`/`NB` da parte 017.
// A função `ptrmapCheckPages` fica dentro de `#if 0` no C e portanto não existe.

/// Esta versão de `balance()` trata o caso especial comum em que uma nova entrada está sendo
/// inserida na extremidade direita da árvore, ou seja, quando a nova entrada será a maior da
/// árvore.
///
/// Em vez de tentar balancear as 3 páginas folha mais à direita, apenas acrescenta uma nova
/// página no lado direito e coloca a única entrada nova nela. Isso deixa o lado direito da
/// árvore um tanto desbalanceado. Mas é provável que logo em seguida se insiram novas entradas
/// no fim, de modo que a página quase vazia se encherá rapidamente. Em média.
///
/// `p_page` é a página folha mais à direita da árvore. `p_parent` é o pai dela. `p_page` precisa
/// ter uma única célula de overflow, que também é a entrada mais à direita da página.
///
/// O buffer `p_space` guarda uma cópia temporária da célula divisora que será inserida em
/// `p_parent`. Essa célula é composta de um número de página de 4 bytes seguido de um inteiro de
/// tamanho variável. Em outras palavras, no máximo 13 bytes. Logo, `p_space` precisa ter pelo
/// menos 13 bytes.
fn balance_quick(p_parent: &MemPageRef, p_page: &MemPageRef, p_space: &mut [u8]) -> i32 {
    let p_bt: BtSharedRef = p_page.borrow().p_bt.as_ref().unwrap().upgrade().unwrap(); // Banco b-tree
    let mut p_new: Option<MemPageRef> = None; // Página recém alocada
    let mut rc: i32; // Código de retorno
    let mut pgno_new: u32 = 0; // Número de página de p_new

    if p_page.borrow().n_cell == 0 {
        return sqlite_corrupt_bkpt(line!() as i32); // dbfuzz001.test
    }

    // Aloca uma nova página. Essa página será a irmã direita de p_page. Torna a página pai
    // gravável, para que a nova célula divisora possa ser inserida. Se as duas operações
    // derem certo, prossegue.
    rc = allocate_btree_page(&p_bt, &mut p_new, &mut pgno_new, 0, 0);

    if rc == SQLITE_OK {
        let p_new_ref: MemPageRef = p_new.clone().unwrap();
        let mut p_out: usize = 4; // Índice de escrita em p_space (&p_space[4])
        let p_cell: Vec<u8> =p_page.borrow().ap_ovfl[0].as_ref().unwrap().to_vec();
        let sz_cell: u16 = {
            let pg = p_page.borrow();
            (pg.x_cell_size)(&*pg, &p_cell)
        };

        zero_page(&mut p_new_ref.borrow_mut(), PTF_INTKEY | PTF_LEAFDATA | PTF_LEAF);
        let b = CellArray {
            n_cell: 1,
            p_ref: p_page.clone(),
            ap_cell: vec![CellPtr { pgno: p_page.borrow().pgno, off: 0, buf: std::rc::Rc::new(p_cell.clone()) }],
            sz_cell: vec![sz_cell],
            ap_end: {
                let mut e = [0usize; NB * 2];
                e[0] = p_page.borrow().a_data_end;
                e
            },
            ix_nx: {
                let mut x = [0i32; NB * 2];
                x[0] = 2;
                x
            },
        };
        rc = rebuild_page(&b, 0, 1, &mut p_new_ref.borrow_mut());
        if rc != 0 {
            release_page(Some(&p_new_ref));
            return rc;
        }
        {
            let mut pn = p_new_ref.borrow_mut();
            pn.n_free = p_bt.borrow().usable_size as i32 - pn.cell_offset as i32 - 2 - sz_cell as i32;
        }

        // Se é um banco com auto-vacuum, atualiza o mapa de ponteiros com as entradas da nova
        // página e de qualquer ponteiro da célula da página para uma página de overflow. Se
        // qualquer uma dessas operações falhar, o código de retorno fica definido, mas o conteúdo
        // da página pai ainda é manipulado pelo código abaixo. Tudo bem: nesse ponto a página pai
        // com certeza está marcada como suja. Devolver um código de erro causa um rollback,
        // desfazendo qualquer mudança feita na página pai.
        if is_autovacuum(&p_bt.borrow()) {
            let parent_pgno = p_parent.borrow().pgno;
            ptrmap_put(&p_bt.borrow(), pgno_new, PTRMAP_BTREE, parent_pgno, &mut rc);
            let pn = p_new_ref.borrow();
            if sz_cell > pn.min_local {
                ptrmap_put_ovfl_ptr(&*pn, &*pn, &p_cell, &mut rc);
            }
        }

        // Cria uma célula divisora para inserir em p_parent. A célula divisora é composta de um
        // número de página de 4 bytes (o número de página de p_page) e de um valor de chave de
        // tamanho variável (que precisa ser igual à maior chave de p_page).
        //
        // Para achar a maior chave de p_page, primeiro acha-se a célula mais à direita de
        // p_page. Os dois primeiros campos dessa célula são o tamanho do registro (um inteiro de
        // tamanho variável de no máximo 32 bits) e o valor da chave (um inteiro de tamanho
        // variável, que pode ter qualquer valor). O primeiro dos laços while abaixo pula o campo
        // de tamanho do registro. O segundo copia o valor da chave da célula de p_page para o
        // buffer p_space.
        {
            let pg = p_page.borrow();
            let mut c = find_cell(&*pg, pg.n_cell as usize - 1);
            let mut p_stop = c + 9;
            loop {
                let byte = pg.a_data[c];
                c += 1;
                if !((byte & 0x80) != 0 && c < p_stop) {
                    break;
                }
            }
            p_stop = c + 9;
            loop {
                let byte = pg.a_data[c];
                p_space[p_out] = byte;
                p_out += 1;
                c += 1;
                if !((byte & 0x80) != 0 && c < p_stop) {
                    break;
                }
            }
        }

        // Insere a nova célula divisora em p_parent.
        if rc == SQLITE_OK {
            let n_cell_parent = p_parent.borrow().n_cell as i32;
            let child_pgno = p_page.borrow().pgno;
            rc = insert_cell(
                &mut p_parent.borrow_mut(),
                n_cell_parent,
                p_space,
                p_out as i32,
                None,
                child_pgno,
            );
        }

        // Faz o ponteiro filho direito de p_parent apontar para a nova página.
        {
            let mut par = p_parent.borrow_mut();
            let off = par.hdr_offset as usize + 8;
            put4byte(&mut par.a_data[off..], pgno_new);
        }

        // Libera a referência à nova página.
        release_page(Some(&p_new_ref));
    }

    rc
}

/// Esta função copia o conteúdo do nó b-tree armazenado na página `p_from` para a página
/// `p_to`. Se `p_from` não era uma página folha, as entradas do mapa de ponteiros de cada
/// página filha são atualizadas para que a página pai guardada no mapa seja `p_to`. Se `p_from`
/// continha células com ponteiros para páginas de overflow, as entradas correspondentes do mapa
/// de ponteiros também são atualizadas para que a página pai seja `p_to`.
///
/// Se `p_from` carrega células de overflow (entradas do vetor `MemPage.ap_ovfl[]`), elas não são
/// copiadas para `p_to`.
///
/// Antes de retornar, `p_to` é reinicializada com `btree_init_page()`.
///
/// O desempenho desta função não é crítico. Ela só é usada por `balance_shallower()` e
/// `balance_deeper()`, que não são chamadas com frequência em circunstâncias normais.
fn copy_node_content(p_from: &MemPageRef, p_to: &MemPageRef, p_rc: &mut i32) {
    if *p_rc == SQLITE_OK {
        let p_bt: BtSharedRef = p_from.borrow().p_bt.as_ref().unwrap().upgrade().unwrap();
        let usable_size = p_bt.borrow().usable_size as usize;
        let i_from_hdr = p_from.borrow().hdr_offset as usize;
        let i_to_hdr: usize = if p_to.borrow().pgno == 1 { 100 } else { 0 };
        let mut rc: i32;

        // Copia o conteúdo do nó b-tree da página p_from para a página p_to.
        let i_data = get2byte(&p_from.borrow().a_data[i_from_hdr + 5..]) as usize;
        {
            let from = p_from.borrow();
            let mut to = p_to.borrow_mut();
            to.a_data[i_data..usable_size].copy_from_slice(&from.a_data[i_data..usable_size]);
            let n = from.cell_offset as usize + 2 * from.n_cell as usize;
            to.a_data[i_to_hdr..i_to_hdr + n].copy_from_slice(&from.a_data[i_from_hdr..i_from_hdr + n]);
        }

        // Reinicializa a página p_to para que o conteúdo da estrutura MemPage case com os dados
        // novos. A inicialização de p_to pode de fato falhar em circunstâncias bem obscuras,
        // embora seja uma cópia da página inicializada p_from.
        p_to.borrow_mut().is_init = 0;
        rc = btree_init_page(&mut p_to.borrow_mut());
        if rc == SQLITE_OK {
            rc = btree_compute_free_space(&mut p_to.borrow_mut());
        }
        if rc != SQLITE_OK {
            *p_rc = rc;
            return;
        }

        // Se é um banco com auto-vacuum, atualiza as entradas do mapa de ponteiros de todas as
        // páginas b-tree ou de overflow para as quais p_to agora contém ponteiros.
        if is_autovacuum(&p_bt.borrow()) {
            *p_rc = set_child_ptrmaps(p_to);
        }
    }
}


// ---- part_020.rs ----

// Trecho btree_c.020.c: NÃO TRADUZIDO. Este trecho é o final do corpo de
// balance_nonroot (continua do btree_c.019.c, que tem a assinatura, as
// declarações locais e o início do laço). Uma função Rust não pode ser
// dividida entre arquivos, então a tradução fiel precisa ser feita junto com
// o trecho 019, depois que o tech lead fixar o tipo CellArray (apCell como
// ponteiros para dentro de páginas que são reescritas) e a assinatura.


// ---- part_021.rs ----

// Trecho btree_c.021.c (revisado e reescrito pelo revisor).
//
// Modelo assumido (o integrador precisa fixar estes nomes no `btreeInt`):
//  - `MemPageRef = Rc<RefCell<MemPage>>`; `MemPage.p_bt` é `Weak<RefCell<BtShared>>`
//    (use `mem_page_bt(&MemPageRef) -> BtSharedRef`).
//  - Ponteiro de célula do C é o deslocamento `usize` dentro de `MemPage.a_data`.
//  - Célula de overflow (`apOvfl[i]`) é `Vec<u8>` própria: `MemPage.ap_ovfl: Vec<Vec<u8>>`,
//    `ai_ovfl: [u16; 4]`, `n_overflow: u8`.
//  - `BtCursor.cursor_id: u64` é único por cursor (substitui a comparação `pOther!=pCur`).
//  - `BtShared.p_tmp_space: Vec<u8>` (o `newCell` do C); aqui é retirada com `mem::take`
//    e devolvida ao fim de `btree_insert`.
//  - `pPage->xCellSize` e `xParseCell` viram `mem_page_cell_size(&MemPageRef, &[u8], off)`
//    e `mem_page_parse_cell(&MemPageRef, &[u8], off, &mut CellInfo)`.
//  - `BtreePayload.p_key: Option<Vec<u8>>`, `p_data: Vec<u8>`.
//  - `sqlite3PageMalloc`/`PageFree` viram `Vec<u8>` zerado (o `pFree` do C só adia a
//    liberação, e as células de overflow já são cópias, então a ordem não é observável).

/// Essa função é chamada quando a página raiz de uma árvore b está cheia demais
/// (tem uma ou mais células de overflow).
///
/// Uma página filha nova é alocada e o conteúdo da raiz atual, inclusive as células
/// de overflow, é copiado para ela. A raiz vira uma página vazia com o ponteiro de
/// filho direito apontando para a página nova.
///
/// Antes de retornar, as entradas do mapa de ponteiros das páginas para as quais a
/// filha nova agora aponta são atualizadas, e também a do novo filho direito da raiz.
///
/// Se tiver sucesso, `*pp_child` recebe uma referência à filha e devolve SQLITE_OK;
/// o chamador precisa chamar `release_page` nela exatamente uma vez. Em erro,
/// devolve o código e `*pp_child` fica `None`.
fn balance_deeper(p_root: &MemPageRef, pp_child: &mut Option<MemPageRef>) -> i32 {
    let mut p_child: Option<MemPageRef> = None;
    let mut pgno_child: Pgno = 0;
    let p_bt = mem_page_bt(p_root);

    // Torna a raiz gravável, aloca a página que será o novo filho direito e copia
    // para ela o conteúdo do nó guardado na raiz.
    let mut rc = pager_write(&p_root.borrow().p_db_page);
    if rc == SQLITE_OK {
        let root_pgno = p_root.borrow().pgno;
        rc = allocate_btree_page(&p_bt, &mut p_child, &mut pgno_child, root_pgno, 0);
        // O C chama copyNodeContent mesmo com rc de erro (a função ignora rc != OK).
        if let Some(c) = p_child.as_ref() {
            copy_node_content(p_root, c, &mut rc);
        }
        if isautovacuum(&p_bt) {
            ptrmap_put(&p_bt, pgno_child, PTRMAP_BTREE, root_pgno, &mut rc);
        }
    }
    if rc != SQLITE_OK {
        *pp_child = None;
        release_page(p_child.as_ref());
        return rc;
    }
    let p_child = p_child.unwrap();

    // Copia as células de overflow da raiz para a filha.
    {
        let root = p_root.borrow();
        let mut child = p_child.borrow_mut();
        let n = root.n_overflow as usize;
        child.ai_ovfl[..n].copy_from_slice(&root.ai_ovfl[..n]);
        child.ap_ovfl = root.ap_ovfl[..n].to_vec();
        child.n_overflow = root.n_overflow;
    }

    // Zera o conteúdo da raiz e instala a filha como filho direito.
    let flags = p_child.borrow().a_data[0] & !PTF_LEAF;
    zero_page(p_root, flags);
    {
        let mut root = p_root.borrow_mut();
        let off = root.hdr_offset as usize + 8;
        put4byte(&mut root.a_data, off, pgno_child);
    }

    *pp_child = Some(p_child);
    SQLITE_OK
}

/// Devolve SQLITE_CORRUPT se algum cursor diferente de `p_cur` estiver válido na
/// mesma árvore b de `p_cur` e apontando para a mesma página.
///
/// Isso acontece se um banco corrompido tem duas ou mais tabelas SQL apontando
/// para a mesma árvore b: um INSERT numa delas dispara um BEFORE TRIGGER que insere
/// na outra, e o rebalanceamento muda o conteúdo debaixo do cursor da primeira.
fn another_valid_cursor(p_cur: &BtCursor) -> i32 {
    let p_bt = p_cur.p_bt.clone();
    let mut p_other = p_bt.borrow().p_cursor.clone();
    while let Some(other_ref) = p_other {
        let other = other_ref.borrow();
        if other.cursor_id != p_cur.cursor_id
            && other.e_state == CURSOR_VALID
            && match (&other.p_page, &p_cur.p_page) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
        {
            return sqlite_corrupt_page(p_cur.p_page.as_ref().unwrap());
        }
        let next = other.p_next.clone();
        drop(other);
        p_other = next;
    }
    SQLITE_OK
}

/// A página para a qual `p_cur` aponta acabou de ser modificada. Decide se a árvore
/// precisa de balanceamento e chama a rotina adequada:
///
///   balance_quick()
///   balance_deeper()
///   balance_nonroot()
fn balance(p_cur: &mut BtCursor) -> i32 {
    let mut rc = SQLITE_OK;
    let mut a_balance_quick_space = [0u8; 13];

    loop {
        let p_page = p_cur.p_page.clone().unwrap();

        if p_page.borrow().n_free < 0 && btree_compute_free_space(&p_page) != SQLITE_OK {
            break;
        }
        let usable_size = p_cur.p_bt.borrow().usable_size as i32;
        let (n_overflow, n_free) = {
            let pg = p_page.borrow();
            (pg.n_overflow, pg.n_free as i32)
        };
        let i_page = p_cur.i_page as i32;
        if n_overflow == 0 && n_free * 3 <= usable_size * 2 {
            // Não precisa rebalancear enquanto: (1) não há células de overflow e
            // (2) o espaço livre é menor que 2/3 do espaço útil da página.
            break;
        } else if i_page == 0 {
            if n_overflow != 0 {
                rc = another_valid_cursor(p_cur);
            }
            if n_overflow != 0 && rc == SQLITE_OK {
                // A raiz está cheia demais: balance_deeper cria uma filha para a
                // raiz e copia o conteúdo atual para ela. A próxima iteração
                // balanceia a filha.
                let mut child: Option<MemPageRef> = None;
                rc = balance_deeper(&p_page, &mut child);
                p_cur.ap_page[1] = child;
                if rc == SQLITE_OK {
                    p_cur.i_page = 1;
                    p_cur.ix = 0;
                    p_cur.ai_idx[0] = 0;
                    p_cur.ap_page[0] = Some(p_page.clone());
                    p_cur.p_page = p_cur.ap_page[1].clone();
                }
            } else {
                break;
            }
        } else if pager_page_refcount(&p_page.borrow().p_db_page) > 1 {
            // A página escrita não é raiz e tem mais de uma referência: ela é
            // ancestral de si mesma. Corrupção.
            rc = sqlite_corrupt_page(&p_page);
        } else {
            let p_parent = p_cur.ap_page[(i_page - 1) as usize].clone().unwrap();
            let i_idx = p_cur.ai_idx[(i_page - 1) as usize] as i32;

            rc = pager_write(&p_parent.borrow().p_db_page);
            if rc == SQLITE_OK && p_parent.borrow().n_free < 0 {
                rc = btree_compute_free_space(&p_parent);
            }
            if rc == SQLITE_OK {
                let quick = {
                    let pg = p_page.borrow();
                    let par = p_parent.borrow();
                    pg.int_key_leaf != 0
                        && pg.n_overflow == 1
                        && pg.ai_ovfl[0] == pg.n_cell
                        && par.pgno != 1
                        && par.n_cell as i32 == i_idx
                };
                if quick {
                    // balance_quick cria um irmão novo de pPage para guardar a célula
                    // de overflow e insere uma célula em pParent, que pode transbordar;
                    // a próxima iteração do laço trata isso.
                    rc = balance_quick(&p_parent, &p_page, &mut a_balance_quick_space);
                } else {
                    // balance_nonroot redistribui as células entre pPage e até dois
                    // irmãos, o que modifica pParent. O espaço auxiliar vive só nesta
                    // chamada (ver a nota do cabeçalho sobre pFree).
                    let page_size = p_cur.p_bt.borrow().page_size as usize;
                    let p_space = vec![0u8; page_size];
                    rc = balance_nonroot(
                        &p_parent,
                        i_idx,
                        p_space,
                        (i_page == 1) as i32,
                        (p_cur.hints & BTREE_BULKLOAD) as i32,
                    );
                }
            }

            p_page.borrow_mut().n_overflow = 0;

            // A próxima iteração do laço balanceia a página pai.
            release_page(Some(&p_page));
            p_cur.i_page -= 1;
            p_cur.p_page = p_cur.ap_page[p_cur.i_page as usize].clone();
        }
        if rc != SQLITE_OK {
            break;
        }
    }
    rc
}

/// Sobrescreve o conteúdo de `p_x` a partir de `dest` (deslocamento em `a_data` de
/// `p_page`). Só grava se o conteúdo for diferente do que já está lá.
fn btree_overwrite_content(
    p_page: &MemPageRef,
    dest: usize,
    p_x: &BtreePayload,
    i_offset: i32,
    i_amt: i32,
) -> i32 {
    let mut i_amt = i_amt;
    let n_data = p_x.n_data - i_offset;
    if n_data <= 0 {
        // Sobrescrevendo com zeros.
        let mut i = 0i32;
        {
            let pg = p_page.borrow();
            while i < i_amt && pg.a_data[dest + i as usize] == 0 {
                i += 1;
            }
        }
        if i < i_amt {
            let rc = pager_write(&p_page.borrow().p_db_page);
            if rc != SQLITE_OK {
                return rc;
            }
            let mut pg = p_page.borrow_mut();
            pg.a_data[dest + i as usize..dest + i_amt as usize].fill(0);
        }
    } else {
        if n_data < i_amt {
            // Dados reais seguidos de zeros: chamada recursiva escreve os zeros e
            // depois cai para escrever os dados reais.
            let rc = btree_overwrite_content(
                p_page,
                dest + n_data as usize,
                p_x,
                i_offset + n_data,
                i_amt - n_data,
            );
            if rc != SQLITE_OK {
                return rc;
            }
            i_amt = n_data;
        }
        let src = &p_x.p_data[i_offset as usize..(i_offset + i_amt) as usize];
        let differs = {
            let pg = p_page.borrow();
            pg.a_data[dest..dest + i_amt as usize] != *src
        };
        if differs {
            let rc = pager_write(&p_page.borrow().p_db_page);
            if rc != SQLITE_OK {
                return rc;
            }
            p_page.borrow_mut().a_data[dest..dest + i_amt as usize].copy_from_slice(src);
        }
    }
    SQLITE_OK
}

/// Sobrescreve a célula para a qual `p_cur` aponta com o conteúdo de `p_x`.
/// Nesta variante a célula tem conteúdo em páginas de overflow.
fn btree_overwrite_overflow_cell(p_cur: &mut BtCursor, p_x: &BtreePayload) -> i32 {
    let n_total = p_x.n_data + p_x.n_zero;
    let p_page = p_cur.p_page.clone().unwrap();
    let p_payload = p_cur.info.p_payload;
    let n_local = p_cur.info.n_local as i32;

    // Sobrescreve primeiro a parte local.
    let rc = btree_overwrite_content(&p_page, p_payload, p_x, 0, n_local);
    if rc != SQLITE_OK {
        return rc;
    }

    // Agora as páginas de overflow.
    let mut i_offset = n_local;
    let mut ovfl_pgno: Pgno = get4byte(&p_page.borrow().a_data, p_payload + i_offset as usize);
    let p_bt = mem_page_bt(&p_page);
    let mut ovfl_page_size: u32 = p_bt.borrow().usable_size - 4;
    loop {
        let mut p_ovfl: Option<MemPageRef> = None;
        let mut rc = btree_get_page(&p_bt, ovfl_pgno, &mut p_ovfl, 0);
        if rc != SQLITE_OK {
            return rc;
        }
        let p_ovfl = p_ovfl.unwrap();
        if pager_page_refcount(&p_ovfl.borrow().p_db_page) != 1 || p_ovfl.borrow().is_init != 0 {
            rc = sqlite_corrupt_page(&p_ovfl);
        } else {
            if (i_offset as u32).wrapping_add(ovfl_page_size) < n_total as u32 {
                ovfl_pgno = get4byte(&p_ovfl.borrow().a_data, 0);
            } else {
                ovfl_page_size = (n_total - i_offset) as u32;
            }
            rc = btree_overwrite_content(&p_ovfl, 4, p_x, i_offset, ovfl_page_size as i32);
        }
        pager_unref(&p_ovfl.borrow().p_db_page);
        if rc != SQLITE_OK {
            return rc;
        }
        i_offset += ovfl_page_size as i32;
        if i_offset >= n_total {
            break;
        }
    }
    SQLITE_OK
}

/// Sobrescreve a célula para a qual `p_cur` aponta com o conteúdo de `p_x`.
fn btree_overwrite_cell(p_cur: &mut BtCursor, p_x: &BtreePayload) -> i32 {
    let n_total = p_x.n_data + p_x.n_zero;
    let p_page = p_cur.p_page.clone().unwrap();

    {
        let pg = p_page.borrow();
        if p_cur.info.p_payload + p_cur.info.n_local as usize > pg.a_data_end
            || p_cur.info.p_payload < pg.cell_offset as usize
        {
            return sqlite_corrupt_page(&p_page);
        }
    }
    if p_cur.info.n_local as i32 == n_total {
        // A célula inteira é local.
        btree_overwrite_content(&p_page, p_cur.info.p_payload, p_x, 0, p_cur.info.n_local as i32)
    } else {
        // A célula tem conteúdo de overflow.
        btree_overwrite_overflow_cell(p_cur, p_x)
    }
}

/// Insere um registro novo na árvore b. O conteúdo é descrito por `p_x`. O cursor
/// `p_cur` só define em qual tabela inserir e fica apontando para um lugar qualquer.
///
/// Para árvore b de tabela (tabelas de rowid) só `p_x.n_key` (o rowid) é usado e
/// `p_x.p_key` deve ser `None`; `n_data`, `p_data` e `n_zero` guardam a linha.
///
/// Para árvore b de índice (índices e WITHOUT ROWID) a chave é uma sequência
/// arbitrária de bytes em `p_x.p_key`/`n_key`, e os campos de dados devem ser zero.
///
/// Se `seek_result` for diferente de zero, um `btree_index_moveto` bem sucedido já
/// posicionou o cursor numa célula adjacente à que será inserida: `< 0` se a célula
/// é menor que a chave, `> 0` se é maior. Se for zero, o cursor está num lugar
/// desconhecido e esta rotina o posiciona antes de inserir. Em índices, se
/// `p_x.n_mem` for diferente de zero, `p_x.a_mem` evita decodificar a chave.
pub fn btree_insert(p_cur: &mut BtCursor, p_x: &BtreePayload, flags: i32, seek_result: i32) -> i32 {
    let mut rc: i32;
    let mut loc = seek_result; // -1: antes do local desejado, +1: depois
    let mut sz_new: i32 = 0;
    let mut idx: i32;
    let p = p_cur.p_btree.clone();
    let p_bt = p.borrow().p_bt.clone();

    // Salva as posições de qualquer outro cursor aberto nesta tabela. Em alguns
    // casos o btree_moveto abaixo não faz nada (INSERT com chave inteira gerada
    // automaticamente), então é importante não limpar o cursor aqui.
    if p_cur.cur_flags & BTCF_MULTIPLE != 0 {
        rc = save_all_cursors(&p_bt, p_cur.pgno_root, Some(&*p_cur));
        if rc != SQLITE_OK {
            return rc;
        }
        if loc != 0 && p_cur.i_page < 0 {
            // Só acontece com esquema corrompido em que mais de uma tabela ou
            // índice usa a mesma página raiz do cursor.
            return sqlite_corrupt_pgno(p_cur.pgno_root);
        }
    }

    // Garante que o cursor não está em CURSOR_FAULT e aponta para célula válida.
    if p_cur.e_state >= CURSOR_REQUIRESEEK {
        rc = move_to_root(p_cur);
        if rc != SQLITE_OK && rc != SQLITE_EMPTY {
            return rc;
        }
    }

    if p_cur.p_key_info.is_none() {
        // Inserção numa b-tree de tabela: invalida cursores incrblob da linha trocada.
        if p.borrow().has_incrblob_cur != 0 {
            invalidate_incrblob_cursors(&p, p_cur.pgno_root, p_x.n_key, 0);
        }

        // BTREE_SAVEPOSITION==0 não implica que o cursor não aponte para uma linha
        // a sobrescrever, então a checagem é completa.
        if p_cur.cur_flags & BTCF_VALID_NKEY != 0 && p_x.n_key == p_cur.info.n_key {
            // O cursor aponta para a entrada a sobrescrever.
            if p_cur.info.n_size != 0
                && p_cur.info.n_payload == (p_x.n_data as u32).wrapping_add(p_x.n_zero as u32)
            {
                // Entrada nova com o mesmo tamanho da antiga: sobrescreve.
                return btree_overwrite_cell(p_cur, p_x);
            }
        } else if loc == 0 {
            // O cursor não aponta para a célula a sobrescrever nem para uma
            // adjacente: move para a célula a sobrescrever ou uma adjacente.
            rc = btree_table_moveto(p_cur, p_x.n_key, (flags & BTREE_APPEND) != 0, &mut loc);
            if rc != SQLITE_OK {
                return rc;
            }
        }
    } else {
        // Índice ou tabela WITHOUT ROWID.
        if loc == 0 && (flags & BTREE_SAVEPOSITION) == 0 {
            if p_x.n_mem != 0 {
                let r = UnpackedRecord {
                    p_key_info: p_cur.p_key_info.clone(),
                    a_mem: p_x.a_mem.clone(),
                    n_field: p_x.n_mem,
                    default_rc: 0,
                    eq_seen: 0,
                    ..Default::default()
                };
                rc = btree_index_moveto(p_cur, &r, &mut loc);
            } else {
                rc = btree_moveto(
                    p_cur,
                    p_x.p_key.as_deref(),
                    p_x.n_key,
                    (flags & BTREE_APPEND) != 0,
                    &mut loc,
                );
            }
            if rc != SQLITE_OK {
                return rc;
            }
        }

        // Se o cursor aponta para uma entrada a sobrescrever e o conteúdo novo é
        // igual ao antigo, usa a otimização de sobrescrita.
        if loc == 0 {
            get_cell_info(p_cur);
            if p_cur.info.n_key == p_x.n_key {
                let x2 = BtreePayload {
                    p_data: p_x.p_key.clone().unwrap_or_default(),
                    n_data: p_x.n_key as i32,
                    n_zero: 0,
                    ..Default::default()
                };
                return btree_overwrite_cell(p_cur, &x2);
            }
        }
    }

    let p_page = p_cur.p_page.clone().unwrap();
    if p_page.borrow().n_free < 0 {
        // O move_to_root acima garante que e_state não passa de CURSOR_INVALID.
        if p_cur.e_state > CURSOR_INVALID {
            rc = sqlite_corrupt_page(&p_page);
        } else {
            rc = btree_compute_free_space(&p_page);
        }
        if rc != SQLITE_OK {
            return rc;
        }
    }

    trace!(
        "INSERT: table={} nkey={} ndata={} page={} {}\n",
        p_cur.pgno_root,
        p_x.n_key,
        p_x.n_data,
        p_page.borrow().pgno,
        if loc == 0 { "overwrite" } else { "new entry" }
    );

    // O buffer `newCell` do C é o p_tmp_space do BtShared.
    let mut new_cell: Vec<u8> = std::mem::take(&mut p_bt.borrow_mut().p_tmp_space);

    'end_insert: {
        if flags & BTREE_PREFORMAT != 0 {
            rc = SQLITE_OK;
            sz_new = p_bt.borrow().n_preformat_size as i32;
            if sz_new < 4 {
                sz_new = 4;
                new_cell[3] = 0;
            }
            if isautovacuum(&p_bt) && sz_new > p_page.borrow().max_local as i32 {
                let mut info = CellInfo::default();
                mem_page_parse_cell(&p_page, &new_cell, 0, &mut info);
                if info.n_payload != info.n_local as u32 {
                    let ovfl = get4byte(&new_cell, sz_new as usize - 4);
                    let pgno = p_page.borrow().pgno;
                    ptrmap_put(&p_bt, ovfl, PTRMAP_OVERFLOW1, pgno, &mut rc);
                    if rc != SQLITE_OK {
                        break 'end_insert;
                    }
                }
            }
        } else {
            rc = fill_in_cell(&p_page, &mut new_cell, p_x, &mut sz_new);
            if rc != SQLITE_OK {
                break 'end_insert;
            }
        }
        idx = p_cur.ix as i32;
        p_cur.info.n_size = 0;
        if loc == 0 {
            let mut info = CellInfo::default();
            if idx >= p_page.borrow().n_cell as i32 {
                rc = sqlite_corrupt_page(&p_page);
                break 'end_insert;
            }
            rc = pager_write(&p_page.borrow().p_db_page);
            if rc != SQLITE_OK {
                break 'end_insert;
            }
            let old_cell = find_cell(&p_page.borrow(), idx as usize);
            if p_page.borrow().leaf == 0 {
                let pg = p_page.borrow();
                new_cell[..4].copy_from_slice(&pg.a_data[old_cell..old_cell + 4]);
            }
            rc = btree_clear_cell(&p_page, old_cell, &mut info);
            invalidate_overflow_cache(p_cur);
            if info.n_size as i32 == sz_new
                && info.n_local as u32 == info.n_payload
                && (!isautovacuum(&p_bt) || sz_new < p_page.borrow().min_local as i32)
            {
                // Sobrescreve a célula antiga com a nova se têm o mesmo tamanho.
                // Não vale em banco autovacuum se a nova usa overflow, pois o
                // insert_cell abaixo é necessário para a entrada PTRMAP_OVERFLOW1.
                let (hdr, data_end) = {
                    let pg = p_page.borrow();
                    (pg.hdr_offset as usize, pg.a_data_end)
                };
                if old_cell < hdr + 10 {
                    rc = sqlite_corrupt_page(&p_page);
                    break 'end_insert;
                }
                if old_cell + sz_new as usize > data_end {
                    rc = sqlite_corrupt_page(&p_page);
                    break 'end_insert;
                }
                p_page.borrow_mut().a_data[old_cell..old_cell + sz_new as usize]
                    .copy_from_slice(&new_cell[..sz_new as usize]);
                rc = SQLITE_OK;
                break 'end_insert;
            }
            drop_cell(&p_page, idx, info.n_size as i32, &mut rc);
            if rc != SQLITE_OK {
                break 'end_insert;
            }
        } else if loc < 0 && p_page.borrow().n_cell > 0 {
            p_cur.ix += 1;
            idx = p_cur.ix as i32;
            p_cur.cur_flags &= !(BTCF_VALID_NKEY | BTCF_VALID_OVFL);
        }
        rc = insert_cell_fast(&p_page, idx, &new_cell, sz_new);

        // Se não houve erro e pPage tem célula de overflow, chama balance() para
        // redistribuir as células. Como balance() pode mover o cursor, zera
        // info.n_size e BTCF_VALID_NKEY.
        //
        // Em vez de voltar à raiz, o cursor é marcado como inválido, o que torna
        // as inserções comuns um pouco mais rápidas e permite, em INSERT ... SELECT
        // com chave inteira crescente, inserir sem reposicionar o cursor.
        if p_page.borrow().n_overflow != 0 {
            p_cur.cur_flags &= !(BTCF_VALID_NKEY | BTCF_VALID_OVFL);
            rc = balance(p_cur);

            // nOverflow precisa voltar a zero mesmo se balance() falhar, senão a
            // estrutura interna fica corrompida. O estado inválido impede o
            // save_cursor_position de tentar salvar a posição.
            p_cur.p_page.as_ref().unwrap().borrow_mut().n_overflow = 0;
            p_cur.e_state = CURSOR_INVALID;
            if (flags & BTREE_SAVEPOSITION) != 0 && rc == SQLITE_OK {
                btree_release_all_cursor_pages(p_cur);
                if p_cur.p_key_info.is_some() {
                    p_cur.p_key = Some(
                        p_x.p_key.as_ref().map(|k| k[..p_x.n_key as usize].to_vec()).unwrap_or_default(),
                    );
                }
                p_cur.e_state = CURSOR_REQUIRESEEK;
                p_cur.n_key = p_x.n_key;
            }
        }
    }

    // end_insert: devolve o buffer temporário ao BtShared.
    p_bt.borrow_mut().p_tmp_space = new_cell;
    rc
}


// ---- part_022.rs ----

// Trecho btree_c.022.c (escrito pelo revisor: o arquivo não existia).
// O modelo de memória é o descrito no cabeçalho de part_021.rs. Funções novas que o
// integrador precisa fornecer: `pager_get(&PagerRef, Pgno, &mut Option<DbPageRef>, i32)`,
// `pager_get_data(&DbPageRef) -> Vec<u8>` (cópia dos bytes da página),
// `put_varint(&mut [u8], u64) -> i32`, `ptrmap_get(&BtSharedRef, Pgno, &mut u8, &mut Pgno) -> i32`,
// `insert_cell(&MemPageRef, i32, &[u8], i32, &mut Vec<u8>, Pgno) -> i32`,
// `btree_payload_to_local(&MemPageRef, i64) -> i32`, `save_cursor_key(&mut BtCursor) -> i32`,
// `btree_restore_cursor_position(&mut BtCursor) -> i32`, `ptrmap_pageno`, `pending_byte_page`.

/// Corpo de `btree_transfer_row`; `tmp` é o `pTmpSpace` do BtShared de destino.
fn btree_transfer_row_body(
    p_dest: &mut BtCursor,
    p_src: &mut BtCursor,
    i_key: i64,
    tmp: &mut Vec<u8>,
) -> i32 {
    let p_bt = p_dest.p_bt.clone();
    let mut a_out: usize = 0; // próximo byte de saída no buffer atual

    get_cell_info(p_src);
    let n_payload = p_src.info.n_payload;
    if n_payload < 0x80 {
        tmp[a_out] = n_payload as u8;
        a_out += 1;
    } else {
        a_out += put_varint(&mut tmp[a_out..], n_payload as u64) as usize;
    }
    if p_dest.p_key_info.is_none() {
        a_out += put_varint(&mut tmp[a_out..], i_key as u64) as usize;
    }
    let mut n_in: u32 = p_src.info.n_local as u32;
    let mut a_in: usize = p_src.info.p_payload;
    let src_page = p_src.p_page.clone().unwrap();
    let src_data_end = src_page.borrow().a_data_end;
    if a_in + n_in as usize > src_data_end {
        return sqlite_corrupt_page(&src_page);
    }
    let mut n_rem: u32 = n_payload;
    let dest_max_local = p_dest.p_page.as_ref().unwrap().borrow().max_local as u32;
    if n_in == n_rem && n_in < dest_max_local {
        let n = n_in as usize;
        tmp[a_out..a_out + n].copy_from_slice(&src_page.borrow().a_data[a_in..a_in + n]);
        p_bt.borrow_mut().n_preformat_size = n_in + a_out as u32;
        return SQLITE_OK;
    }

    let mut rc = SQLITE_OK;
    let p_src_pager = p_src.p_bt.borrow().p_pager.clone();
    let mut p_pgno_out: Option<usize> = None; // deslocamento do campo de overflow no buffer atual
    let mut ovfl_in: Pgno = 0;
    let mut p_page_in: Option<DbPageRef> = None;
    let mut in_data: Option<Vec<u8>> = None; // None: a leitura vem de src_page
    let mut p_page_out: Option<MemPageRef> = None;

    let mut n_out: u32 =
        btree_payload_to_local(p_dest.p_page.as_ref().unwrap(), n_payload as i64) as u32;
    p_bt.borrow_mut().n_preformat_size = n_out + a_out as u32;
    if n_out < n_payload {
        p_pgno_out = Some(a_out + n_out as usize);
        p_bt.borrow_mut().n_preformat_size += 4;
    }

    if n_rem > n_in {
        if a_in + n_in as usize + 4 > src_data_end {
            return sqlite_corrupt_page(&src_page);
        }
        ovfl_in = get4byte(&src_page.borrow().a_data, p_src.info.p_payload + n_in as usize);
    }

    loop {
        n_rem -= n_out;
        loop {
            if n_in > 0 {
                let n_copy = n_out.min(n_in) as usize;
                // Origem: página fonte ou página de overflow lida do paginador.
                let chunk: Vec<u8> = match &in_data {
                    Some(d) => d[a_in..a_in + n_copy].to_vec(),
                    None => src_page.borrow().a_data[a_in..a_in + n_copy].to_vec(),
                };
                match &p_page_out {
                    Some(po) => po.borrow_mut().a_data[a_out..a_out + n_copy].copy_from_slice(&chunk),
                    None => tmp[a_out..a_out + n_copy].copy_from_slice(&chunk),
                }
                n_out -= n_copy as u32;
                n_in -= n_copy as u32;
                a_out += n_copy;
                a_in += n_copy;
            }
            if n_out > 0 {
                pager_unref_opt(p_page_in.take());
                rc = pager_get(&p_src_pager, ovfl_in, &mut p_page_in, PAGER_GET_READONLY);
                if rc == SQLITE_OK {
                    let d = pager_get_data(p_page_in.as_ref().unwrap());
                    ovfl_in = get4byte(&d, 0);
                    a_in = 4;
                    n_in = p_src.p_bt.borrow().usable_size - 4;
                    in_data = Some(d);
                }
            }
            if !(rc == SQLITE_OK && n_out > 0) {
                break;
            }
        }

        if rc == SQLITE_OK && n_rem > 0 && p_pgno_out.is_some() {
            let mut pgno_new: Pgno = 0;
            let mut p_new: Option<MemPageRef> = None;
            rc = allocate_btree_page(&p_bt, &mut p_new, &mut pgno_new, 0, 0);
            let off = p_pgno_out.unwrap();
            match &p_page_out {
                Some(po) => put4byte(&mut po.borrow_mut().a_data, off, pgno_new),
                None => put4byte(tmp, off, pgno_new),
            }
            if isautovacuum(&p_bt) {
                if let Some(po) = &p_page_out {
                    let pgno_out = po.borrow().pgno;
                    ptrmap_put(&p_bt, pgno_new, PTRMAP_OVERFLOW2, pgno_out, &mut rc);
                }
            }
            release_page(p_page_out.as_ref());
            p_page_out = p_new;
            if let Some(po) = &p_page_out {
                p_pgno_out = Some(0);
                put4byte(&mut po.borrow_mut().a_data, 0, 0);
                a_out = 4;
                n_out = (p_bt.borrow().usable_size - 4).min(n_rem);
            }
        }
        if !(n_rem > 0 && rc == SQLITE_OK) {
            break;
        }
    }

    release_page(p_page_out.as_ref());
    pager_unref_opt(p_page_in.take());
    rc
}

/// Parte da cópia da linha atual do cursor `p_src` para `p_dest`. Se os cursores
/// estão em tabelas de chave inteira, `i_key` é o rowid usado ao copiar; senão o
/// registro é copiado literalmente.
///
/// Não grava a linha em `p_dest`: cria as páginas de overflow necessárias e escreve
/// os dados da célula nova no `pTmpSpace` do BtShared de destino. O tamanho da célula
/// fica em `nPreformatSize`; o chamador completa a inserção com `btree_insert` e a
/// flag BTREE_PREFORMAT. Devolve SQLITE_OK ou um código de erro.
pub fn btree_transfer_row(p_dest: &mut BtCursor, p_src: &mut BtCursor, i_key: i64) -> i32 {
    let p_bt = p_dest.p_bt.clone();
    let mut tmp = std::mem::take(&mut p_bt.borrow_mut().p_tmp_space);
    let rc = btree_transfer_row_body(p_dest, p_src, i_key, &mut tmp);
    p_bt.borrow_mut().p_tmp_space = tmp;
    rc
}

/// Apaga a entrada para a qual o cursor aponta.
///
/// Se o bit BTREE_SAVEPOSITION de `flags` for zero, o cursor fica num lugar qualquer
/// depois do delete. Se estiver ligado, fica num estado em que o próximo
/// `btree_next`/`btree_previous` o leva à mesma linha que levaria sem o delete.
///
/// O bit BTREE_AUXDELETE marca um de vários deletes associados a uma entrada de
/// tabela e seus índices; é só uma dica, que esta implementação não usa.
pub fn btree_delete(p_cur: &mut BtCursor, flags: u8) -> i32 {
    let p = p_cur.p_btree.clone();
    let p_bt = p.borrow().p_bt.clone();
    let mut rc: i32;
    let mut info = CellInfo::default();
    let mut b_preserve: u8; // 2 para CURSOR_SKIPNEXT

    if p_cur.e_state != CURSOR_VALID {
        if p_cur.e_state >= CURSOR_REQUIRESEEK {
            rc = btree_restore_cursor_position(p_cur);
            if rc != SQLITE_OK || p_cur.e_state != CURSOR_VALID {
                return rc;
            }
        } else {
            return sqlite_corrupt_pgno(p_cur.pgno_root);
        }
    }

    let i_cell_depth = p_cur.i_page as i32;
    let i_cell_idx = p_cur.ix as i32;
    let p_page = p_cur.p_page.clone().unwrap();
    if p_page.borrow().n_cell as i32 <= i_cell_idx {
        return sqlite_corrupt_page(&p_page);
    }
    let mut p_cell = find_cell(&p_page.borrow(), i_cell_idx as usize);
    if p_page.borrow().n_free < 0 && btree_compute_free_space(&p_page) != SQLITE_OK {
        return sqlite_corrupt_page(&p_page);
    }
    {
        // aCellIdx[nCell] é um deslocamento em bytes (aCellIdx é u8*).
        let pg = p_page.borrow();
        if p_cell < pg.cell_offset as usize + pg.n_cell as usize {
            drop(pg);
            return sqlite_corrupt_page(&p_page);
        }
    }

    // Se BTREE_SAVEPOSITION está ligado, a posição do cursor precisa ser preservada.
    // Se o delete causar rebalanceamento, isso é feito guardando a chave do cursor e
    // deixando-o em CURSOR_REQUIRESEEK. Senão, o cursor fica em CURSOR_SKIPNEXT
    // apontando para a entrada logo antes ou depois da apagada.
    //
    //    b_preserve==0   não precisa salvar a posição
    //    b_preserve==1   usa CURSOR_REQUIRESEEK para salvar a posição
    //    b_preserve==2   o cursor não se move; define CURSOR_SKIPNEXT
    b_preserve = ((flags & BTREE_SAVEPOSITION) != 0) as u8;
    if b_preserve != 0 {
        let (leaf, n_free, n_cell) = {
            let pg = p_page.borrow();
            (pg.leaf != 0, pg.n_free as i32, pg.n_cell)
        };
        let cell_size = mem_page_cell_size(&p_page, &p_page.borrow().a_data, p_cell) as i32;
        if !leaf
            || (n_free + cell_size + 2) > (p_bt.borrow().usable_size * 2 / 3) as i32
            || n_cell == 1 // ver dbfuzz001.test
        {
            // Vai precisar de rebalanceamento: guarda a chave do cursor.
            rc = save_cursor_key(p_cur);
            if rc != SQLITE_OK {
                return rc;
            }
        } else {
            b_preserve = 2;
        }
    }

    // Se a página da entrada não é folha, move o cursor para a maior entrada da
    // árvore menor que a apagada. Essa célula substitui a apagada no nó interno.
    // Usa-se a anterior e não a próxima porque a anterior sempre pertence à
    // subárvore da filha da célula apagada, o que facilita o balanceamento.
    if p_page.borrow().leaf == 0 {
        rc = btree_previous(p_cur, 0);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    // Salva as posições dos outros cursores abertos nesta tabela antes de alterar.
    if p_cur.cur_flags & BTCF_MULTIPLE != 0 {
        rc = save_all_cursors(&p_bt, p_cur.pgno_root, Some(&*p_cur));
        if rc != SQLITE_OK {
            return rc;
        }
    }

    // Apagando linha de b-tree de tabela: invalida cursores incrblob da linha.
    if p_cur.p_key_info.is_none() && p.borrow().has_incrblob_cur != 0 {
        invalidate_incrblob_cursors(&p, p_cur.pgno_root, p_cur.info.n_key, 0);
    }

    // Torna gravável a página da entrada, libera as páginas de overflow dela e
    // remove a célula de dentro da página.
    rc = pager_write(&p_page.borrow().p_db_page);
    if rc != SQLITE_OK {
        return rc;
    }
    rc = btree_clear_cell(&p_page, p_cell, &mut info);
    drop_cell(&p_page, i_cell_idx, info.n_size as i32, &mut rc);
    if rc != SQLITE_OK {
        return rc;
    }

    // Se a célula apagada não estava numa folha, o cursor aponta para a maior
    // entrada da subárvore da filha da célula apagada. A célula da folha é movida
    // para o nó interno no lugar da apagada.
    if p_page.borrow().leaf == 0 {
        let p_leaf = p_cur.p_page.clone().unwrap();

        if p_leaf.borrow().n_free < 0 {
            rc = btree_compute_free_space(&p_leaf);
            if rc != SQLITE_OK {
                return rc;
            }
        }
        let n: Pgno = if i_cell_depth < p_cur.i_page as i32 - 1 {
            p_cur.ap_page[(i_cell_depth + 1) as usize].as_ref().unwrap().borrow().pgno
        } else {
            p_cur.p_page.as_ref().unwrap().borrow().pgno
        };
        let last = p_leaf.borrow().n_cell as usize - 1;
        p_cell = find_cell(&p_leaf.borrow(), last);
        if p_cell < 4 {
            return sqlite_corrupt_page(&p_leaf);
        }
        let n_cell = mem_page_cell_size(&p_leaf, &p_leaf.borrow().a_data, p_cell) as i32;
        let cell_bytes: Vec<u8> =
            p_leaf.borrow().a_data[p_cell - 4..p_cell + n_cell as usize].to_vec();
        let mut p_tmp = std::mem::take(&mut p_bt.borrow_mut().p_tmp_space);
        rc = pager_write(&p_leaf.borrow().p_db_page);
        if rc == SQLITE_OK {
            rc = insert_cell(&p_page, i_cell_idx, &cell_bytes, n_cell + 4, &mut p_tmp, n);
        }
        p_bt.borrow_mut().p_tmp_space = p_tmp;
        drop_cell(&p_leaf, last as i32, n_cell, &mut rc);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    // Balanceia a árvore. Se a entrada apagada estava numa folha, o cursor ainda
    // aponta para ela, a primeira chamada de balance() conserta a árvore e o if(...)
    // seguinte nunca é verdadeiro.
    //
    // Se estava num nó interno, o cursor aponta para a folha de onde saiu a célula
    // de reposição. A folha pode ficar com espaço sobrando e o nó interno pode ficar
    // cheio ou vazio demais: balanceia a folha primeiro e, se o balanceamento não
    // subiu o bastante, sobe o cursor até o nó interno e o balanceia também.
    let usable = p_cur.p_bt.borrow().usable_size as i32;
    if p_cur.p_page.as_ref().unwrap().borrow().n_free as i32 * 3 <= usable * 2 {
        // Otimização: com espaço livre menor que 2/3 da página, balance() não faria nada.
        rc = SQLITE_OK;
    } else {
        rc = balance(p_cur);
    }
    if rc == SQLITE_OK && p_cur.i_page as i32 > i_cell_depth {
        release_page(p_cur.p_page.as_ref());
        p_cur.i_page -= 1;
        while p_cur.i_page as i32 > i_cell_depth {
            let pg = p_cur.ap_page[p_cur.i_page as usize].clone();
            release_page(pg.as_ref());
            p_cur.i_page -= 1;
        }
        p_cur.p_page = p_cur.ap_page[p_cur.i_page as usize].clone();
        rc = balance(p_cur);
    }

    if rc == SQLITE_OK {
        if b_preserve > 1 {
            p_cur.e_state = CURSOR_SKIPNEXT;
            let n_cell = p_page.borrow().n_cell;
            if i_cell_idx >= n_cell as i32 {
                p_cur.skip_next = -1;
                p_cur.ix = n_cell.wrapping_sub(1);
            } else {
                p_cur.skip_next = 1;
            }
        } else {
            rc = move_to_root(p_cur);
            if b_preserve != 0 {
                btree_release_all_cursor_pages(p_cur);
                p_cur.e_state = CURSOR_REQUIRESEEK;
            }
            if rc == SQLITE_EMPTY {
                rc = SQLITE_OK;
            }
        }
    }
    rc
}

/// Cria uma nova tabela b-tree e grava em `*pi_table` o número da página raiz.
///
/// O tipo é determinado por `create_tab_flags`. Só estes valores estão em uso:
///
///     BTREE_INTKEY|BTREE_LEAFDATA     tabelas SQL com chave rowid
///     BTREE_ZERODATA                  índices SQL
fn btree_create_table_inner(p: &BtreeRef, pi_table: &mut Pgno, create_tab_flags: i32) -> i32 {
    let p_bt = p.borrow().p_bt.clone();
    let mut p_root: Option<MemPageRef> = None;
    let mut pgno_root: Pgno = 0;
    let mut rc: i32;

    if p_bt.borrow().auto_vacuum != 0 {
        let mut pgno_move: Pgno = 0; // página movida para abrir lugar para a raiz
        let mut p_page_move: Option<MemPageRef> = None;

        // Criar uma tabela pode exigir mover uma página existente para dar lugar à
        // raiz. Caso ela seja uma página de overflow, apaga todos os caches de
        // overflow dos cursores abertos.
        invalidate_all_overflow_cache(&p_bt);

        // Lê meta[3] do banco para saber onde vai a raiz da nova tabela: meta[3] é a
        // maior raiz criada até agora, então a nova é (meta[3]+1).
        btree_get_meta(p, BTREE_LARGEST_ROOT_PAGE as i32, &mut pgno_root);
        if pgno_root > btree_pagecount(&p_bt) {
            return sqlite_corrupt_pgno(pgno_root);
        }
        pgno_root += 1;

        // A nova raiz não pode cair numa página do mapa de ponteiros nem na
        // página do PENDING_BYTE.
        while pgno_root == ptrmap_pageno(&p_bt, pgno_root) || pgno_root == pending_byte_page(&p_bt) {
            pgno_root += 1;
        }

        // Aloca uma página. A que reside em pgno_root será movida para a alocada
        // (a menos que a alocada seja a própria pgno_root).
        rc = allocate_btree_page(&p_bt, &mut p_page_move, &mut pgno_move, pgno_root, BTALLOC_EXACT);
        if rc != SQLITE_OK {
            return rc;
        }

        if pgno_move != pgno_root {
            // pgno_root vai ser a raiz da nova tabela, mas foi alocada pgno_move. Se
            // não veio de extensão do arquivo, a página atual em pgno_move já está
            // no journal.
            let mut e_type: u8 = 0;
            let mut i_ptr_page: Pgno = 0;

            // Salva as posições dos cursores abertos, caso guardem uma referência
            // xFetch da página pgno_root.
            rc = save_all_cursors(&p_bt, 0, None);
            release_page(p_page_move.as_ref());
            if rc != SQLITE_OK {
                return rc;
            }

            // Move a página que está em pgno_root para pgno_move.
            rc = btree_get_page(&p_bt, pgno_root, &mut p_root, 0);
            if rc != SQLITE_OK {
                return rc;
            }
            rc = ptrmap_get(&p_bt, pgno_root, &mut e_type, &mut i_ptr_page);
            if e_type == PTRMAP_ROOTPAGE || e_type == PTRMAP_FREEPAGE {
                rc = sqlite_corrupt_pgno(pgno_root);
            }
            if rc != SQLITE_OK {
                release_page(p_root.as_ref());
                return rc;
            }
            rc = relocate_page(&p_bt, p_root.as_ref().unwrap(), e_type, i_ptr_page, pgno_move, 0);
            release_page(p_root.as_ref());

            // Obtém a página em pgno_root.
            if rc != SQLITE_OK {
                return rc;
            }
            p_root = None;
            rc = btree_get_page(&p_bt, pgno_root, &mut p_root, 0);
            if rc != SQLITE_OK {
                return rc;
            }
            rc = pager_write(&p_root.as_ref().unwrap().borrow().p_db_page);
            if rc != SQLITE_OK {
                release_page(p_root.as_ref());
                return rc;
            }
        } else {
            p_root = p_page_move;
        }

        // Atualiza o mapa de ponteiros e os metadados com o número da nova raiz.
        ptrmap_put(&p_bt, pgno_root, PTRMAP_ROOTPAGE, 0, &mut rc);
        if rc != SQLITE_OK {
            release_page(p_root.as_ref());
            return rc;
        }

        // Ao alocar a nova raiz, a página 1 foi tornada gravável (para aumentar o
        // arquivo ou decrementar a contagem da freelist), então btree_update_meta
        // não pode falhar.
        rc = btree_update_meta(p, 4, pgno_root);
        if rc != SQLITE_OK {
            release_page(p_root.as_ref());
            return rc;
        }
    } else {
        rc = allocate_btree_page(&p_bt, &mut p_root, &mut pgno_root, 1, 0);
        if rc != SQLITE_OK {
            return rc;
        }
    }
    let p_root = p_root.unwrap();
    let ptf_flags: u8 = if create_tab_flags & BTREE_INTKEY != 0 {
        PTF_INTKEY | PTF_LEAFDATA | PTF_LEAF
    } else {
        PTF_ZERODATA | PTF_LEAF
    };
    zero_page(&p_root, ptf_flags);
    pager_unref(&p_root.borrow().p_db_page);
    *pi_table = pgno_root;
    SQLITE_OK
}


// ---- part_023.rs ----

// Notas da tradução deste trecho:
//  - Os `assert!` do C (mutex_held, in_trans, ...) somem: o Debian compila com NDEBUG.
//  - `sqlite3BtreeCreateTable` e o `btreeCreateTable` estático (parte 022) colidem no
//    mesmo módulo. O estático é chamado aqui como `btree_create_table_inner`, e o
//    `btreeDropTable` estático é `btree_drop_table_inner`.
//  - `BTREE_CLEAR_CELL(rc, pPage, pCell, info)` é a função `btree_clear_cell`, que
//    devolve o `rc` (o ponteiro de célula do C é o deslocamento `usize` em `a_data`).
//  - Os Btrees e páginas são passados pelos apelidos `BtreeRef`, `BtSharedRef`, `MemPageRef`.

/// Cria uma nova tabela ou índice b-tree com o mutex do Btree adquirido.
pub fn btree_create_table(p: &BtreeRef, pi_table: &mut Pgno, flags: i32) -> i32 {
    btree_enter(&mut p.borrow_mut());
    let rc = btree_create_table_inner(p, pi_table, flags);
    btree_leave(&mut p.borrow_mut());
    rc
}

/// Apaga a página de banco de dados dada e todos os seus filhos. Devolve
/// a página para a lista livre.
fn clear_database_page(
    p_bt: &BtSharedRef,         // O BTree que contém a tabela
    pgno: Pgno,                 // Número da página a limpar
    free_page_flag: i32,        // Desaloca a página se verdadeiro
    mut pn_change: Option<&mut i64>, // Soma o número de células liberadas a este contador
) -> i32 {
    let mut info = CellInfo::default();

    if pgno > btree_pagecount(p_bt) {
        return sqlite_corrupt_pgno(pgno);
    }
    let mut pp_page: Option<MemPageRef> = None;
    let mut rc = get_and_init_page(p_bt, pgno, &mut pp_page, 0);
    if rc != SQLITE_OK {
        return rc;
    }
    let p_page: MemPageRef = pp_page.unwrap();

    'cleardatabasepage_out: {
        let open_flags = p_bt.borrow().open_flags;
        let refcount = pager_page_refcount(&p_page.borrow().p_db_page);
        if (open_flags as u32 & BTREE_SINGLE) == 0 && refcount != (1 + (pgno == 1) as i64) {
            rc = sqlite_corrupt_page(&p_page);
            break 'cleardatabasepage_out;
        }
        let hdr = p_page.borrow().hdr_offset as usize;
        let mut i: usize = 0;
        while i < p_page.borrow().n_cell as usize {
            let p_cell = find_cell(&p_page.borrow(), i);
            if p_page.borrow().leaf == 0 {
                let child = get4byte(&p_page.borrow().a_data, p_cell);
                rc = clear_database_page(p_bt, child, 1, pn_change.as_deref_mut());
                if rc != SQLITE_OK {
                    break 'cleardatabasepage_out;
                }
            }
            rc = btree_clear_cell(&p_page, p_cell, &mut info);
            if rc != SQLITE_OK {
                break 'cleardatabasepage_out;
            }
            i += 1;
        }
        if p_page.borrow().leaf == 0 {
            let child = get4byte(&p_page.borrow().a_data, hdr + 8);
            rc = clear_database_page(p_bt, child, 1, pn_change.as_deref_mut());
            if rc != SQLITE_OK {
                break 'cleardatabasepage_out;
            }
            if p_page.borrow().int_key != 0 {
                pn_change = None;
            }
        }
        if let Some(n) = pn_change {
            // testcase( !pPage->intKey );
            *n = n.wrapping_add(p_page.borrow().n_cell as i64);
        }
        if free_page_flag != 0 {
            free_page(&p_page, &mut rc);
        } else {
            let db_page = p_page.borrow().p_db_page.clone().unwrap();
            rc = pager_write(&db_page);
            if rc == SQLITE_OK {
                let flags = p_page.borrow().a_data[hdr] | PTF_LEAF;
                zero_page(&mut p_page.borrow_mut(), flags);
            }
        }
    }
    release_page(Some(&p_page));
    rc
}

/// Apaga toda a informação de uma única tabela do banco de dados. `i_table` é
/// o número de página da raiz da tabela. Depois que esta rotina retorna, a
/// página raiz está vazia, mas ainda existe.
///
/// Esta rotina falha com SQLITE_LOCKED se houver cursores de leitura abertos
/// na tabela. Cursores de escrita abertos são movidos para a raiz da tabela.
///
/// Se `pn_change` não for None, o inteiro apontado é incrementado pelo número
/// de entradas da tabela.
pub fn btree_clear_table(p: &BtreeRef, i_table: i32, pn_change: Option<&mut i64>) -> i32 {
    let p_bt = p.borrow().p_bt.clone().unwrap();
    btree_enter(&mut p.borrow_mut());

    let mut rc = save_all_cursors(&p_bt, i_table as Pgno, None);

    if SQLITE_OK == rc {
        // Invalida todos os cursores incrblob abertos na tabela iTable (supondo que
        // iTable é a raiz de uma b-tree de tabela; se não for, a chamada a seguir
        // não faz nada).
        if p.borrow().has_incrblob_cur != 0 {
            invalidate_incrblob_cursors(p,i_table as Pgno, 0, 1);
        }
        rc = clear_database_page(&p_bt, i_table as Pgno, 0, pn_change);
    }
    btree_leave(&mut p.borrow_mut());
    rc
}

/// Apaga toda a informação da única tabela em que `p_cur` está aberto.
///
/// Esta rotina só funciona para `p_cur` numa tabela efêmera.
pub fn btree_clear_table_of_cursor(p_cur: &BtCursor) -> i32 {
    btree_clear_table(p_cur.p_btree.as_ref().unwrap(), p_cur.pgno_root as i32, None)
}

/// Apaga toda a informação de uma tabela e adiciona a raiz da tabela à lista
/// livre. Exceto que a raiz da tabela principal (a da página 1) nunca é
/// adicionada à lista livre.
///
/// Esta rotina falha com SQLITE_LOCKED se houver cursores abertos na tabela.
///
/// Se AUTOVACUUM estiver habilitado e a página em `i_table` não for a última
/// página raiz do arquivo, a última página raiz é movida para o lugar antes
/// ocupado por `i_table`, e o espaço antes ocupado pela última página raiz vai
/// para a lista livre no lugar de `i_table`. Assim todas as páginas raiz ficam
/// no início do arquivo, o que é necessário para o AUTOVACUUM funcionar.
/// `*pi_moved` recebe o número de página que era a última raiz antes da
/// mudança. Se nenhuma página for movida, `*pi_moved` recebe 0. A última página
/// raiz é gravada em meta[3] e o valor de meta[3] é atualizado aqui.
fn btree_drop_table_inner(p: &BtreeRef, i_table: Pgno, pi_moved: &mut i32) -> i32 {
    let p_bt = p.borrow().p_bt.clone().unwrap();

    if i_table > btree_pagecount(&p_bt) {
        return sqlite_corrupt_pgno(i_table);
    }

    let mut rc = btree_clear_table(p, i_table as i32, None);
    if rc != SQLITE_OK {
        return rc;
    }
    let mut pp_page: Option<MemPageRef> = None;
    rc = btree_get_page(&p_bt, i_table, &mut pp_page, 0);
    if rc != SQLITE_OK {
        // NEVER(rc)
        release_page(pp_page.as_ref());
        return rc;
    }
    let p_page: MemPageRef = pp_page.unwrap();

    *pi_moved = 0;

    if p_bt.borrow().auto_vacuum != 0 {
        let mut max_root_pgno: Pgno = 0;
        btree_get_meta(p, BTREE_LARGEST_ROOT_PAGE as i32, &mut max_root_pgno);

        if i_table == max_root_pgno {
            // Se a tabela removida é a de maior número de página raiz do banco
            // de dados, coloca a página raiz na lista livre.
            free_page(&p_page, &mut rc);
            release_page(Some(&p_page));
            if rc != SQLITE_OK {
                return rc;
            }
        } else {
            // A tabela removida não tem o maior número de página raiz do banco
            // de dados. Então move a página que tem para o buraco deixado pela
            // página raiz removida.
            release_page(Some(&p_page));
            let mut pp_move: Option<MemPageRef> = None;
            rc = btree_get_page(&p_bt, max_root_pgno, &mut pp_move, 0);
            if rc != SQLITE_OK {
                return rc;
            }
            let p_move: MemPageRef = pp_move.unwrap();
            rc = relocate_page(&p_bt, &p_move, PTRMAP_ROOTPAGE, 0, i_table, 0);
            release_page(Some(&p_move));
            if rc != SQLITE_OK {
                return rc;
            }
            let mut pp_move: Option<MemPageRef> = None;
            rc = btree_get_page(&p_bt, max_root_pgno, &mut pp_move, 0);
            if let Some(p_move) = pp_move.as_ref() {
                free_page(p_move, &mut rc);
            }
            release_page(pp_move.as_ref());
            if rc != SQLITE_OK {
                return rc;
            }
            *pi_moved = max_root_pgno as i32;
        }

        // Define o novo valor 'max-root-page' no cabeçalho do banco de dados. É o
        // valor antigo menos um, menos mais um se esse for um número de página
        // raiz, menos mais um se for a PENDING_BYTE_PAGE.
        max_root_pgno = max_root_pgno.wrapping_sub(1);
        while max_root_pgno == pending_byte_page(&p_bt)
            || ptrmap_ispage(&p_bt, max_root_pgno)
        {
            max_root_pgno = max_root_pgno.wrapping_sub(1);
        }

        rc = btree_update_meta(p, 4, max_root_pgno);
    } else {
        free_page(&p_page, &mut rc);
        release_page(Some(&p_page));
    }
    rc
}

pub fn btree_drop_table(p: &BtreeRef, i_table: i32, pi_moved: &mut i32) -> i32 {
    btree_enter(&mut p.borrow_mut());
    let rc = btree_drop_table_inner(p, i_table as Pgno, pi_moved);
    btree_leave(&mut p.borrow_mut());
    rc
}

/// Esta função só pode ser chamada se a conexão b-tree já tem uma transação de
/// leitura ou escrita aberta no banco de dados.
///
/// Lê a meta-informação de um arquivo de banco de dados. Meta[0] é o número de
/// páginas livres no banco de dados. Meta[1] a meta[15] ficam disponíveis para
/// as camadas superiores. Meta[0] é somente leitura, os outros são de
/// leitura e escrita.
///
/// A camada do esquema numera os valores meta de outro jeito. Nela (e nos
/// opcodes SetCookie e ReadCookie) o número de páginas livres não é visível,
/// então Cookie[0] é o mesmo que Meta[1].
///
/// Esta rotina trata Meta[BTREE_DATA_VERSION] como caso especial. Em vez de
/// ler o valor do cabeçalho, carrega o "DataVersion" do paginador. O valor
/// BTREE_DATA_VERSION não é gravado no arquivo do banco de dados: é um número
/// calculado pelo paginador. Mas o padrão de acesso é o dos valores meta do
/// cabeçalho, então é cômodo lê-lo por esta rotina.
pub fn btree_get_meta(p: &BtreeRef, idx: i32, p_meta: &mut u32) {
    let p_bt = p.borrow().p_bt.clone().unwrap();

    btree_enter(&mut p.borrow_mut());

    if idx == BTREE_DATA_VERSION as i32 {
        let i_b_data_version = p.borrow().i_b_data_version;
        *p_meta = pager_data_version(p_bt.borrow().p_pager.as_ref().unwrap())
            .wrapping_add(i_b_data_version);
    } else {
        let p_page1 = p_bt.borrow().p_page_1.clone().unwrap();
        *p_meta = get4byte(&p_page1.borrow().a_data, 36 + idx as usize * 4);
    }

    btree_leave(&mut p.borrow_mut());
}

/// Grava a meta-informação de volta no banco de dados. Meta[0] é somente
/// leitura e não pode ser escrito.
pub fn btree_update_meta(p: &BtreeRef, idx: i32, i_meta: u32) -> i32 {
    let p_bt = p.borrow().p_bt.clone().unwrap();
    btree_enter(&mut p.borrow_mut());
    let p_page1 = p_bt.borrow().p_page_1.clone().unwrap();
    let db_page = p_page1.borrow().p_db_page.clone().unwrap();
    let rc = pager_write(&db_page);
    if rc == SQLITE_OK {
        put4byte(&mut p_page1.borrow_mut().a_data, 36 + idx as usize * 4, i_meta);
        if idx == BTREE_INCR_VACUUM as i32 {
            p_bt.borrow_mut().incr_vacuum = i_meta as u8;
        }
    }
    btree_leave(&mut p.borrow_mut());
    rc
}

/// O primeiro argumento, `p_cur`, é um cursor aberto em alguma b-tree. Conta o
/// número de entradas da b-tree e grava o resultado em `*pn_entry`.
///
/// Devolve SQLITE_OK se a operação for bem sucedida. Senão, se ocorrer um erro
/// (erro de E/S ou corrupção do banco de dados), devolve um código de erro do
/// SQLite.
pub fn btree_count(db: &Sqlite3, p_cur: &mut BtCursor, pn_entry: &mut i64) -> i32 {
    let mut n_entry: i64 = 0; // Valor a devolver em *pnEntry

    let mut rc = move_to_root(p_cur);
    if rc == SQLITE_EMPTY {
        *pn_entry = 0;
        return SQLITE_OK;
    }

    // Salvo se ocorrer um erro, o laço a seguir roda uma iteração por página da
    // B-Tree (sem contar as páginas de overflow).
    while rc == SQLITE_OK && db.is_interrupted == 0 {
        // Se esta é uma página folha ou a árvore não é int-key, esta página
        // contém entradas contáveis. Incrementa o contador de acordo.
        let mut p_page: MemPageRef = p_cur.p_page.clone().unwrap();
        if p_page.borrow().leaf != 0 || p_page.borrow().int_key == 0 {
            n_entry = n_entry.wrapping_add(p_page.borrow().n_cell as i64);
        }

        // pPage é um nó folha. Este laço navega o cursor para que aponte para a
        // primeira célula interior que aponta para o pai da próxima página da
        // árvore ainda não visitada. O valor pCur->aiIdx[pCur->iPage] recebe o
        // índice da célula pai da página, ou o número de células da página se a
        // próxima página a visitar é o filho direito do pai.
        //
        // Se todas as páginas da árvore foram visitadas, devolve SQLITE_OK ao
        // chamador.
        if p_page.borrow().leaf != 0 {
            loop {
                if p_cur.i_page == 0 {
                    // Todas as páginas da b-tree foram visitadas. Retorna com sucesso.
                    *pn_entry = n_entry;
                    return move_to_root(p_cur);
                }
                move_to_parent(p_cur);
                if p_cur.ix < p_cur.p_page.as_ref().unwrap().borrow().n_cell {
                    break;
                }
            }

            p_cur.ix = p_cur.ix.wrapping_add(1);
            p_page = p_cur.p_page.clone().unwrap();
        }

        // Desce para o nó filho da célula para a qual o cursor aponta agora. É
        // o filho direito se (iIdx==pPage->nCell).
        let i_idx = p_cur.ix;
        if i_idx == p_page.borrow().n_cell {
            let hdr = p_page.borrow().hdr_offset as usize;
            let child = get4byte(&p_page.borrow().a_data, hdr + 8);
            rc = move_to_child(p_cur, child);
        } else {
            let p_cell = find_cell(&p_page.borrow(), i_idx as usize);
            let child = get4byte(&p_page.borrow().a_data, p_cell);
            rc = move_to_child(p_cur, child);
        }
    }

    // Ocorreu um erro. Devolve o código de erro.
    rc
}


// ---- part_024.rs ----

use std::sync::atomic::Ordering;

/// Retorna o pager associado a um Btree. Usada só em testes e depuração.
pub fn btree_pager(p: &Btree) -> PagerRef {
    p.p_bt.borrow().p_pager.clone()
}

/// Registra um erro de OOM durante o integrity_check.
fn check_oom(p_check: &mut IntegrityCk) {
    p_check.rc = SQLITE_NOMEM;
    p_check.mx_err = 0; /* Faz o processamento do integrity_check parar */
    if p_check.n_err == 0 {
        p_check.n_err += 1;
    }
}

/// Invoca o manipulador de progresso, se apropriado. Também verifica se
/// houve interrupção.
fn check_progress(p_check: &mut IntegrityCk) {
    let db = p_check.db.clone();
    let (interrupted, x_progress, n_progress_ops) = {
        let d = db.borrow();
        (
            d.u1.is_interrupted.load(Ordering::SeqCst),
            d.x_progress.clone(),
            d.n_progress_ops,
        )
    };
    if interrupted != 0 {
        p_check.rc = SQLITE_INTERRUPT;
        p_check.n_err += 1;
        p_check.mx_err = 0;
    }
    if let Some(x_progress) = x_progress {
        debug_assert!(n_progress_ops > 0);
        p_check.n_step = p_check.n_step.wrapping_add(1);
        if ((p_check.n_step as u32) % (n_progress_ops as u32)) == 0 && x_progress() != 0 {
            p_check.rc = SQLITE_INTERRUPT;
            p_check.n_err += 1;
            p_check.mx_err = 0;
        }
    }
}

/// Expande o prefixo de mensagem (formato com %u) usando v0, v1 e v2, na
/// ordem em que o printf do C consumiria os argumentos.
fn check_format_prefix(z_pfx: &str, v0: u32, v1: u32, v2: i32) -> String {
    let args = [v0, v1, v2 as u32];
    let mut out = String::new();
    let mut next = 0usize;
    let mut rest = z_pfx;
    while let Some(pos) = rest.find("%u") {
        out.push_str(&rest[..pos]);
        out.push_str(&args[next].to_string());
        next += 1;
        rest = &rest[pos + 2..];
    }
    out.push_str(rest);
    out
}

/// Anexa uma mensagem à string de mensagens de erro. O texto chega já
/// formatado (os argumentos do printf do C são expandidos no chamador).
fn check_append_msg(p_check: &mut IntegrityCk, msg: &str) {
    check_progress(p_check);
    if p_check.mx_err == 0 {
        return;
    }
    p_check.mx_err -= 1;
    p_check.n_err += 1;
    if p_check.err_msg.n_char != 0 {
        api::str_append(&mut p_check.err_msg, b"\n");
    }
    if let Some(z_pfx) = p_check.z_pfx {
        let pfx = check_format_prefix(z_pfx, p_check.v0, p_check.v1, p_check.v2);
        api::str_append(&mut p_check.err_msg, pfx.as_bytes());
    }
    api::str_append(&mut p_check.err_msg, msg.as_bytes());
    if p_check.err_msg.acc_error == SQLITE_NOMEM {
        check_oom(p_check);
    }
}

/// Retorna não zero se o bit em IntegrityCk.aPgRef[] que corresponde à
/// página iPg já está ligado.
fn get_page_referenced(p_check: &IntegrityCk, i_pg: u32) -> i32 {
    debug_assert!(!p_check.a_pg_ref.is_empty());
    debug_assert!(i_pg <= p_check.n_ck_page);
    (p_check.a_pg_ref[(i_pg / 8) as usize] & (1u8 << (i_pg & 0x07))) as i32
}

/// Liga o bit em IntegrityCk.aPgRef[] que corresponde à página iPg.
fn set_page_referenced(p_check: &mut IntegrityCk, i_pg: u32) {
    debug_assert!(!p_check.a_pg_ref.is_empty());
    debug_assert!(i_pg <= p_check.n_ck_page);
    p_check.a_pg_ref[(i_pg / 8) as usize] |= 1u8 << (i_pg & 0x07);
}

/// Soma 1 à contagem de referências da página iPage. Se esta for a segunda
/// referência à página, acrescenta uma mensagem de erro. Retorna 1 se há 2
/// ou mais referências e 0 se esta é a primeira. Também confere que o
/// número da página está dentro dos limites.
fn check_ref(p_check: &mut IntegrityCk, i_page: u32) -> i32 {
    if i_page > p_check.n_ck_page || i_page == 0 {
        check_append_msg(p_check, &format!("invalid page number {}", i_page));
        return 1;
    }
    if get_page_referenced(p_check, i_page) != 0 {
        check_append_msg(p_check, &format!("2nd reference to page {}", i_page));
        return 1;
    }
    set_page_referenced(p_check, i_page);
    0
}

/// Confere que a entrada do mapa de ponteiros para a página iChild aponta
/// para a página iParent, com tipo eType. Se não, acrescenta uma mensagem
/// de erro.
fn check_ptrmap(p_check: &mut IntegrityCk, i_child: u32, e_type: u8, i_parent: u32) {
    let mut e_ptrmap_type: u8 = 0;
    let mut i_ptrmap_parent: u32 = 0;

    let rc = ptrmap_get(&p_check.p_bt, i_child, &mut e_ptrmap_type, &mut i_ptrmap_parent);
    if rc != SQLITE_OK {
        if rc == SQLITE_NOMEM || rc == SQLITE_IOERR_NOMEM {
            check_oom(p_check);
        }
        check_append_msg(p_check, &format!("Failed to read ptrmap key={}", i_child));
        return;
    }

    if e_ptrmap_type != e_type || i_ptrmap_parent != i_parent {
        check_append_msg(
            p_check,
            &format!(
                "Bad ptr map entry key={} expected=({},{}) got=({},{})",
                i_child, e_type, i_parent, e_ptrmap_type, i_ptrmap_parent
            ),
        );
    }
}

/// Confere a integridade da freelist ou de uma lista de páginas de
/// overflow. Verifica que o número de páginas na lista é N.
fn check_list(p_check: &mut IntegrityCk, is_free_list: i32, mut i_page: u32, mut n: u32) {
    let expected = n;
    let n_err_at_start = p_check.n_err;
    while i_page != 0 && p_check.mx_err != 0 {
        if check_ref(p_check, i_page) != 0 {
            break;
        }
        n = n.wrapping_sub(1);
        let mut p_ovfl_page: Option<PgHdrRef> = None;
        if pager_get(&p_check.p_pager, i_page, &mut p_ovfl_page, 0) != 0 {
            check_append_msg(p_check, &format!("failed to get page {}", i_page));
            break;
        }
        let p_ovfl_page = p_ovfl_page.unwrap();
        let p_ovfl_data = pager_get_data(&p_ovfl_page);
        let (auto_vacuum, usable_size) = {
            let bt = p_check.p_bt.borrow();
            (bt.auto_vacuum, bt.usable_size)
        };
        if is_free_list != 0 {
            let n_leaf = get4byte(&p_ovfl_data, 4);
            if auto_vacuum != 0 {
                check_ptrmap(p_check, i_page, PTRMAP_FREEPAGE, 0);
            }
            if n_leaf > usable_size / 4 - 2 {
                check_append_msg(
                    p_check,
                    &format!("freelist leaf count too big on page {}", i_page),
                );
                n = n.wrapping_sub(1);
            } else {
                for i in 0..(n_leaf as usize) {
                    let i_free_page = get4byte(&p_ovfl_data, 8 + i * 4);
                    if auto_vacuum != 0 {
                        check_ptrmap(p_check, i_free_page, PTRMAP_FREEPAGE, 0);
                    }
                    check_ref(p_check, i_free_page);
                }
                n = n.wrapping_sub(n_leaf);
            }
        } else {
            /* Se este banco suporta auto-vacuum e iPage não é a última página
            ** desta lista de overflow, confere que a entrada do mapa de
            ** ponteiros da página seguinte aponta para iPage. */
            if auto_vacuum != 0 && n > 0 {
                let i = get4byte(&p_ovfl_data, 0);
                check_ptrmap(p_check, i, PTRMAP_OVERFLOW2, i_page);
            }
        }
        i_page = get4byte(&p_ovfl_data, 0);
        pager_unref(&p_ovfl_page);
    }
    if n != 0 && n_err_at_start == p_check.n_err {
        check_append_msg(
            p_check,
            &format!(
                "{} is {} but should be {}",
                if is_free_list != 0 { "size" } else { "overflow list length" },
                expected.wrapping_sub(n),
                expected
            ),
        );
    }
}

/// Min-heap. aHeap[0] é o número de elementos; aHeap[1] é a raiz; os
/// filhos de aHeap[N] são aHeap[N*2] e aHeap[N*2+1]. Cada nó é menor ou
/// igual aos seus filhos, então a raiz é sempre o mínimo. Usado para testar
/// sobreposição e cobertura de células: cada entrada u32 é o intervalo de
/// uma célula ou freeblock da página (16 bits altos: primeiro byte; 16
/// bits baixos: último byte).
fn btree_heap_insert(a_heap: &mut [u32], mut x: u32) {
    a_heap[0] += 1;
    let mut i = a_heap[0] as usize;
    a_heap[i] = x;
    loop {
        let j = i / 2;
        if j == 0 || a_heap[j] <= a_heap[i] {
            break;
        }
        x = a_heap[j];
        a_heap[j] = a_heap[i];
        a_heap[i] = x;
        i = j;
    }
}

/// Remove a raiz do heap (o mínimo) e a escreve em *pOut. Retorna 0 se o
/// heap está vazio.
fn btree_heap_pull(a_heap: &mut [u32], p_out: &mut u32) -> i32 {
    let x = a_heap[0] as usize;
    if x == 0 {
        return 0;
    }
    *p_out = a_heap[1];
    a_heap[1] = a_heap[x];
    a_heap[x] = 0xffffffff;
    a_heap[0] -= 1;
    let mut i = 1usize;
    loop {
        let mut j = i * 2;
        if j > a_heap[0] as usize {
            break;
        }
        if a_heap[j] > a_heap[j + 1] {
            j += 1;
        }
        if a_heap[i] < a_heap[j] {
            break;
        }
        a_heap.swap(i, j);
        i = j;
    }
    1
}

/// Faz várias verificações de sanidade em uma página de uma árvore e
/// retorna a profundidade da árvore. Páginas raiz retornam 0, pais de
/// raízes retornam 1 e assim por diante.
///
/// 1. Células e freeblocks não se sobrepõem e juntos cobrem a página.
/// 2. Chaves inteiras das células estão em ordem.
/// 3. Integridade das páginas de overflow.
/// 4. Chamada recursiva em todos os filhos.
/// 5. A profundidade de todos os filhos é a mesma.
fn check_tree_page(
    p_check: &mut IntegrityCk,
    i_page: u32,
    pi_min_key: &mut i64,
    mut max_key: i64,
) -> i32 {
    let mut p_page: Option<MemPageRef> = None; /* A página analisada */
    let mut depth: i32 = -1; /* Profundidade de uma subárvore */
    let mut do_coverage_check = true; /* Verdadeiro se a cobertura deve ser checada */
    let mut key_can_be_equal = true; /* Verdadeiro se a IPK pode ser igual a maxKey */
    let saved_z_pfx = p_check.z_pfx;
    let saved_v1 = p_check.v1;
    let saved_v2 = p_check.v2;
    let mut saved_is_init: u8 = 0;

    /* Confere que a página existe */
    check_progress(p_check);
    'end_of_check: {
        if p_check.mx_err == 0 {
            break 'end_of_check;
        }
        let p_bt = p_check.p_bt.clone();
        let usable_size: u32 = p_bt.borrow().usable_size;
        if i_page == 0 {
            return 0;
        }
        if check_ref(p_check, i_page) != 0 {
            return 0;
        }
        p_check.z_pfx = Some("Tree %u page %u: ");
        p_check.v1 = i_page;
        let rc = btree_get_page(&p_bt, i_page, &mut p_page, 0);
        if rc != 0 {
            check_append_msg(p_check, &format!("unable to get the page. error code={}", rc));
            if rc == SQLITE_IOERR_NOMEM {
                p_check.rc = SQLITE_NOMEM;
            }
            break 'end_of_check;
        }
        let page_ref = p_page.clone().unwrap();

        /* Zera MemPage.isInit para garantir que a detecção de corrupção em
        ** btreeInitPage() seja executada. */
        {
            let mut pg = page_ref.borrow_mut();
            saved_is_init = pg.is_init;
            pg.is_init = 0;
        }
        let rc = btree_init_page(&mut page_ref.borrow_mut());
        if rc != 0 {
            debug_assert!(rc == SQLITE_CORRUPT); /* O único erro possível do InitPage */
            check_append_msg(p_check, &format!("btreeInitPage() returns error code {}", rc));
            break 'end_of_check;
        }
        let rc = btree_compute_free_space(&mut page_ref.borrow_mut());
        if rc != 0 {
            debug_assert!(rc == SQLITE_CORRUPT);
            check_append_msg(p_check, "free space corruption");
            break 'end_of_check;
        }
        let page = page_ref.borrow();
        let data: &[u8] = &page.a_data;
        let hdr = page.hdr_offset as usize;
        let leaf = page.leaf != 0;

        /* Prepara a análise das células */
        p_check.z_pfx = Some("Tree %u page %u cell %u: ");
        let content_offset: u32 = get2byte_not_zero(data, hdr + 5);
        debug_assert!(content_offset <= usable_size); /* Garantido por btreeInitPage() */

        /* EVIDENCE-OF: R-37002-32774 O inteiro de dois bytes no deslocamento 3
        ** dá o número de células da página. */
        let n_cell = get2byte(data, hdr + 3) as i32;
        debug_assert!(page.n_cell as i32 == n_cell);
        if leaf || page.int_key == 0 {
            p_check.n_row += n_cell as i64;
        }

        /* EVIDENCE-OF: R-23882-45353 O vetor de ponteiros de célula de uma
        ** página btree segue imediatamente o cabeçalho da página. */
        let cell_start = hdr + 12 - 4 * (leaf as usize);

        if !leaf {
            /* Analisa a página filha direita das páginas internas */
            let pgno = get4byte(data, hdr + 8);
            if p_bt.borrow().auto_vacuum != 0 {
                p_check.z_pfx = Some("Tree %u page %u right child: ");
                check_ptrmap(p_check, pgno, PTRMAP_BTREE, i_page);
            }
            let mk = max_key;
            depth = check_tree_page(p_check, pgno, &mut max_key, mk);
            key_can_be_equal = false;
        } else {
            /* Em folhas a verificação de cobertura ocorre no mesmo laço das
            ** outras verificações de célula, então inicializa o heap. */
            p_check.heap[0] = 0;
        }

        /* EVIDENCE-OF: R-02776-14802 O vetor de ponteiros de célula consiste
        ** em K deslocamentos de 2 bytes para o conteúdo das células. */
        let mut i = n_cell - 1;
        while i >= 0 && p_check.mx_err != 0 {
            let mut info = CellInfo::default();

            /* Confere o tamanho da célula */
            p_check.v2 = i;
            let pc: u32 = get2byte_aligned(data, cell_start + (i as usize) * 2);
            i -= 1;
            if pc < content_offset || pc > usable_size - 4 {
                check_append_msg(
                    p_check,
                    &format!("Offset {} out of range {}..{}", pc, content_offset, usable_size - 4),
                );
                do_coverage_check = false;
                continue;
            }
            let p_cell: &[u8] = &data[pc as usize..];
            (page.x_parse_cell)(&*page, p_cell, &mut info);
            if pc + info.n_size as u32 > usable_size {
                check_append_msg(p_check, "Extends off end of page");
                do_coverage_check = false;
                continue;
            }

            /* Confere se a chave primária inteira está fora do intervalo */
            if page.int_key != 0 {
                let out_of_order = if key_can_be_equal {
                    info.n_key > max_key
                } else {
                    info.n_key >= max_key
                };
                if out_of_order {
                    check_append_msg(p_check, &format!("Rowid {} out of order", info.n_key));
                }
                max_key = info.n_key;
                key_can_be_equal = false; /* Só a primeira chave da página pode ser == maxKey */
            }

            /* Confere a lista de overflow do conteúdo */
            if info.n_payload > info.n_local as u32 {
                debug_assert!(pc + info.n_size as u32 - 4 <= usable_size);
                let n_page: u32 =
                    (info.n_payload - info.n_local as u32 + usable_size - 5) / (usable_size - 4);
                let pgno_ovfl: u32 = get4byte(p_cell, info.n_size as usize - 4);
                if p_bt.borrow().auto_vacuum != 0 {
                    check_ptrmap(p_check, pgno_ovfl, PTRMAP_OVERFLOW1, i_page);
                }
                check_list(p_check, 0, pgno_ovfl, n_page);
            }

            if !leaf {
                /* Confere a sanidade da página filha esquerda das internas */
                let pgno = get4byte(p_cell, 0);
                if p_bt.borrow().auto_vacuum != 0 {
                    check_ptrmap(p_check, pgno, PTRMAP_BTREE, i_page);
                }
                let mk = max_key;
                let d2 = check_tree_page(p_check, pgno, &mut max_key, mk);
                key_can_be_equal = false;
                if d2 != depth {
                    check_append_msg(p_check, "Child page depth differs");
                    depth = d2;
                }
            } else {
                /* Popula o heap de cobertura das folhas */
                btree_heap_insert(
                    &mut p_check.heap,
                    (pc << 16) | (pc + info.n_size as u32 - 1),
                );
            }
        }
        *pi_min_key = max_key;

        /* Confere a cobertura completa da página */
        p_check.z_pfx = None;
        if do_coverage_check && p_check.mx_err > 0 {
            /* Nas folhas o min-heap já foi inicializado e as células já foram
            ** inseridas. Nas internas isso ainda não foi feito, então faz agora */
            if !leaf {
                p_check.heap[0] = 0;
                let mut i = n_cell - 1;
                while i >= 0 {
                    let pc: u32 = get2byte_aligned(data, cell_start + (i as usize) * 2);
                    let size: u32 = (page.x_cell_size)(&*page, &data[pc as usize..]) as u32;
                    btree_heap_insert(&mut p_check.heap, (pc << 16) | (pc + size - 1));
                    i -= 1;
                }
            }
            /* Acrescenta os freeblocks ao min-heap
            **
            ** EVIDENCE-OF: R-20690-50594 O segundo campo do cabeçalho da página
            ** btree é o deslocamento do primeiro freeblock, ou zero se não há. */
            let mut i: i32 = get2byte(data, hdr + 1) as i32;
            while i > 0 {
                debug_assert!((i as u32) <= usable_size - 4); /* Garantido por btreeComputeFreeSpace() */
                let size: i32 = get2byte(data, i as usize + 2) as i32;
                debug_assert!((i + size) as u32 <= usable_size);
                btree_heap_insert(&mut p_check.heap, ((i as u32) << 16) | ((i + size - 1) as u32));
                /* EVIDENCE-OF: R-58208-19414 Os 2 primeiros bytes de um freeblock
                ** são o deslocamento do próximo freeblock, ou zero se é o último. */
                let j: i32 = get2byte(data, i as usize) as i32;
                /* EVIDENCE-OF: R-06866-39125 Freeblocks são sempre encadeados em
                ** ordem crescente de deslocamento. */
                debug_assert!(j == 0 || j > i + size);
                debug_assert!((j as u32) <= usable_size - 4);
                i = j;
            }
            /* Analisa o min-heap procurando sobreposição entre células e/ou
            ** freeblocks, e contando em nFrag os bytes não rastreados.
            **
            ** Cada entrada é (endereço_inicial<<16)|endereço_final. Há uma
            ** primeira entrada implícita que cobre o cabeçalho da página, o
            ** vetor de ponteiros e o espaço até o início do conteúdo.
            **
            ** O laço tira as entradas em ordem e compara o início com o fim
            ** anterior. Sobreposição significa bytes usados mais de uma vez;
            ** lacuna é somada à contagem de fragmentação. */
            let mut n_frag: i32 = 0;
            let mut prev: u32 = content_offset.wrapping_sub(1); /* Primeira entrada implícita */
            let mut x: u32 = 0;
            while btree_heap_pull(&mut p_check.heap, &mut x) != 0 {
                if (prev & 0xffff) >= (x >> 16) {
                    check_append_msg(
                        p_check,
                        &format!("Multiple uses for byte {} of page {}", x >> 16, i_page),
                    );
                    break;
                } else {
                    n_frag = n_frag.wrapping_add(
                        (x >> 16).wrapping_sub(prev & 0xffff).wrapping_sub(1) as i32,
                    );
                    prev = x;
                }
            }
            n_frag = n_frag
                .wrapping_add(usable_size.wrapping_sub(prev & 0xffff).wrapping_sub(1) as i32);
            /* EVIDENCE-OF: R-43263-13491 O total de bytes em todos os fragmentos
            ** fica no quinto campo do cabeçalho da página btree.
            ** EVIDENCE-OF: R-07161-27322 O inteiro de um byte no deslocamento 7
            ** dá o número de bytes livres fragmentados na área de conteúdo. */
            if p_check.heap[0] == 0 && n_frag != data[hdr + 7] as i32 {
                check_append_msg(
                    p_check,
                    &format!(
                        "Fragmentation of {} bytes reported as {} on page {}",
                        n_frag as u32, data[hdr + 7], i_page
                    ),
                );
            }
        }
    }

    /* end_of_check */
    if !do_coverage_check {
        if let Some(pg) = &p_page {
            pg.borrow_mut().is_init = saved_is_init;
        }
    }
    release_page(p_page.as_ref());
    p_check.z_pfx = saved_z_pfx;
    p_check.v1 = saved_v1;
    p_check.v2 = saved_v2;
    depth + 1
}


// ---- part_025.rs ----

/// Faz uma verificação completa do arquivo BTree dado. aRoot[] é um vetor
/// de números de página, cada um a página raiz de uma tabela. nRoot é o
/// número de entradas de aRoot.
///
/// Uma transação de leitura ou de leitura e escrita precisa estar aberta
/// antes de chamar esta função.
///
/// Escreve o número de erros vistos em *pnErr. Exceto por alguns erros de
/// alocação, uma mensagem de erro é devolvida em *pzOut se *pnErr for
/// diferente de zero; se *pnErr==0, *pzOut recebe None.
///
/// Se a primeira entrada de aRoot[] for 0, a lista de páginas raiz está
/// incompleta: é uma "verificação de integridade parcial" (ocorre ao
/// verificar uma única tabela). O zero é pulado e as verificações da
/// freelist e de que toda página é referenciada também são puladas, já que
/// não há como saber quais páginas as btrees não verificadas cobrem.
/// Exceto se aRoot[1] for 1: aí a freelist ainda é verificada.
pub fn btree_integrity_check(
    db: &SqliteRef,
    p: &BtreeRef,
    a_root: &[u32],
    a_cnt: &mut [Mem],
    n_root: i32,
    mx_err: i32,
    pn_err: &mut i32,
    pz_out: &mut Option<Vec<u8>>,
) -> i32 {
    let p_bt: BtSharedRef = p.borrow().p_bt.clone();
    let saved_db_flags: u64 = p_bt.borrow().db.borrow().flags;
    let mut b_partial = false; /* Verdadeiro se não checa todas as btrees */
    let mut b_ck_freelist = true; /* Verdadeiro para varrer a freelist */

    debug_assert!(n_root > 0);
    debug_assert!(!a_cnt.is_empty());

    /* aRoot[0]==0 significa verificação parcial */
    if a_root[0] == 0 {
        debug_assert!(n_root > 1);
        b_partial = true;
        if a_root[1] != 1 {
            b_ck_freelist = false;
        }
    }

    btree_enter(p);
    debug_assert!(p.borrow().in_trans > TRANS_NONE && p_bt.borrow().in_transaction > TRANS_NONE);
    let (p_pager, n_ck_page, page_size) = {
        let bt = p_bt.borrow();
        (bt.p_pager.clone(), btree_pagecount(&bt), bt.page_size)
    };
    let mut s_check = IntegrityCk {
        p_bt: p_bt.clone(),
        p_pager,
        db: db.clone(),
        a_pg_ref: Vec::new(),
        n_ck_page,
        mx_err,
        n_err: 0,
        rc: SQLITE_OK,
        n_step: 0,
        z_pfx: None,
        v0: 0,
        v1: 0,
        v2: 0,
        err_msg: StrAccum::default(),
        heap: Vec::new(),
        n_row: 0,
    };
    str_accum_init(&mut s_check.err_msg, None, 100, SQLITE_MAX_LENGTH);
    s_check.err_msg.printf_flags = SQLITE_PRINTF_INTERNAL;

    'integrity_ck_cleanup: {
        if s_check.n_ck_page == 0 {
            break 'integrity_ck_cleanup;
        }

        /* As alocações abaixo não falham em Rust, então os ramos de OOM do C
        ** (checkOom + goto integrity_ck_cleanup) não têm como ocorrer. */
        s_check.a_pg_ref = vec![0u8; (s_check.n_ck_page / 8) as usize + 1];
        s_check.heap = vec![0u32; (page_size / 4) as usize];

        let i = pending_byte_page(&p_bt.borrow());
        if i <= s_check.n_ck_page {
            set_page_referenced(&mut s_check, i);
        }

        /* Confere a integridade da freelist */
        let page1_get4byte = |off: usize| -> u32 {
            let bt = p_bt.borrow();
            let page1 = bt.p_page1.as_ref().unwrap().borrow();
            get4byte(&page1.a_data, off)
        };
        if b_ck_freelist {
            s_check.z_pfx = Some("Freelist: ");
            check_list(&mut s_check, 1, page1_get4byte(32), page1_get4byte(36));
            s_check.z_pfx = None;
        }

        /* Confere todas as tabelas */
        if !b_partial {
            if p_bt.borrow().auto_vacuum != 0 {
                let mut mx: u32 = 0;
                for i in 0..(n_root as usize) {
                    if mx < a_root[i] {
                        mx = a_root[i];
                    }
                }
                let mx_in_hdr = page1_get4byte(52);
                if mx != mx_in_hdr {
                    check_append_msg(
                        &mut s_check,
                        &format!("max rootpage ({}) disagrees with header ({})", mx, mx_in_hdr),
                    );
                }
            } else if page1_get4byte(64) != 0 {
                check_append_msg(
                    &mut s_check,
                    "incremental_vacuum enabled with a max rootpage of zero",
                );
            }
        }
        p_bt.borrow().db.borrow_mut().flags &= !(SQLITE_CELLSIZECK as u64);
        for i in 0..(n_root as usize) {
            if s_check.mx_err == 0 {
                break;
            }
            s_check.n_row = 0;
            if a_root[i] != 0 {
                let mut not_used: i64 = 0;
                if p_bt.borrow().auto_vacuum != 0 && a_root[i] > 1 && !b_partial {
                    check_ptrmap(&mut s_check, a_root[i], PTRMAP_ROOTPAGE, 0);
                }
                s_check.v0 = a_root[i];
                check_tree_page(&mut s_check, a_root[i], &mut not_used, LARGEST_INT64);
            }
            mem_set_array_int64(a_cnt, i as i32, s_check.n_row);
        }
        p_bt.borrow().db.borrow_mut().flags = saved_db_flags;

        /* Garante que toda página do arquivo é referenciada */
        if !b_partial {
            let mut i: u32 = 1;
            while i <= s_check.n_ck_page && s_check.mx_err != 0 {
                /* Se o banco usa auto-vacuum, garante que nenhuma tabela
                ** referencia páginas do mapa de ponteiros. */
                let (is_ptrmap_page, auto_vacuum) = {
                    let bt = p_bt.borrow();
                    (ptrmap_pageno(&bt, i) == i, bt.auto_vacuum != 0)
                };
                if get_page_referenced(&s_check, i) == 0 && (!is_ptrmap_page || !auto_vacuum) {
                    check_append_msg(&mut s_check, &format!("Page {}: never used", i));
                }
                if get_page_referenced(&s_check, i) != 0 && (is_ptrmap_page && auto_vacuum) {
                    check_append_msg(&mut s_check, &format!("Page {}: pointer map referenced", i));
                }
                i += 1;
            }
        }
    }

    /* integrity_ck_cleanup: limpa e reporta os erros */
    s_check.heap = Vec::new();
    s_check.a_pg_ref = Vec::new();
    *pn_err = s_check.n_err;
    if s_check.n_err == 0 {
        api::str_reset(&mut s_check.err_msg);
        *pz_out = None;
    } else {
        *pz_out = str_accum_finish(&mut s_check.err_msg);
    }
    btree_leave(p);
    s_check.rc
}

/// Retorna o caminho completo do arquivo de banco subjacente. Retorna uma
/// string vazia se o banco está em memória ou é TEMP.
///
/// O nome do arquivo do pager é invariante enquanto o pager está aberto,
/// então é seguro acessá-lo sem o mutex do BtShared.
pub fn btree_get_filename(p: &BtreeRef) -> Vec<u8> {
    let p_pager = p.borrow().p_bt.borrow().p_pager.clone();
    pager_filename(&p_pager, 1)
}

/// Retorna o caminho do arquivo de journal deste banco. O valor é o mesmo
/// tenha o journal sido criado ou não.
///
/// O nome do journal do pager é invariante enquanto o pager está aberto,
/// então é seguro acessá-lo sem o mutex do BtShared.
pub fn btree_get_journalname(p: &BtreeRef) -> Vec<u8> {
    let p_pager = p.borrow().p_bt.borrow().p_pager.clone();
    pager_journalname(&p_pager)
}

/// Retorna SQLITE_TXN_NONE, SQLITE_TXN_READ ou SQLITE_TXN_WRITE para
/// descrever o estado de transação atual do Btree p.
pub fn btree_txn_state(p: Option<&BtreeRef>) -> i32 {
    match p {
        Some(p) => p.borrow().in_trans as i32,
        None => 0,
    }
}

/// Executa um checkpoint no Btree passado como primeiro argumento.
///
/// Retorna SQLITE_LOCKED se esta ou qualquer outra conexão tem uma
/// transação aberta no cache compartilhado ao qual o Btree está ligado.
///
/// O parâmetro eMode é um de SQLITE_CHECKPOINT_PASSIVE, FULL ou RESTART.
pub fn btree_checkpoint(
    p: Option<&BtreeRef>,
    e_mode: i32,
    pn_log: Option<&mut i32>,
    pn_ckpt: Option<&mut i32>,
) -> i32 {
    let mut rc = SQLITE_OK;
    if let Some(p) = p {
        let p_bt: BtSharedRef = p.borrow().p_bt.clone();
        btree_enter(p);
        if p_bt.borrow().in_transaction != TRANS_NONE {
            rc = SQLITE_LOCKED;
        } else {
            let p_pager = p_bt.borrow().p_pager.clone();
            let db = p.borrow().db.clone();
            rc = pager_checkpoint(&p_pager, &db, e_mode, pn_log, pn_ckpt);
        }
        btree_leave(p);
    }
    rc
}

/// Retorna verdadeiro se há um backup em execução no Btree p.
pub fn btree_is_in_backup(p: &BtreeRef) -> i32 {
    (p.borrow().n_backup != 0) as i32
}

/// Devolve o blob de memória associado a uma única btree compartilhada. A
/// memória é usada pelo código cliente para os seus fins (por exemplo, o
/// esquema de alto nível associado à btree). A camada btree cuida da
/// contagem de referências.
///
/// Na primeira chamada em uma btree compartilhada o blob é alocado e
/// zerado (nBytes só serve para decidir se aloca); nas seguintes nBytes é
/// ignorado e o mesmo blob é devolvido.
///
/// Se nBytes for 0 e o blob ainda não existir, devolve None. Se já existir,
/// devolve-o normalmente.
///
/// Pouco antes de a btree compartilhada ser fechada, a função xFree
/// passada na alocação é invocada sobre o blob. A xFree não deve liberar a
/// memória, a camada btree faz isso.
pub fn btree_schema(
    p: &BtreeRef,
    n_bytes: i32,
    x_free: Option<fn(&SchemaRef)>,
) -> Option<SchemaRef> {
    let p_bt: BtSharedRef = p.borrow().p_bt.clone();
    btree_enter(p);
    if p_bt.borrow().p_schema.is_none() && n_bytes != 0 {
        let mut bt = p_bt.borrow_mut();
        bt.p_schema = Some(std::rc::Rc::new(std::cell::RefCell::new(Schema::default())));
        bt.x_free_schema = x_free;
    }
    btree_leave(p);
    let schema = p_bt.borrow().p_schema.clone();
    schema
}

/// Retorna SQLITE_LOCKED_SHAREDCACHE se outro usuário da mesma btree
/// compartilhada do handle dado mantém um bloqueio exclusivo na tabela
/// sqlite_schema. Caso contrário SQLITE_OK.
pub fn btree_schema_locked(p: &BtreeRef) -> i32 {
    btree_enter(p);
    let rc = query_shared_cache_table_lock(p, SCHEMA_ROOT, READ_LOCK);
    debug_assert!(rc == SQLITE_OK || rc == SQLITE_LOCKED_SHAREDCACHE);
    btree_leave(p);
    rc
}

/// Obtém um bloqueio na tabela cuja página raiz é iTab. O bloqueio é de
/// escrita se isWriteLock for verdadeiro e de leitura se for falso.
pub fn btree_lock_table(p: &BtreeRef, i_tab: i32, is_write_lock: u8) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(p.borrow().in_trans != TRANS_NONE);
    if p.borrow().sharable != 0 {
        let lock_type: u8 = READ_LOCK + is_write_lock;
        debug_assert!(READ_LOCK + 1 == WRITE_LOCK);
        debug_assert!(is_write_lock == 0 || is_write_lock == 1);

        btree_enter(p);
        rc = query_shared_cache_table_lock(p, i_tab as u32, lock_type);
        if rc == SQLITE_OK {
            rc = set_shared_cache_table_lock(p, i_tab as u32, lock_type);
        }
        btree_leave(p);
    }
    rc
}

/// O cursor pCsr precisa estar aberto para escrita em uma tabela INTKEY,
/// apontando para uma entrada válida. Esta função modifica os dados
/// armazenados nessa entrada.
///
/// Só o conteúdo pode ser modificado, não o comprimento. Se chamada com
/// parâmetros que escrevem além do fim dos dados existentes, nada é
/// modificado e SQLITE_CORRUPT é devolvido.
pub fn btree_put_data(p_csr: &mut BtCursor, offset: u32, amt: u32, z: &mut [u8]) -> i32 {
    debug_assert!(p_csr.cur_flags & BTCF_INCRBLOB != 0);

    let mut rc = restore_cursor_position(p_csr);
    if rc != SQLITE_OK {
        return rc;
    }
    debug_assert!(p_csr.e_state != CURSOR_REQUIRESEEK);
    if p_csr.e_state != CURSOR_VALID {
        return SQLITE_ABORT;
    }

    /* Salva as posições de todos os outros cursores abertos nesta tabela.
    ** Necessário caso algum deles guarde referências a uma versão xFetch da
    ** página modificada pelo accessPayload abaixo.
    **
    ** pCsr está aberto em uma tabela INTKEY, e saveCursorPosition() e logo
    ** saveAllCursors() não falham em BTREE_INTKEY: só devolvem SQLITE_OK. */
    let p_bt: BtSharedRef = p_csr.p_bt.clone();
    rc = save_all_cursors(&p_bt, p_csr.pgno_root, Some(&*p_csr));
    debug_assert!(rc == SQLITE_OK);

    /* Confere algumas suposições: (a) o cursor está aberto para escrita,
    ** (b) há transação de leitura e escrita aberta, (c) a conexão tem o
    ** bloqueio de escrita da tabela (se necessário), (d) não há bloqueios de
    ** leitura conflitantes e (e) o cursor aponta para uma linha válida de
    ** uma tabela intKey. */
    if (p_csr.cur_flags & BTCF_WRITEFLAG) == 0 {
        return SQLITE_READONLY;
    }
    debug_assert!(
        (p_bt.borrow().bts_flags & BTS_READ_ONLY) == 0
            && p_bt.borrow().in_transaction == TRANS_WRITE
    );

    access_payload(p_csr, offset, amt, z, 1)
}


// ---- part_026.rs ----

/// Marca este cursor como um cursor de blob incremental.
pub fn btree_incrblob_cursor(p_cur: &mut BtCursor) {
    p_cur.cur_flags |= BTCF_INCRBLOB;
    p_cur.p_btree.borrow_mut().has_incrblob_cur = 1;
}

/// Define os campos "versão de leitura" (byte no deslocamento 18) e
/// "versão de escrita" (byte no deslocamento 19) do cabeçalho do banco de dados
/// como i_version.
pub fn btree_set_version(p_btree: &BtreeRef, i_version: i32) -> i32 {
    let p_bt: BtSharedRef = p_btree.borrow().p_bt.clone();
    let mut rc: i32; // Código de retorno

    debug_assert!(i_version == 1 || i_version == 2);

    // Se os campos de versão forem definidos como 1, não abre automaticamente a
    // conexão WAL, mesmo que os campos de versão estejam atualmente em 2.
    {
        let mut bt = p_bt.borrow_mut();
        bt.bts_flags &= !BTS_NO_WAL;
        if i_version == 1 {
            bt.bts_flags |= BTS_NO_WAL;
        }
    }

    rc = btree_begin_trans(p_btree, 0, None);
    if rc == SQLITE_OK {
        let p_page1 = p_bt.borrow().p_page1.clone().unwrap();
        let differs = {
            let page = p_page1.borrow();
            page.a_data[18] != (i_version as u8) || page.a_data[19] != (i_version as u8)
        };
        if differs {
            rc = btree_begin_trans(p_btree, 2, None);
            if rc == SQLITE_OK {
                let p_db_page = p_page1.borrow().p_db_page.clone();
                rc = pager_write(&p_db_page);
                if rc == SQLITE_OK {
                    let mut page = p_page1.borrow_mut();
                    page.a_data[18] = i_version as u8;
                    page.a_data[19] = i_version as u8;
                }
            }
        }
    }

    p_bt.borrow_mut().bts_flags &= !BTS_NO_WAL;
    rc
}

/// Retorna verdadeiro se o cursor tem uma dica especificada. Esta rotina só é
/// usada dentro de instruções assert().
pub fn btree_cursor_has_hint(p_csr: &BtCursor, mask: u32) -> i32 {
    ((p_csr.hints as u32 & mask) != 0) as i32
}

/// Retorna verdadeiro se o Btree dado é somente leitura.
pub fn btree_is_readonly(p: &BtreeRef) -> i32 {
    let p_bt: BtSharedRef = p.borrow().p_bt.clone();
    let flags = p_bt.borrow().bts_flags;
    ((flags & BTS_READ_ONLY) != 0) as i32
}

/// Retorna o tamanho do cabeçalho acrescentado a cada página por este módulo:
/// ROUND8(sizeof(MemPage)). No layout de 64 bits do C, MemPage ocupa 136
/// bytes, já múltiplo de 8.
pub fn header_size_btree() -> i32 {
    136
}

/// Se nenhuma transação está ativa e o banco de dados não é temporário,
/// limpa o cache de páginas do paginador em memória.
pub fn btree_clear_cache(p: &BtreeRef) {
    let p_bt: BtSharedRef = p.borrow().p_bt.clone();
    let (in_transaction, p_pager) = {
        let bt = p_bt.borrow();
        (bt.in_transaction, bt.p_pager.clone())
    };
    if in_transaction == TRANS_NONE {
        pager_clear_cache(&p_pager);
    }
}

/// Retorna verdadeiro se o Btree passado como único argumento é compartilhável.
pub fn btree_sharable(p: &Btree) -> i32 {
    p.sharable as i32
}

/// Retorna o número de conexões ao objeto BtShared acessado pelo Btree passado
/// como único argumento. Para caches privados é sempre 1. Para caches
/// compartilhados pode ser 1 ou mais.
pub fn btree_connection_count(p: &Btree) -> i32 {
    p.p_bt.borrow().n_ref
}

