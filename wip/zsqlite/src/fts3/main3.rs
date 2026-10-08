//! `fts3.c` (parte 3): a avaliação da consulta MATCH (`fts3Eval*`): carga das doclists, doclists
//! incrementais, tokens adiados, cálculo de NEAR, avanço pelas linhas, estatísticas de frases e
//! listas de posições (`sqlite3Fts3EvalPhraseStats`, `sqlite3Fts3EvalPhrasePoslist`).
//!
//! # Desvios do C, decorrentes do modelo v2
//!
//! * **A árvore sai do cursor.** Os nós da expressão são uma arena ([`Fts3ExprTree`]) e a
//!   avaliação precisa do cursor e da árvore ao mesmo tempo: [`Eval`] reúne a conexão, a tabela, o
//!   cursor e a árvore (que o ponto de entrada retira do cursor com `Option::take` e devolve).
//! * **Doclist de frase.** `Fts3Doclist.pList` é um deslocamento em `a_all` ou, quando
//!   `b_free_list` (a lista é um buffer próprio), em `a_list`; a convenção deste porte é que
//!   `b_free_list` vale sempre que a lista mora em `a_list`, inclusive a que
//!   `sqlite3Fts3MsrIncrNext` devolve por cópia.
//! * **Listas de posições entre frases** (`char *aPoslist` do NEAR): a lista de uma frase é
//!   copiada (`n_list` bytes) quando outra frase a lê, e a edição no lugar de
//!   `fts3EvalNearTrim` é feita no buffer da própria frase.
//! * **`sqlite3Fts3EvalPhrasePoslist`** devolve uma cópia da lista de posições do documento
//!   (do ponto pedido até o `0x00` final, e zero depois), e não um ponteiro.
//! * **Sem `aTmp`** (o espaço de trabalho de `fts3EvalNearTest`): o `Vec` temporário é local.

use crate::connection::Connection;
use crate::consts::SQLITE_OK;
use crate::vdbeapi::{column_blob, column_bytes, reset};

use super::int::{
    ExprId, Fts3Cursor, Fts3Doclist, Fts3ExprTree, Fts3Phrase, Fts3Table, FTSQUERY_AND,
    FTSQUERY_NEAR, FTSQUERY_NOT, FTSQUERY_OR, FTSQUERY_PHRASE, FTS_CORRUPT_VTAB,
};
use super::main2::{
    docid_cmp, fts3_cursor_seek, fts3_doclist_count_docids, fts3_doclist_next,
    fts3_doclist_phrase_merge, fts3_poslist_near_merge, fts3_poslist_phrase_merge,
    fts3_seg_reader_cursor_free, fts3_term_seg_reader_cursor, fts3_term_select, reset_opt,
};
use super::snippet::fts3_expr_iterate;
use super::varint::{fts3_get_varint, fts3_get_varint32, fts3_get_varint_bounded};
use super::write::{
    fts3_cache_deferred_doclists, fts3_columnlist_copy, fts3_defer_token,
    fts3_deferred_token_list, fts3_doclist_prev, fts3_free_deferred_doclists, fts3_msr_incr_next,
    fts3_msr_incr_restart, fts3_msr_incr_start, fts3_msr_ovfl, fts3_poslist_copy,
    fts3_select_doctotal, sl,
};
use crate::util::at;

/// `MAX_INCR_PHRASE_TOKENS`.
const MAX_INCR_PHRASE_TOKENS: usize = 4;

impl Fts3Doclist {
    /// O buffer onde mora a lista de posições corrente.
    fn list_buf(&self) -> &[u8] {
        if self.b_free_list {
            &self.a_list
        } else {
            &self.a_all
        }
    }

    /// O buffer (mutável) onde mora a lista de posições corrente.
    fn list_buf_mut(&mut self) -> &mut Vec<u8> {
        if self.b_free_list {
            &mut self.a_list
        } else {
            &mut self.a_all
        }
    }

    /// A lista de posições corrente do `p_list` até o fim do buffer.
    pub fn list_tail(&self) -> &[u8] {
        match self.p_list {
            Some(o) => sl(self.list_buf(), o),
            None => &[],
        }
    }

    /// Uma cópia dos `n_list` bytes da lista de posições corrente.
    pub fn list_bytes(&self) -> Vec<u8> {
        let t = self.list_tail();
        let n = (self.n_list.max(0) as usize).min(t.len());
        t[..n].to_vec()
    }

    /// Faz de `list` a lista de posições corrente (um buffer próprio).
    fn set_own_list(&mut self, list: Vec<u8>, n_list: i32) {
        self.a_list = list;
        self.p_list = Some(0);
        self.n_list = n_list;
        self.b_free_list = true;
    }
}

/// O nó de frase `id`.
fn phrase_mut(tree: &mut Fts3ExprTree, id: ExprId) -> &mut Fts3Phrase {
    tree[id].p_phrase.as_mut().expect("fts3: nó FTSQUERY_PHRASE sem frase")
}

/// `fts3EvalInvalidatePoslist`.
pub fn fts3_eval_invalidate_poslist(p_phrase: &mut Fts3Phrase) {
    let dl = &mut p_phrase.doclist;
    if dl.b_free_list {
        dl.a_list = Vec::new();
    }
    dl.p_list = None;
    dl.n_list = 0;
    dl.b_free_list = false;
}

/// `sqlite3Fts3EvalPhraseCleanup`: solta a doclist da frase e os leitores de segmentos dos tokens.
pub fn eval_phrase_cleanup(db: &mut Connection, p_phrase: &mut Fts3Phrase) {
    p_phrase.doclist.a_all = Vec::new();
    fts3_eval_invalidate_poslist(p_phrase);
    p_phrase.doclist = Fts3Doclist::default();
    for tok in p_phrase.a_token.iter_mut() {
        if let Some(seg) = tok.p_segcsr.take() {
            fts3_seg_reader_cursor_free(db, seg);
        }
    }
}

/// `fts3EvalPhraseMergeToken`: mescla a doclist `p_list` do token `i_token` na doclist da frase.
fn fts3_eval_phrase_merge_token(
    b_desc_idx: bool,
    p: &mut Fts3Phrase,
    i_token: i32,
    p_list: Option<Vec<u8>>,
) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(i_token != p.i_doclist_token);

    match p_list {
        None => p.doclist.a_all = Vec::new(),
        Some(list) if p.i_doclist_token < 0 => p.doclist.a_all = list,
        Some(_) if p.doclist.a_all.is_empty() => {}
        Some(list) => {
            let cur = std::mem::take(&mut p.doclist.a_all);
            let (p_left, mut p_right, n_diff) = if p.i_doclist_token < i_token {
                (cur, list, i_token - p.i_doclist_token)
            } else {
                (list, cur, p.i_doclist_token - i_token)
            };
            rc = fts3_doclist_phrase_merge(b_desc_idx, n_diff, &p_left, &mut p_right);
            p.doclist.a_all = p_right;
        }
    }

    if i_token > p.i_doclist_token {
        p.i_doclist_token = i_token;
    }
    rc
}

/// `fts3EvalDeferredPhrase`: calcula a lista de posições da frase para a linha corrente a partir
/// dos tokens adiados.
fn fts3_eval_deferred_phrase(p_csr: &Fts3Cursor, p_phrase: &mut Fts3Phrase) -> i32 {
    let mut a_poslist: Option<Vec<u8>> = None; /* lista de posições dos tokens adiados */
    let mut i_prev: i32 = -1; /* o token adiado anterior */

    for i_token in 0..p_phrase.a_token.len() {
        if let Some(di) = p_phrase.a_token[i_token].p_deferred {
            let list = match fts3_deferred_token_list(&p_csr.p_deferred[di]) {
                Ok(l) => l,
                Err(rc) => return rc,
            };

            match list {
                None => {
                    if p_phrase.doclist.b_free_list {
                        p_phrase.doclist.a_list = Vec::new();
                    }
                    p_phrase.doclist.p_list = None;
                    p_phrase.doclist.n_list = 0;
                    return SQLITE_OK;
                }
                Some(l) => match a_poslist.take() {
                    None => a_poslist = Some(l),
                    Some(prev) => {
                        debug_assert!(i_prev >= 0);
                        let mut out: Vec<u8> = Vec::new();
                        let (mut p1, mut p2) = (0usize, 0usize);
                        fts3_poslist_phrase_merge(
                            &mut out,
                            i_token as i32 - i_prev,
                            false,
                            true,
                            &prev,
                            &mut p1,
                            &l,
                            &mut p2,
                        );
                        if out.is_empty() {
                            if p_phrase.doclist.b_free_list {
                                p_phrase.doclist.a_list = Vec::new();
                            }
                            p_phrase.doclist.p_list = None;
                            p_phrase.doclist.n_list = 0;
                            return SQLITE_OK;
                        }
                        a_poslist = Some(out);
                    }
                },
            }
            i_prev = i_token as i32;
        }
    }

    if i_prev >= 0 {
        let a_poslist = a_poslist.unwrap_or_default();
        let n_poslist = a_poslist.len() as i32;
        let n_max_undeferred = p_phrase.i_doclist_token;
        if n_max_undeferred < 0 {
            p_phrase.doclist.set_own_list(a_poslist, n_poslist);
            p_phrase.doclist.i_docid = p_csr.i_prev_id;
        } else {
            let undeferred = p_phrase.doclist.list_tail().to_vec();
            let (b1, b2, n_distance): (&[u8], &[u8], i32) = if n_max_undeferred > i_prev {
                (&a_poslist, &undeferred, n_max_undeferred - i_prev)
            } else {
                (&undeferred, &a_poslist, i_prev - n_max_undeferred)
            };
            let mut out: Vec<u8> = Vec::new();
            let (mut p1, mut p2) = (0usize, 0usize);
            if fts3_poslist_phrase_merge(&mut out, n_distance, false, true, b1, &mut p1, b2, &mut p2) {
                let n = out.len() as i32;
                p_phrase.doclist.set_own_list(out, n);
            } else {
                if p_phrase.doclist.b_free_list {
                    p_phrase.doclist.a_list = Vec::new();
                }
                p_phrase.doclist.p_list = None;
                p_phrase.doclist.n_list = 0;
            }
        }
    }
    SQLITE_OK
}

/// `fts3EvalDlPhraseNext`.
fn fts3_eval_dl_phrase_next(b_desc_idx: bool, p_dl: &mut Fts3Doclist, pb_eof: &mut bool) {
    let end = p_dl.a_all.len();
    let i_iter = p_dl.p_next_docid.unwrap_or(0);

    if p_dl.a_all.is_empty() || i_iter >= end {
        /* Já se chegou ao fim desta doclist. EOF. */
        *pb_eof = true;
    } else {
        let mut it = i_iter;
        let (n, i_delta) = fts3_get_varint(sl(&p_dl.a_all, it));
        it += n as usize;
        if !b_desc_idx || p_dl.p_next_docid.is_none() {
            p_dl.i_docid = p_dl.i_docid.wrapping_add(i_delta);
        } else {
            p_dl.i_docid = p_dl.i_docid.wrapping_sub(i_delta);
        }
        p_dl.b_free_list = false;
        p_dl.p_list = Some(it);
        fts3_poslist_copy(None, &p_dl.a_all, &mut it);
        p_dl.n_list = (it - p_dl.p_list.unwrap_or(0)) as i32;

        /* `it` aponta logo depois do 0x00 que fecha a lista de posições do docid. Se a lista foi
        ** editada no lugar por `fts3EvalNearTrim`, ele pode não apontar o próximo docid: pula o
        ** enchimento de zeros que a edição deixou. */
        while it < end && at(&p_dl.a_all, it) == 0 {
            it += 1;
        }
        p_dl.p_next_docid = Some(it);
        *pb_eof = false;
    }
}

/// `TokenDoclist`.
#[derive(Default, Clone)]
struct TokenDoclist {
    b_ignore: bool,
    i_docid: i64,
    /// `pList`/`nList`: a lista de posições (`None` é o `NULL`).
    list: Option<Vec<u8>>,
}

/// `incrPhraseTokenNext`.
fn incr_phrase_token_next(
    p_tab: &Fts3Table,
    p_phrase: &mut Fts3Phrase,
    i_token: usize,
    p: &mut TokenDoclist,
    pb_eof: &mut bool,
) -> i32 {
    let mut rc = SQLITE_OK;

    if p_phrase.i_doclist_token == i_token as i32 {
        debug_assert!(!p.b_ignore);
        fts3_eval_dl_phrase_next(p_tab.b_desc_idx, &mut p_phrase.doclist, pb_eof);
        p.list = Some(p_phrase.doclist.list_bytes());
        p.i_docid = p_phrase.doclist.i_docid;
    } else {
        let p_token = &mut p_phrase.a_token[i_token];
        debug_assert!(p_token.p_deferred.is_none());
        if let Some(seg) = p_token.p_segcsr.as_mut() {
            debug_assert!(!p.b_ignore);
            match fts3_msr_incr_next(p_tab, seg) {
                Ok(Some((i_docid, list))) => {
                    p.i_docid = i_docid;
                    p.list = Some(list);
                }
                Ok(None) => {
                    p.list = None;
                    *pb_eof = true;
                }
                Err(e) => {
                    rc = e;
                    p.list = None;
                    *pb_eof = true;
                }
            }
        } else {
            p.b_ignore = true;
        }
    }
    rc
}

/// `fts3EvalIncrPhraseNext`.
fn fts3_eval_incr_phrase_next(
    b_desc: bool,
    p_tab: &Fts3Table,
    p: &mut Fts3Phrase,
    pb_eof: &mut bool,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut b_eof = false;
    debug_assert!(p.b_incr);

    if p.n_token() == 1 {
        let res = match p.a_token[0].p_segcsr.as_mut() {
            Some(seg) => fts3_msr_incr_next(p_tab, seg),
            None => Ok(None),
        };
        match res {
            Ok(Some((i_docid, list))) => {
                let n = list.len() as i32;
                p.doclist.i_docid = i_docid;
                p.doclist.set_own_list(list, n);
            }
            Ok(None) => {
                p.doclist.p_list = None;
                p.doclist.n_list = 0;
                b_eof = true;
            }
            Err(e) => {
                rc = e;
                p.doclist.p_list = None;
                p.doclist.n_list = 0;
                b_eof = true;
            }
        }
    } else {
        let n_token = p.a_token.len();
        let mut a: Vec<TokenDoclist> = vec![TokenDoclist::default(); n_token];
        debug_assert!(n_token <= MAX_INCR_PHRASE_TOKENS);

        while !b_eof {
            let mut b_max_set = false;
            let mut i_max: i64 = 0; /* o maior docid de todos os iteradores */

            /* Avança o iterador de cada token da frase uma vez. */
            let mut i = 0usize;
            while rc == SQLITE_OK && i < n_token && !b_eof {
                rc = incr_phrase_token_next(p_tab, p, i, &mut a[i], &mut b_eof);
                if !a[i].b_ignore && (!b_max_set || docid_cmp(b_desc, i_max, a[i].i_docid) < 0) {
                    i_max = a[i].i_docid;
                    b_max_set = true;
                }
                i += 1;
            }

            /* Continua avançando os iteradores até todos apontarem o mesmo documento. */
            let mut i = 0usize;
            while i < n_token {
                while rc == SQLITE_OK
                    && !b_eof
                    && !a[i].b_ignore
                    && docid_cmp(b_desc, a[i].i_docid, i_max) < 0
                {
                    rc = incr_phrase_token_next(p_tab, p, i, &mut a[i], &mut b_eof);
                    if docid_cmp(b_desc, a[i].i_docid, i_max) > 0 {
                        i_max = a[i].i_docid;
                        i = 0;
                    }
                }
                i += 1;
            }

            /* Confere se as entradas correntes são mesmo uma frase. */
            if !b_eof {
                let mut n_list = 0i32;
                let mut a_doclist: Vec<u8> = a[n_token - 1].list.clone().unwrap_or_default();

                let mut i = 0usize;
                while i < n_token - 1 {
                    if !a[i].b_ignore {
                        let mut out: Vec<u8> = Vec::new();
                        let (mut pl, mut pr) = (0usize, 0usize);
                        let n_dist = (n_token - 1 - i) as i32;
                        let l = a[i].list.clone().unwrap_or_default();
                        let res = fts3_poslist_phrase_merge(
                            &mut out, n_dist, false, true, &l, &mut pl, &a_doclist, &mut pr,
                        );
                        if !res {
                            break;
                        }
                        n_list = out.len() as i32;
                        a_doclist = out;
                    }
                    i += 1;
                }
                if i == n_token - 1 {
                    p.doclist.i_docid = i_max;
                    p.doclist.set_own_list(a_doclist, n_list);
                    break;
                }
            }
        }
    }

    *pb_eof = b_eof;
    rc
}

/// `fts3EvalPhraseNext`.
fn fts3_eval_phrase_next(
    b_desc: bool,
    p_tab: &Fts3Table,
    p: &mut Fts3Phrase,
    pb_eof: &mut bool,
) -> i32 {
    let mut rc = SQLITE_OK;
    if p.b_incr {
        rc = fts3_eval_incr_phrase_next(b_desc, p_tab, p, pb_eof);
    } else if b_desc != p_tab.b_desc_idx && !p.doclist.a_all.is_empty() {
        let dl = &mut p.doclist;
        fts3_doclist_prev(
            p_tab.b_desc_idx,
            &dl.a_all,
            &mut dl.p_next_docid,
            &mut dl.i_docid,
            &mut dl.n_list,
            pb_eof,
        );
        dl.p_list = dl.p_next_docid;
        dl.b_free_list = false;
    } else {
        fts3_eval_dl_phrase_next(p_tab.b_desc_idx, &mut p.doclist, pb_eof);
    }
    rc
}

/// `Fts3TokenAndCost`.
struct TokenAndCost {
    /// O nó da frase a que o token pertence.
    p_phrase: ExprId,
    /// A posição do token na frase.
    i_token: usize,
    /// `pToken` não nulo.
    live: bool,
    /// A raiz do agrupamento NEAR/AND.
    p_root: Option<ExprId>,
    /// Páginas de estouro para carregar a doclist.
    n_ovfl: i32,
    /// A coluna que o token precisa casar.
    i_col: i32,
}

/// O que a avaliação precisa: a conexão, a tabela, o cursor e a árvore de expressão (retirada do
/// cursor enquanto dura a avaliação).
pub struct Eval<'a> {
    /// A conexão.
    pub db: &'a mut Connection,
    /// A tabela.
    pub p: &'a mut Fts3Table,
    /// O cursor.
    pub csr: &'a mut Fts3Cursor,
    /// A árvore de expressão.
    pub tree: &'a mut Fts3ExprTree,
}

impl<'a> Eval<'a> {
    /// `fts3EvalAllocateReaders`.
    fn allocate_readers(&mut self, expr: Option<ExprId>, pn_token: &mut i32, pn_or: &mut i32, rc: &mut i32) {
        let Some(id) = expr else { return };
        if *rc != SQLITE_OK {
            return;
        }
        if self.tree[id].e_type == FTSQUERY_PHRASE {
            let n_token = phrase_mut(self.tree, id).a_token.len();
            *pn_token += n_token as i32;
            for i in 0..n_token {
                let (z, is_prefix) = {
                    let t = &phrase_mut(self.tree, id).a_token[i];
                    (t.z.clone(), t.is_prefix)
                };
                let (seg, rc2) =
                    fts3_term_seg_reader_cursor(self.db, self.p, self.csr.i_langid, &z, is_prefix);
                phrase_mut(self.tree, id).a_token[i].p_segcsr = Some(seg);
                if rc2 != SQLITE_OK {
                    *rc = rc2;
                    return;
                }
            }
            debug_assert!(phrase_mut(self.tree, id).i_doclist_token == 0);
            phrase_mut(self.tree, id).i_doclist_token = -1;
        } else {
            *pn_or += (self.tree[id].e_type == FTSQUERY_OR) as i32;
            let (l, r) = (self.tree[id].p_left, self.tree[id].p_right);
            self.allocate_readers(l, pn_token, pn_or, rc);
            self.allocate_readers(r, pn_token, pn_or, rc);
        }
    }

    /// `fts3EvalPhraseLoad`: carrega a doclist inteira da frase.
    fn phrase_load(&mut self, id: ExprId) -> i32 {
        let mut rc = SQLITE_OK;
        let n = phrase_mut(self.tree, id).a_token.len();
        let mut i_token = 0usize;
        while rc == SQLITE_OK && i_token < n {
            let ph = phrase_mut(self.tree, id);
            if ph.a_token[i_token].p_segcsr.is_some() {
                let i_col = ph.i_column;
                match fts3_term_select(self.db, self.p, &mut ph.a_token[i_token], i_col) {
                    Ok(list) => {
                        rc = fts3_eval_phrase_merge_token(self.p.b_desc_idx, ph, i_token as i32, list)
                    }
                    Err(e) => rc = e,
                }
            }
            i_token += 1;
        }
        rc
    }

    /// `fts3EvalPhraseStart`.
    fn phrase_start(&mut self, b_opt_ok: bool, id: ExprId) -> i32 {
        let mut rc = SQLITE_OK;
        let b_desc = self.csr.b_desc;
        let b_desc_idx = self.p.b_desc_idx;
        let n_column = self.p.n_column();
        let mut b_have_incr = false;
        let ph = phrase_mut(self.tree, id);
        let n_token = ph.a_token.len();
        let mut b_incr_ok = b_opt_ok && b_desc == b_desc_idx && n_token <= MAX_INCR_PHRASE_TOKENS && n_token > 0;
        let mut i = 0usize;
        while b_incr_ok && i < n_token {
            let t = &ph.a_token[i];
            if t.b_first || (t.p_segcsr.as_ref().is_some_and(|s| !s.b_lookup)) {
                b_incr_ok = false;
            }
            if t.p_segcsr.is_some() {
                b_have_incr = true;
            }
            i += 1;
        }

        if b_incr_ok && b_have_incr {
            /* Usa a carga incremental. */
            let i_col = if ph.i_column >= n_column { -1 } else { ph.i_column };
            let mut i = 0usize;
            while rc == SQLITE_OK && i < n_token {
                let t = &mut ph.a_token[i];
                let z = t.z.clone();
                if let Some(seg) = t.p_segcsr.as_mut() {
                    rc = fts3_msr_incr_start(self.db, self.p, seg, i_col, &z);
                }
                i += 1;
            }
            ph.b_incr = true;
        } else {
            /* Carrega a doclist inteira da frase na memória. */
            rc = self.phrase_load(id);
            phrase_mut(self.tree, id).b_incr = false;
        }
        rc
    }

    /// `fts3EvalStartReaders`.
    fn start_readers(&mut self, expr: Option<ExprId>, rc: &mut i32) {
        let Some(id) = expr else { return };
        if *rc != SQLITE_OK {
            return;
        }
        if self.tree[id].e_type == FTSQUERY_PHRASE {
            let ph = phrase_mut(self.tree, id);
            let n_token = ph.a_token.len();
            if n_token > 0 {
                let all_deferred = ph.a_token.iter().all(|t| t.p_deferred.is_some());
                self.tree[id].b_deferred = all_deferred;
            }
            *rc = self.phrase_start(true, id);
        } else {
            let (l, r) = (self.tree[id].p_left, self.tree[id].p_right);
            self.start_readers(l, rc);
            self.start_readers(r, rc);
            let d = match (l, r) {
                (Some(l), Some(r)) => self.tree[l].b_deferred && self.tree[r].b_deferred,
                _ => false,
            };
            self.tree[id].b_deferred = d;
        }
    }

    /// `fts3EvalTokenCosts`.
    fn token_costs(
        &mut self,
        p_root: Option<ExprId>,
        expr: ExprId,
        a_tc: &mut Vec<TokenAndCost>,
        ap_or: &mut Vec<ExprId>,
        rc: &mut i32,
    ) {
        if *rc != SQLITE_OK {
            return;
        }
        let e_type = self.tree[expr].e_type;
        if e_type == FTSQUERY_PHRASE {
            let n_token = phrase_mut(self.tree, expr).a_token.len();
            let mut i = 0usize;
            while *rc == SQLITE_OK && i < n_token {
                let ph = phrase_mut(self.tree, expr);
                let i_col = ph.i_column;
                a_tc.push(TokenAndCost {
                    p_phrase: expr,
                    i_token: i,
                    live: true,
                    p_root,
                    n_ovfl: 0,
                    i_col,
                });
                let r = match ph.a_token[i].p_segcsr.as_ref() {
                    Some(seg) => fts3_msr_ovfl(self.db, self.p, seg),
                    None => Ok(0),
                };
                match r {
                    Ok(n) => {
                        if let Some(last) = a_tc.last_mut() {
                            last.n_ovfl = n;
                        }
                    }
                    Err(e) => *rc = e,
                }
                i += 1;
            }
        } else if e_type != FTSQUERY_NOT {
            let (l, r) = (self.tree[expr].p_left, self.tree[expr].p_right);
            let (Some(l), Some(r)) = (l, r) else { return };
            let mut p_root = p_root;
            if e_type == FTSQUERY_OR {
                p_root = Some(l);
                ap_or.push(l);
            }
            self.token_costs(p_root, l, a_tc, ap_or, rc);
            if e_type == FTSQUERY_OR {
                p_root = Some(r);
                ap_or.push(r);
            }
            self.token_costs(p_root, r, a_tc, ap_or, rc);
        }
    }

    /// `fts3EvalAverageDocsize`.
    fn average_docsize(&mut self, pn_page: &mut i32) -> i32 {
        let mut rc = SQLITE_OK;
        if self.csr.n_row_avg == 0 {
            /* O tamanho médio dos documentos, de que o custo de cada doclist depende, ainda não
            ** foi calculado. A linha 0 de `%_stat` é um blob com (nCol+1) varints: o número de
            ** documentos e, depois, o total de dados de cada coluna. */
            let mut n_doc: i64 = 0;
            let mut n_byte: i64 = 0;

            let st = match fts3_select_doctotal(self.db, self.p) {
                Ok(s) => s,
                Err(rc) => return rc,
            };
            let blob: Option<Vec<u8>> = column_blob(self.db, st, 0).map(|b| b.to_vec());
            let _ = column_bytes(self.db, st, 0);
            if let Some(a) = blob {
                let mut i = 0usize;
                let (n, v) = fts3_get_varint_bounded(&a);
                i += n as usize;
                n_doc = v;
                while i < a.len() {
                    let (n, v) = fts3_get_varint_bounded(&a[i..]);
                    i += n as usize;
                    n_byte = v;
                }
            }
            if n_doc == 0 || n_byte == 0 {
                reset(self.db, st);
                return FTS_CORRUPT_VTAB;
            }

            self.csr.n_doc = n_doc;
            let pgsz = self.p.n_pgsz as i64;
            self.csr.n_row_avg = (((n_byte / n_doc) + pgsz) / pgsz) as i32;
            rc = reset(self.db, st);
        }

        *pn_page = self.csr.n_row_avg;
        rc
    }

    /// `fts3EvalSelectDeferred`.
    fn select_deferred(&mut self, p_root: Option<ExprId>, a_tc: &mut [TokenAndCost]) -> i32 {
        let mut n_doc_size = 0;
        let mut rc;
        let mut n_ovfl = 0;
        let mut n_token = 0;
        let mut n_min_est = 0;
        let mut n_load4: i32 = 1;

        /* Tokens nunca são adiados em tabelas criadas com a opção content=xxx. */
        if self.p.z_content_tbl.is_some() {
            return SQLITE_OK;
        }

        /* Conta os tokens deste agrupamento. Se nenhuma doclist passa para páginas de estouro,
        ** ou se há só 1 token, não há o que adiar. */
        for tc in a_tc.iter() {
            if tc.p_root == p_root {
                n_ovfl += tc.n_ovfl;
                n_token += 1;
            }
        }
        if n_ovfl == 0 || n_token < 2 {
            return SQLITE_OK;
        }

        /* O tamanho médio dos documentos (em páginas). */
        rc = self.average_docsize(&mut n_doc_size);

        /* Percorre os tokens do agrupamento em ordem crescente do número de páginas de estouro e
        ** adia o token se carregar a doclist custa N páginas ou mais, com
        ** N = (nMinEst + 4^nOther - 1) / 4^nOther. */
        let mut ii = 0;
        while ii < n_token && rc == SQLITE_OK {
            let mut p_tc: Option<usize> = None; /* o token restante mais barato */
            for (i_tc, tc) in a_tc.iter().enumerate() {
                if tc.live
                    && tc.p_root == p_root
                    && p_tc.map_or(true, |j| tc.n_ovfl < a_tc[j].n_ovfl)
                {
                    p_tc = Some(i_tc);
                }
            }
            let Some(j) = p_tc else { break };
            let (c_ovfl, c_phrase, c_token, c_col) =
                (a_tc[j].n_ovfl, a_tc[j].p_phrase, a_tc[j].i_token, a_tc[j].i_col);

            let div = (n_load4 / 4).max(1);
            if ii != 0 && c_ovfl >= ((n_min_est + (n_load4 / 4) - 1) / div) * n_doc_size {
                /* O número de páginas de estouro a carregar para este token (e portanto os
                ** seguintes) é maior que o estimado se todos os seguintes forem adiados. */
                let ph = phrase_mut(self.tree, c_phrase);
                let tok = &mut ph.a_token[c_token];
                rc = fts3_defer_token(self.csr, tok, c_col);
                if let Some(seg) = tok.p_segcsr.take() {
                    fts3_seg_reader_cursor_free(self.db, seg);
                }
            } else {
                /* nLoad4 vale (4^nOther) na próxima volta, limitado a 2^24. */
                if ii < 12 {
                    n_load4 *= 4;
                }

                let multi = phrase_mut(self.tree, c_phrase).a_token.len() > 1;
                if ii == 0 || (multi && ii != n_token - 1) {
                    /* Ou é o token mais barato da consulta, ou é parte de uma frase de vários
                    ** tokens: a doclist inteira será carregada, que seja agora. */
                    let ph = phrase_mut(self.tree, c_phrase);
                    let b_desc_idx = self.p.b_desc_idx;
                    match fts3_term_select(self.db, self.p, &mut ph.a_token[c_token], c_col) {
                        Err(e) => rc = e,
                        Ok(list) => {
                            rc = fts3_eval_phrase_merge_token(b_desc_idx, ph, c_token as i32, list);
                        }
                    }
                    if rc == SQLITE_OK {
                        let n_count = fts3_doclist_count_docids(&ph.doclist.a_all);
                        if ii == 0 || n_count < n_min_est {
                            n_min_est = n_count;
                        }
                    }
                }
            }
            a_tc[j].live = false;
            ii += 1;
        }

        rc
    }

    /// `fts3EvalStart`.
    fn start(&mut self) -> i32 {
        let mut rc = SQLITE_OK;
        let mut n_token = 0;
        let mut n_or = 0;
        let root = self.tree.root;

        /* Um leitor de segmentos para cada token da expressão. */
        self.allocate_readers(root, &mut n_token, &mut n_or, &mut rc);

        /* Decide quais tokens da expressão devem ser adiados, se algum. */
        if rc == SQLITE_OK && n_token > 1 && self.p.b_fts4 {
            if let Some(r) = root {
                let mut a_tc: Vec<TokenAndCost> = Vec::new();
                let mut ap_or: Vec<ExprId> = Vec::new();
                self.token_costs(None, r, &mut a_tc, &mut ap_or, &mut rc);

                if rc == SQLITE_OK {
                    rc = self.select_deferred(None, &mut a_tc);
                    let mut ii = 0;
                    while rc == SQLITE_OK && ii < ap_or.len() {
                        rc = self.select_deferred(Some(ap_or[ii]), &mut a_tc);
                        ii += 1;
                    }
                }
            }
        }

        self.start_readers(root, &mut rc);
        rc
    }

    /// `fts3EvalNearTrim`: apara a lista de posições da frase `ph` pela vizinhança da lista
    /// `(pos_phrase, pos_off)`. Devolve verdadeiro se sobrou algo.
    fn near_trim(
        &mut self,
        n_near: i32,
        pos: &mut ExprId,
        pn_token: &mut i32,
        ph_id: ExprId,
    ) -> bool {
        let left: Vec<u8> = phrase_mut(self.tree, *pos).doclist.list_bytes();
        let ph = phrase_mut(self.tree, ph_id);
        let n_param1 = n_near + ph.n_token();
        let n_param2 = n_near + *pn_token;
        let right: Vec<u8> = ph.doclist.list_bytes();

        let mut out: Vec<u8> = Vec::new();
        let mut a_tmp: Vec<u8> = Vec::new();
        let (mut p1, mut p2) = (0usize, 0usize);
        let res = fts3_poslist_near_merge(
            &mut out, &mut a_tmp, n_param1, n_param2, &left, &mut p1, &right, &mut p2,
        );
        if res {
            let n_new = out.len() as i64 - 1;
            let n_list = ph.doclist.n_list;
            let off = ph.doclist.p_list.unwrap_or(0);
            let buf = ph.doclist.list_buf_mut();
            if buf.len() < off + out.len() {
                buf.resize(off + out.len(), 0);
            }
            buf[off..off + out.len()].copy_from_slice(&out);
            if n_new >= 0 && n_new <= n_list as i64 {
                let a = off + n_new as usize;
                let b = (off + n_list as usize).min(buf.len());
                if a < b {
                    buf[a..b].fill(0);
                }
                ph.doclist.n_list = n_new as i32;
            }
            *pos = ph_id;
            *pn_token = ph.n_token();
        }
        res
    }

    /// `fts3EvalNextRow`.
    fn next_row(&mut self, id: ExprId, rc: &mut i32) {
        if *rc != SQLITE_OK || self.tree[id].b_eof {
            return;
        }
        let b_desc = self.csr.b_desc;
        self.tree[id].b_start = true;

        match self.tree[id].e_type {
            FTSQUERY_NEAR | FTSQUERY_AND => {
                let (Some(left), Some(right)) = (self.tree[id].p_left, self.tree[id].p_right) else {
                    return;
                };
                if self.tree[left].b_deferred {
                    /* O lado esquerdo é todo adiado: supõe-se que casa todas as linhas. */
                    self.next_row(right, rc);
                    self.tree[id].i_docid = self.tree[right].i_docid;
                    self.tree[id].b_eof = self.tree[right].b_eof;
                } else if self.tree[right].b_deferred {
                    self.next_row(left, rc);
                    self.tree[id].i_docid = self.tree[left].i_docid;
                    self.tree[id].b_eof = self.tree[left].b_eof;
                } else {
                    /* Nem o lado direito nem o esquerdo são adiados. */
                    self.next_row(left, rc);
                    self.next_row(right, rc);
                    while !self.tree[left].b_eof && !self.tree[right].b_eof && *rc == SQLITE_OK {
                        let i_diff = docid_cmp(b_desc, self.tree[left].i_docid, self.tree[right].i_docid);
                        if i_diff == 0 {
                            break;
                        }
                        if i_diff < 0 {
                            self.next_row(left, rc);
                        } else {
                            self.next_row(right, rc);
                        }
                    }
                    self.tree[id].i_docid = self.tree[left].i_docid;
                    self.tree[id].b_eof = self.tree[left].b_eof || self.tree[right].b_eof;
                    if self.tree[id].e_type == FTSQUERY_NEAR && self.tree[id].b_eof {
                        debug_assert!(self.tree[right].e_type == FTSQUERY_PHRASE);
                        if !phrase_mut(self.tree, right).doclist.a_all.is_empty() {
                            while *rc == SQLITE_OK && !self.tree[right].b_eof {
                                zero_list(&mut phrase_mut(self.tree, right).doclist);
                                self.next_row(right, rc);
                            }
                        }
                        let left_has = self.tree[left]
                            .p_phrase
                            .as_ref()
                            .is_some_and(|ph| !ph.doclist.a_all.is_empty());
                        if left_has {
                            while *rc == SQLITE_OK && !self.tree[left].b_eof {
                                zero_list(&mut phrase_mut(self.tree, left).doclist);
                                self.next_row(left, rc);
                            }
                        }
                        self.tree[right].b_eof = true;
                        self.tree[left].b_eof = true;
                    }
                }
            }

            FTSQUERY_OR => {
                let (Some(left), Some(right)) = (self.tree[id].p_left, self.tree[id].p_right) else {
                    return;
                };
                let mut i_cmp = docid_cmp(b_desc, self.tree[left].i_docid, self.tree[right].i_docid);

                if self.tree[right].b_eof || (!self.tree[left].b_eof && i_cmp < 0) {
                    self.next_row(left, rc);
                } else if self.tree[left].b_eof || i_cmp > 0 {
                    self.next_row(right, rc);
                } else {
                    self.next_row(left, rc);
                    self.next_row(right, rc);
                }

                self.tree[id].b_eof = self.tree[left].b_eof && self.tree[right].b_eof;
                i_cmp = docid_cmp(b_desc, self.tree[left].i_docid, self.tree[right].i_docid);
                if self.tree[right].b_eof || (!self.tree[left].b_eof && i_cmp < 0) {
                    self.tree[id].i_docid = self.tree[left].i_docid;
                } else {
                    self.tree[id].i_docid = self.tree[right].i_docid;
                }
            }

            FTSQUERY_NOT => {
                let (Some(left), Some(right)) = (self.tree[id].p_left, self.tree[id].p_right) else {
                    return;
                };
                if !self.tree[right].b_start {
                    self.next_row(right, rc);
                }

                self.next_row(left, rc);
                if !self.tree[left].b_eof {
                    while *rc == SQLITE_OK
                        && !self.tree[right].b_eof
                        && docid_cmp(b_desc, self.tree[left].i_docid, self.tree[right].i_docid) > 0
                    {
                        self.next_row(right, rc);
                    }
                }
                self.tree[id].i_docid = self.tree[left].i_docid;
                self.tree[id].b_eof = self.tree[left].b_eof;
            }

            _ => {
                let ph = phrase_mut(self.tree, id);
                fts3_eval_invalidate_poslist(ph);
                let mut b_eof = false;
                *rc = fts3_eval_phrase_next(b_desc, self.p, ph, &mut b_eof);
                let i_docid = ph.doclist.i_docid;
                self.tree[id].b_eof = b_eof;
                self.tree[id].i_docid = i_docid;
            }
        }
    }

    /// `fts3EvalNearTest`.
    fn near_test(&mut self, expr: ExprId, rc: &mut i32) -> bool {
        let mut res = true;

        /* O bloco roda se `expr` é a raiz de uma consulta NEAR. O filho direito de um NEAR é
        ** sempre uma frase; o esquerdo é uma frase ou um NEAR. */
        let parent_near = self.tree[expr]
            .p_parent
            .is_some_and(|pp| self.tree[pp].e_type == FTSQUERY_NEAR);
        if *rc == SQLITE_OK && self.tree[expr].e_type == FTSQUERY_NEAR && !parent_near {
            let mut p = expr;
            while let Some(l) = self.tree[p].p_left {
                p = l;
            }
            let mut pos = p;
            let mut n_token = phrase_mut(self.tree, p).n_token();

            let mut q = self.tree[p].p_parent;
            while res {
                let Some(qq) = q else { break };
                if self.tree[qq].e_type != FTSQUERY_NEAR {
                    break;
                }
                let Some(ph_id) = self.tree[qq].p_right else { break };
                let n_near = self.tree[qq].n_near;
                res = self.near_trim(n_near, &mut pos, &mut n_token, ph_id);
                q = self.tree[qq].p_parent;
            }

            let Some(right) = self.tree[expr].p_right else { return res };
            pos = right;
            n_token = phrase_mut(self.tree, right).n_token();
            let mut q = self.tree[expr].p_left;
            while res {
                let Some(qq) = q else { break };
                let Some(par) = self.tree[qq].p_parent else { break };
                let n_near = self.tree[par].n_near;
                let ph_id = if self.tree[qq].e_type == FTSQUERY_NEAR {
                    match self.tree[qq].p_right {
                        Some(r) => r,
                        None => break,
                    }
                } else {
                    qq
                };
                res = self.near_trim(n_near, &mut pos, &mut n_token, ph_id);
                q = self.tree[qq].p_left;
            }
        }
        res
    }

    /// `fts3EvalTestExpr`.
    fn test_expr(&mut self, expr: ExprId, rc: &mut i32) -> bool {
        let mut b_hit = true;
        if *rc != SQLITE_OK {
            return b_hit;
        }
        match self.tree[expr].e_type {
            FTSQUERY_NEAR | FTSQUERY_AND => {
                let (Some(l), Some(r)) = (self.tree[expr].p_left, self.tree[expr].p_right) else {
                    return b_hit;
                };
                b_hit = self.test_expr(l, rc) && self.test_expr(r, rc) && self.near_test(expr, rc);

                /* Se a expressão NEAR não casa nenhuma linha, zera a doclist de todas as frases
                ** envolvidas: `snippet()`, `offsets()` e `matchinfo()` não devem reconhecer
                ** ocorrências de frases de NEAR que não casaram. */
                let parent_near = self.tree[expr]
                    .p_parent
                    .is_some_and(|pp| self.tree[pp].e_type == FTSQUERY_NEAR);
                if !b_hit && self.tree[expr].e_type == FTSQUERY_NEAR && !parent_near {
                    let mut q = expr;
                    while self.tree[q].p_phrase.is_none() {
                        let (Some(rr), Some(ll)) = (self.tree[q].p_right, self.tree[q].p_left) else {
                            break;
                        };
                        if self.tree[rr].i_docid == self.csr.i_prev_id {
                            fts3_eval_invalidate_poslist(phrase_mut(self.tree, rr));
                        }
                        q = ll;
                    }
                    if self.tree[q].i_docid == self.csr.i_prev_id && self.tree[q].p_phrase.is_some() {
                        fts3_eval_invalidate_poslist(phrase_mut(self.tree, q));
                    }
                }
            }

            FTSQUERY_OR => {
                let (Some(l), Some(r)) = (self.tree[expr].p_left, self.tree[expr].p_right) else {
                    return b_hit;
                };
                let b_hit1 = self.test_expr(l, rc);
                let b_hit2 = self.test_expr(r, rc);
                b_hit = b_hit1 || b_hit2;
            }

            FTSQUERY_NOT => {
                let (Some(l), Some(r)) = (self.tree[expr].p_left, self.tree[expr].p_right) else {
                    return b_hit;
                };
                b_hit = self.test_expr(l, rc) && !self.test_expr(r, rc);
            }

            _ => {
                let deferred = !self.csr.p_deferred.is_empty()
                    && (self.tree[expr].b_deferred
                        || (self.tree[expr].i_docid == self.csr.i_prev_id
                            && phrase_mut(self.tree, expr).doclist.p_list.is_some()));
                if deferred {
                    let b_def = self.tree[expr].b_deferred;
                    let ph = phrase_mut(self.tree, expr);
                    if b_def {
                        fts3_eval_invalidate_poslist(ph);
                    }
                    *rc = fts3_eval_deferred_phrase(self.csr, ph);
                    b_hit = ph.doclist.p_list.is_some();
                    self.tree[expr].i_docid = self.csr.i_prev_id;
                } else {
                    let n_list = phrase_mut(self.tree, expr).doclist.n_list;
                    b_hit = !self.tree[expr].b_eof
                        && self.tree[expr].i_docid == self.csr.i_prev_id
                        && n_list > 0;
                }
            }
        }
        b_hit
    }

    /// `sqlite3Fts3EvalTestDeferred`: verdadeiro se a linha corrente não casa, considerando os
    /// tokens adiados e o NEAR.
    pub fn test_deferred(&mut self, p_rc: &mut i32) -> bool {
        let mut rc = *p_rc;
        let mut b_miss = false;
        if rc == SQLITE_OK {
            /* Com tokens adiados, carrega a linha corrente na memória e a varre para achar a lista
            ** de posições de cada token adiado. */
            if !self.csr.p_deferred.is_empty() {
                rc = fts3_cursor_seek(self.db, self.p, self.csr);
                if rc == SQLITE_OK {
                    rc = fts3_cache_deferred_doclists(self.db, self.p, self.csr);
                }
            }
            if let Some(root) = self.tree.root {
                b_miss = !self.test_expr(root, &mut rc);
            }

            /* Solta as listas de posições acumuladas para os tokens adiados. */
            fts3_free_deferred_doclists(self.csr);
            *p_rc = rc;
        }
        rc == SQLITE_OK && b_miss
    }

    /// `fts3EvalRestart`.
    pub fn restart(&mut self, expr: Option<ExprId>, rc: &mut i32) {
        let Some(id) = expr else { return };
        if *rc != SQLITE_OK {
            return;
        }
        if self.tree[id].p_phrase.is_some() {
            {
                let ph = phrase_mut(self.tree, id);
                fts3_eval_invalidate_poslist(ph);
                if ph.b_incr {
                    for tok in ph.a_token.iter_mut() {
                        debug_assert!(tok.p_deferred.is_none());
                        if let Some(seg) = tok.p_segcsr.as_mut() {
                            fts3_msr_incr_restart(seg);
                        }
                    }
                }
            }
            if phrase_mut(self.tree, id).b_incr {
                *rc = self.phrase_start(false, id);
            }
            let ph = phrase_mut(self.tree, id);
            ph.doclist.p_next_docid = None;
            ph.doclist.i_docid = 0;
            ph.p_or_poslist = None;
        }

        self.tree[id].i_docid = 0;
        self.tree[id].b_eof = false;
        self.tree[id].b_start = false;

        let (l, r) = (self.tree[id].p_left, self.tree[id].p_right);
        self.restart(l, rc);
        self.restart(r, rc);
    }

    /// `sqlite3Fts3EvalNextRow` (o `fts3EvalNextRow` público para o `snippet.rs`).
    pub fn next_row_pub(&mut self, id: ExprId, rc: &mut i32) {
        self.next_row(id, rc);
    }

    /// `fts3EvalGatherStats`.
    fn gather_stats(&mut self, expr: ExprId) -> i32 {
        let mut rc = SQLITE_OK;
        debug_assert!(self.tree[expr].e_type == FTSQUERY_PHRASE);
        if self.tree[expr].a_mi.is_empty() {
            let n_col = self.p.n_column();
            let i_prev_id = self.csr.i_prev_id;

            /* Acha a raiz da expressão NEAR. */
            let mut root = expr;
            while let Some(par) = self.tree[root].p_parent {
                if self.tree[par].e_type == FTSQUERY_NEAR || self.tree[root].b_deferred {
                    root = par;
                } else {
                    break;
                }
            }
            let i_docid = self.tree[root].i_docid;
            let b_eof = self.tree[root].b_eof;
            debug_assert!(self.tree[root].b_start);

            /* Aloca o `aMI[]` de cada nó FTSQUERY_PHRASE. */
            rc = fts3_expr_iterate(self.tree, root, &mut |t, id, _| {
                t[id].a_mi = vec![0u32; (n_col * 3).max(0) as usize];
                SQLITE_OK
            });
            if rc != SQLITE_OK {
                return rc;
            }
            self.restart(Some(root), &mut rc);

            while !self.csr.is_eof && rc == SQLITE_OK {
                loop {
                    /* Garante que o comando de `%_content` está reiniciado. */
                    if !self.csr.is_require_seek {
                        reset_opt(self.db, self.csr.p_stmt);
                    }

                    /* Avança para o próximo documento. */
                    self.next_row(root, &mut rc);
                    self.csr.is_eof = self.tree[root].b_eof;
                    self.csr.is_require_seek = true;
                    self.csr.is_matchinfo_needed = true;
                    self.csr.i_prev_id = self.tree[root].i_docid;
                    if !(!self.csr.is_eof
                        && self.tree[root].e_type == FTSQUERY_NEAR
                        && self.test_deferred(&mut rc))
                    {
                        break;
                    }
                }

                if rc == SQLITE_OK && !self.csr.is_eof {
                    update_counts(self.tree, Some(root), n_col);
                }
            }

            self.csr.is_eof = false;
            self.csr.i_prev_id = i_prev_id;

            if b_eof {
                self.tree[root].b_eof = b_eof;
            } else {
                /* Cuidado: a raiz pode percorrer os docids em ordem crescente ou decrescente,
                ** por isso o laço não pode comparar `iDocid` com `<`. */
                self.restart(Some(root), &mut rc);
                loop {
                    self.next_row(root, &mut rc);
                    if self.tree[root].b_eof {
                        rc = FTS_CORRUPT_VTAB;
                    }
                    if !(self.tree[root].i_docid != i_docid && rc == SQLITE_OK) {
                        break;
                    }
                }
            }
        }
        rc
    }

    /// `sqlite3Fts3EvalPhraseStats`: preenche `ai_out` com, por coluna, o número de ocorrências
    /// (`[3*i+1]`) e o de linhas com pelo menos uma (`[3*i+2]`) da frase `expr`.
    pub fn phrase_stats(&mut self, expr: ExprId, ai_out: &mut [u32]) -> i32 {
        let mut rc = SQLITE_OK;
        let n_col = self.p.n_column().max(0) as usize;

        let parent_near = self.tree[expr]
            .p_parent
            .is_some_and(|pp| self.tree[pp].e_type == FTSQUERY_NEAR);
        if self.tree[expr].b_deferred && !parent_near {
            debug_assert!(self.csr.n_doc > 0);
            for i_col in 0..n_col {
                ai_out[i_col * 3 + 1] = self.csr.n_doc as u32;
                ai_out[i_col * 3 + 2] = self.csr.n_doc as u32;
            }
        } else {
            rc = self.gather_stats(expr);
            if rc == SQLITE_OK {
                for i_col in 0..n_col {
                    ai_out[i_col * 3 + 1] = self.tree[expr].a_mi[i_col * 3 + 1];
                    ai_out[i_col * 3 + 2] = self.tree[expr].a_mi[i_col * 3 + 2];
                }
            }
        }
        rc
    }

    /// `sqlite3Fts3EvalPhrasePoslist`: a lista de posições da frase `expr` na coluna `i_col` da
    /// linha corrente (uma cópia, do começo da lista da coluna até o `0x00` final do documento).
    pub fn phrase_poslist(&mut self, expr: ExprId, i_col: i32) -> Result<Option<Vec<u8>>, i32> {
        let n_column = self.p.n_column();
        let b_desc_doclist = self.p.b_desc_idx;
        let b_desc = self.csr.b_desc;
        let i_prev_id = self.csr.i_prev_id;

        /* Se a frase vale para uma coluna que não é `i_col`, devolve NULL. */
        debug_assert!(i_col >= 0 && i_col < n_column);
        {
            let ph = phrase_mut(self.tree, expr);
            if ph.i_column < n_column && ph.i_column != i_col {
                return Ok(None);
            }
        }

        let mut i_docid = self.tree[expr].i_docid;
        let mut p_iter: Option<usize> = phrase_mut(self.tree, expr).doclist.p_list;
        let mut from_or = false;
        if i_docid != i_prev_id || self.tree[expr].b_eof {
            let mut rc = SQLITE_OK;
            let mut b_or = false;
            let mut b_tree_eof = false;
            let mut p_near = expr; /* o ancestral NEAR mais sênior (ou `expr`) */

            /* Se a frase não descende de um OR, devolve NULL. Senão a entrada do docid
            ** `iPrevId` pode estar antes no buffer da doclist. */
            let mut q = self.tree[expr].p_parent;
            while let Some(pp) = q {
                if self.tree[pp].e_type == FTSQUERY_OR {
                    b_or = true;
                }
                if self.tree[pp].e_type == FTSQUERY_NEAR {
                    p_near = pp;
                }
                if self.tree[pp].b_eof {
                    b_tree_eof = true;
                }
                q = self.tree[pp].p_parent;
            }
            if !b_or {
                return Ok(None);
            }
            let mut p_run = p_near; /* o ancestral não adiado mais próximo de `p_near` */
            while self.tree[p_run].b_deferred {
                match self.tree[p_run].p_parent {
                    Some(pp) => p_run = pp,
                    None => break,
                }
            }

            /* Descendente de um OR: não pode usar frase incremental; carrega a doclist inteira. */
            if phrase_mut(self.tree, expr).b_incr {
                let b_eof_save = self.tree[p_run].b_eof;
                self.restart(Some(p_run), &mut rc);
                while rc == SQLITE_OK && !self.tree[p_run].b_eof {
                    self.next_row(p_run, &mut rc);
                    if !b_eof_save && self.tree[p_run].i_docid == i_docid {
                        break;
                    }
                }
                if rc == SQLITE_OK && self.tree[p_run].b_eof != b_eof_save {
                    rc = FTS_CORRUPT_VTAB;
                }
            }
            if b_tree_eof {
                while rc == SQLITE_OK && !self.tree[p_run].b_eof {
                    self.next_row(p_run, &mut rc);
                }
            }
            if rc != SQLITE_OK {
                return Err(rc);
            }

            let mut b_match = true;
            let mut q = Some(p_near);
            while let Some(pp) = q {
                let mut b_eof;
                let mut p_test = pp;
                if self.tree[p_test].e_type == FTSQUERY_NEAR {
                    match self.tree[p_test].p_right {
                        Some(r) => p_test = r,
                        None => break,
                    }
                }
                let ph = phrase_mut(self.tree, p_test);

                let mut it = ph.p_or_poslist;
                i_docid = ph.i_or_docid;
                if b_desc == b_desc_doclist {
                    b_eof = ph.doclist.a_all.is_empty()
                        || it.is_some_and(|o| o >= ph.doclist.a_all.len());
                    while (it.is_none() || docid_cmp(b_desc_doclist, i_docid, i_prev_id) < 0) && !b_eof {
                        fts3_doclist_next(b_desc_doclist, &ph.doclist.a_all, &mut it, &mut i_docid, &mut b_eof);
                    }
                } else {
                    b_eof = ph.doclist.a_all.is_empty() || it.is_some_and(|o| o == 0);
                    while (it.is_none() || docid_cmp(b_desc_doclist, i_docid, i_prev_id) > 0) && !b_eof {
                        let mut dummy = 0i32;
                        fts3_doclist_prev(
                            b_desc_doclist,
                            &ph.doclist.a_all,
                            &mut it,
                            &mut i_docid,
                            &mut dummy,
                            &mut b_eof,
                        );
                    }
                }
                ph.p_or_poslist = it;
                ph.i_or_docid = i_docid;
                if b_eof || i_docid != i_prev_id {
                    b_match = false;
                }
                q = self.tree[pp].p_left;
            }

            if b_match {
                p_iter = phrase_mut(self.tree, expr).p_or_poslist;
                from_or = true;
            } else {
                p_iter = None;
            }
        }
        let Some(mut it) = p_iter else { return Ok(None) };

        let ph = phrase_mut(self.tree, expr);
        let buf: &[u8] = if from_or { &ph.doclist.a_all } else { ph.doclist.list_buf() };

        let mut i_this: i32;
        if at(buf, it) == 0x01 {
            it += 1;
            let (n, v) = fts3_get_varint32(sl(buf, it));
            it += n as usize;
            i_this = v;
        } else {
            i_this = 0;
        }
        while i_this < i_col {
            fts3_columnlist_copy(None, buf, &mut it);
            if at(buf, it) == 0x00 {
                return Ok(None);
            }
            it += 1;
            let (n, v) = fts3_get_varint32(sl(buf, it));
            it += n as usize;
            i_this = v;
        }
        if at(buf, it) == 0x00 || i_col != i_this {
            return Ok(None);
        }

        let mut end = it;
        fts3_poslist_copy(None, buf, &mut end);
        let end = end.min(buf.len());
        Ok(Some(buf.get(it..end).map(|s| s.to_vec()).unwrap_or_default()))
    }
}

/// `memset(pDl->pList, 0, pDl->nList)`.
fn zero_list(dl: &mut Fts3Doclist) {
    let off = dl.p_list.unwrap_or(0);
    let n = dl.n_list.max(0) as usize;
    if dl.p_list.is_some() {
        let buf = dl.list_buf_mut();
        let end = (off + n).min(buf.len());
        if off < end {
            buf[off..end].fill(0);
        }
    }
}

/// `fts3EvalUpdateCounts`.
fn update_counts(tree: &mut Fts3ExprTree, p_expr: Option<ExprId>, n_col: i32) {
    let Some(id) = p_expr else { return };
    let mut counts: Vec<(i32, i32)> = Vec::new();
    if let Some(ph) = tree[id].p_phrase.as_ref() {
        if ph.doclist.p_list.is_some() {
            let buf = ph.doclist.list_tail();
            let mut i_col = 0i32;
            let mut p = 0usize;
            loop {
                let mut c: u8 = 0;
                let mut i_cnt = 0i32;
                while (0xFE & (at(buf, p) | c)) != 0 {
                    if (c & 0x80) == 0 {
                        i_cnt += 1;
                    }
                    c = at(buf, p) & 0x80;
                    p += 1;
                }
                counts.push((i_col, i_cnt));
                if at(buf, p) == 0x00 {
                    break;
                }
                p += 1;
                let (n, v) = fts3_get_varint32(sl(buf, p));
                p += n as usize;
                i_col = v;
                if i_col >= n_col {
                    break;
                }
            }
        }
    }
    for (i_col, i_cnt) in counts {
        let base = (i_col.max(0) as usize) * 3;
        if let Some(a) = tree[id].a_mi.get_mut(base + 1) {
            *a = a.wrapping_add(i_cnt as u32);
        }
        if let Some(a) = tree[id].a_mi.get_mut(base + 2) {
            *a = a.wrapping_add((i_cnt > 0) as u32);
        }
    }
    let (l, r) = (tree[id].p_left, tree[id].p_right);
    update_counts(tree, l, n_col);
    update_counts(tree, r, n_col);
}

/// `fts3EvalStart`: aloca os leitores de segmentos, decide os tokens adiados e prepara as frases.
pub fn fts3_eval_start(db: &mut Connection, p: &mut Fts3Table, p_csr: &mut Fts3Cursor) -> i32 {
    let Some(mut tree) = p_csr.p_expr.take() else {
        return SQLITE_OK;
    };
    let rc = {
        let mut ev = Eval { db, p, csr: p_csr, tree: &mut tree };
        ev.start()
    };
    p_csr.p_expr = Some(tree);
    rc
}

/// `fts3EvalNext`: avança o cursor para a próxima linha da consulta de texto completo.
pub fn fts3_eval_next(db: &mut Connection, p: &mut Fts3Table, p_csr: &mut Fts3Cursor) -> i32 {
    let mut rc = SQLITE_OK;
    match p_csr.p_expr.take() {
        None => p_csr.is_eof = true,
        Some(mut tree) => {
            {
                let mut ev = Eval { db, p, csr: p_csr, tree: &mut tree };
                if let Some(root) = ev.tree.root {
                    loop {
                        if !ev.csr.is_require_seek {
                            reset_opt(ev.db, ev.csr.p_stmt);
                        }
                        ev.next_row(root, &mut rc);
                        ev.csr.is_eof = ev.tree[root].b_eof;
                        ev.csr.is_require_seek = true;
                        ev.csr.is_matchinfo_needed = true;
                        ev.csr.i_prev_id = ev.tree[root].i_docid;
                        if !(!ev.csr.is_eof && ev.test_deferred(&mut rc)) {
                            break;
                        }
                    }
                } else {
                    ev.csr.is_eof = true;
                }
            }
            p_csr.p_expr = Some(tree);
        }
    }

    /* Confere se o cursor passou do fim do intervalo de docids dado por `iMinDocid` e
    ** `iMaxDocid`. Se sim, liga o EOF. */
    if rc == SQLITE_OK
        && ((!p_csr.b_desc && p_csr.i_prev_id > p_csr.i_max_docid)
            || (p_csr.b_desc && p_csr.i_prev_id < p_csr.i_min_docid))
    {
        p_csr.is_eof = true;
    }

    rc
}
