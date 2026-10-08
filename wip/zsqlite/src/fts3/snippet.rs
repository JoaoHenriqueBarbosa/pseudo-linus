//! `fts3_snippet.c`: as funções `snippet()`, `offsets()` e `matchinfo()` e o `fts3ExprIterate`.
//!
//! # Desvios do C, decorrentes do modelo v2
//!
//! * **Resultado.** As funções do C gravam o resultado no `sqlite3_context`; aqui devolvem um
//!   [`Fts3Result`] que `main.rs` aplica (a conexão do contexto está emprestada enquanto a
//!   tabela está retirada). `sqlite3_result_text(..., sqlite3_free)` e `result_blob` com
//!   destrutor são cópias; um `NULL` do C (`res.z` nunca alocado) é [`Fts3Result::Null`].
//! * **Árvore.** As funções retiram a árvore de expressão do cursor (`Option::take`) e a devolvem
//!   ao fim; [`Eval`] reúne conexão, tabela, cursor e árvore.
//! * **Iteração de frases.** `sqlite3Fts3ExprIterate` com `void *pCtx` é a lista
//!   [`fts3_expr_phrases`] (as frases na ordem do C, sem o filho direito de um NOT) e um laço no
//!   chamador, que assim pode usar a avaliação (que também precisa da árvore). Para os callbacks
//!   que só tocam a árvore, [`fts3_expr_iterate`] roda `x` em cada frase.
//! * **Listas de posições** (`char *`) são cópias (`sqlite3Fts3EvalPhrasePoslist`) com deslocamento.
//! * **`MatchinfoBuffer`.** As três vagas e as contagens de referência do C existem para devolver
//!   o blob sem cópia; aqui o resultado é uma cópia, então o buffer guarda só o formato e os dados
//!   globais (a cópia de `fts3MIBufferSetGlobal`), de que cada chamada parte.

use crate::connection::Connection;
use crate::consts::{SQLITE_DONE, SQLITE_ERROR, SQLITE_NULL, SQLITE_OK};
use crate::printf::{mprintf, PrintfArg};
use crate::util::at;
use crate::vdbeapi::{column_blob, column_text, column_type, reset};

use std::rc::Rc;

use super::expr::fts3_open_tokenizer;
use super::int::{
    ExprId, Fts3Cursor, Fts3ExprTree, Fts3Table, Fts3TokenizerCursor, FTSQUERY_NOT, FTSQUERY_PHRASE,
    FTS_CORRUPT_VTAB,
};
use super::main2::reset_opt;
use super::main3::Eval;
use super::varint::{fts3_get_varint, fts3_get_varint32, fts3_get_varint_bounded};
use super::write::{
    fts3_segments_close, fts3_select_docsize, fts3_select_doctotal, sl,
};

const FTS3_MATCHINFO_NPHRASE: u8 = b'p';
const FTS3_MATCHINFO_NCOL: u8 = b'c';
const FTS3_MATCHINFO_NDOC: u8 = b'n';
const FTS3_MATCHINFO_AVGLENGTH: u8 = b'a';
const FTS3_MATCHINFO_LENGTH: u8 = b'l';
const FTS3_MATCHINFO_LCS: u8 = b's';
const FTS3_MATCHINFO_HITS: u8 = b'x';
const FTS3_MATCHINFO_LHITS: u8 = b'y';
const FTS3_MATCHINFO_LHITS_BM: u8 = b'b';
const FTS3_MATCHINFO_DEFAULT: &[u8] = b"pcx";

/// O que uma das funções SQL deixa no contexto.
#[derive(Debug, Clone)]
pub enum Fts3Result {
    /// Nada (o resultado fica `NULL`).
    Null,
    /// `sqlite3_result_text`.
    Text(Vec<u8>),
    /// `sqlite3_result_blob`.
    Blob(Vec<u8>),
    /// `sqlite3_result_error`.
    Error(Vec<u8>),
    /// `sqlite3_result_error_code`.
    ErrorCode(i32),
    /// `sqlite3_result_error_nomem`.
    Nomem,
}

/// `MatchinfoBuffer`: o formato do `matchinfo()` em cache e os dados globais.
pub struct MatchinfoBuffer {
    /// `nElem`.
    n_elem: usize,
    /// `bGlobal`: os dados globais foram carregados.
    b_global: bool,
    /// `zMatchinfo`.
    z_matchinfo: Vec<u8>,
    /// A cópia global (`fts3MIBufferSetGlobal`): o ponto de partida de cada chamada.
    a_global: Vec<u32>,
}

impl MatchinfoBuffer {
    /// `fts3MIBufferNew`.
    fn new(n_elem: usize, z_matchinfo: &[u8]) -> Self {
        MatchinfoBuffer {
            n_elem,
            b_global: false,
            z_matchinfo: z_matchinfo.to_vec(),
            a_global: vec![0u32; n_elem],
        }
    }

    /// `fts3MIBufferAlloc`: o vetor de saída desta chamada.
    fn alloc(&self) -> Vec<u32> {
        let mut a = vec![0u32; self.n_elem];
        a.copy_from_slice(&self.a_global);
        a
    }

    /// `fts3MIBufferSetGlobal`.
    fn set_global(&mut self, a_out: &[u32]) {
        self.b_global = true;
        self.a_global.copy_from_slice(&a_out[..self.n_elem.min(a_out.len())]);
    }
}

// ---------------------------------------------------------------------------------------------
// Iteração das frases
// ---------------------------------------------------------------------------------------------

/// As frases da subárvore de `root`, na ordem de `fts3ExprIterate2`: o filho esquerdo e depois o
/// direito, exceto o direito de um NOT.
pub fn fts3_expr_phrases(tree: &Fts3ExprTree, root: Option<ExprId>) -> Vec<ExprId> {
    fn rec(tree: &Fts3ExprTree, id: ExprId, v: &mut Vec<ExprId>) {
        if tree[id].e_type != FTSQUERY_PHRASE {
            if let Some(l) = tree[id].p_left {
                rec(tree, l, v);
            }
            if tree[id].e_type != FTSQUERY_NOT {
                if let Some(r) = tree[id].p_right {
                    rec(tree, r, v);
                }
            }
        } else {
            v.push(id);
        }
    }
    let mut v = Vec::new();
    if let Some(r) = root {
        rec(tree, r, &mut v);
    }
    v
}

/// `sqlite3Fts3ExprIterate`: roda `x(árvore, frase, número da frase)` em cada frase até um erro.
pub fn fts3_expr_iterate(
    tree: &mut Fts3ExprTree,
    root: ExprId,
    x: &mut dyn FnMut(&mut Fts3ExprTree, ExprId, i32) -> i32,
) -> i32 {
    let ids = fts3_expr_phrases(tree, Some(root));
    for (i, id) in ids.into_iter().enumerate() {
        let rc = x(tree, id, i as i32);
        if rc != SQLITE_OK {
            return rc;
        }
    }
    SQLITE_OK
}

/// `fts3ExprPhraseCount`: conta as frases e grava `iPhrase` em cada uma.
fn fts3_expr_phrase_count(tree: &mut Fts3ExprTree, root: Option<ExprId>) -> i32 {
    let ids = fts3_expr_phrases(tree, root);
    for (i, id) in ids.iter().enumerate() {
        tree[*id].i_phrase = i as i32;
    }
    ids.len() as i32
}

/// `fts3GetDeltaPosition`.
fn fts3_get_delta_position(buf: &[u8], pp: &mut usize, pi_pos: &mut i64) {
    let (n, i_val) = fts3_get_varint32(sl(buf, *pp));
    *pp += n as usize;
    *pi_pos += i_val as i64 - 2;
}

// ---------------------------------------------------------------------------------------------
// Texto de saída e tokenizador
// ---------------------------------------------------------------------------------------------

/// `StrBuffer`: `allocated` é o `z != NULL` do C (qualquer chamada de `fts3StringAppend` aloca).
#[derive(Default)]
struct StrBuffer {
    z: Vec<u8>,
    allocated: bool,
}

impl StrBuffer {
    /// `fts3StringAppend` com `nAppend` explícito.
    fn append(&mut self, z: &[u8]) {
        self.allocated = true;
        self.z.extend_from_slice(z);
    }

    /// `fts3StringAppend` com `nAppend` negativo: o texto C até o primeiro NUL.
    fn append_cstr(&mut self, z: &[u8]) {
        let n = z.iter().position(|&c| c == 0).unwrap_or(z.len());
        self.append(&z[..n]);
    }
}

/// `xNext` do tokenizador com as saídas em variáveis (que ficam como estavam em `SQLITE_DONE`).
fn tok_next(
    c: &mut dyn Fts3TokenizerCursor,
    i_begin: &mut i32,
    i_end: &mut i32,
    i_pos: &mut i32,
) -> i32 {
    match c.next() {
        Ok(t) => {
            *i_begin = t.i_start_offset;
            *i_end = t.i_end_offset;
            *i_pos = t.i_position;
            SQLITE_OK
        }
        Err(rc) => rc,
    }
}

// ---------------------------------------------------------------------------------------------
// snippet()
// ---------------------------------------------------------------------------------------------

/// `SnippetPhrase`.
#[derive(Default, Clone)]
struct SnippetPhrase {
    n_token: i32,
    list: Vec<u8>,
    i_head: i64,
    p_head: Option<usize>,
    i_tail: i64,
    p_tail: Option<usize>,
}

/// `SnippetIter`.
struct SnippetIter {
    i_col: i32,
    n_snippet: i32,
    a_phrase: Vec<SnippetPhrase>,
    i_current: i32,
}

/// `SnippetFragment`.
#[derive(Default, Clone, Copy)]
struct SnippetFragment {
    i_col: i32,
    i_pos: i32,
    covered: u64,
    hlmask: u64,
}

/// `fts3SnippetAdvance`.
fn fts3_snippet_advance(list: &[u8], pp_iter: &mut Option<usize>, pi_iter: &mut i64, i_next: i32) {
    if let Some(mut p) = *pp_iter {
        let mut i_iter = *pi_iter;
        let mut p_opt = Some(p);
        while i_iter < i_next as i64 {
            if (at(list, p) & 0xFE) == 0 {
                i_iter = -1;
                p_opt = None;
                break;
            }
            fts3_get_delta_position(list, &mut p, &mut i_iter);
            p_opt = Some(p);
        }
        *pi_iter = i_iter;
        *pp_iter = p_opt;
    }
}

/// `fts3SnippetNextCandidate`: verdadeiro (1) quando não há mais candidatos.
fn fts3_snippet_next_candidate(p_iter: &mut SnippetIter) -> bool {
    if p_iter.i_current < 0 {
        /* O primeiro candidato sempre começa no deslocamento 0 (mesmo com nota 0). */
        p_iter.i_current = 0;

        /* Avança o iterador "head" de cada frase até o primeiro deslocamento maior ou igual a
        ** (iNext+nSnippet). */
        let n = p_iter.n_snippet;
        for p in p_iter.a_phrase.iter_mut() {
            fts3_snippet_advance(&p.list, &mut p.p_head, &mut p.i_head, n);
        }
    } else {
        let mut i_end: i32 = 0x7FFF_FFFF;
        for p in p_iter.a_phrase.iter() {
            if p.p_head.is_some() && p.i_head < i_end as i64 {
                i_end = p.i_head as i32;
            }
        }
        if i_end == 0x7FFF_FFFF {
            return true;
        }

        let i_start = i_end - p_iter.n_snippet + 1;
        p_iter.i_current = i_start;
        for p in p_iter.a_phrase.iter_mut() {
            fts3_snippet_advance(&p.list, &mut p.p_head, &mut p.i_head, i_end + 1);
            fts3_snippet_advance(&p.list, &mut p.p_tail, &mut p.i_tail, i_start);
        }
    }
    false
}

/// `fts3SnippetDetails`: (primeiro token, nota, frases cobertas, termos a destacar).
fn fts3_snippet_details(p_iter: &SnippetIter, m_covered: u64) -> (i32, i32, u64, u64) {
    let i_start = p_iter.i_current;
    let mut i_score = 0;
    let mut m_cover: u64 = 0;
    let mut m_highlight: u64 = 0;

    for (i, p_phrase) in p_iter.a_phrase.iter().enumerate() {
        if let Some(mut p_csr) = p_phrase.p_tail {
            let mut i_csr = p_phrase.i_tail;

            while i_csr < (i_start + p_iter.n_snippet) as i64 && i_csr >= i_start as i64 {
                let m_phrase: u64 = 1u64 << (i % 64);
                let m_pos: u64 = 1u64.checked_shl((i_csr - i_start as i64) as u32).unwrap_or(0);

                if ((m_cover | m_covered) & m_phrase) != 0 {
                    i_score += 1;
                } else {
                    i_score += 1000;
                }
                m_cover |= m_phrase;

                let mut j = 0;
                while j < p_phrase.n_token && j < p_iter.n_snippet {
                    m_highlight |= m_pos >> j;
                    j += 1;
                }

                if (at(&p_phrase.list, p_csr) & 0x0FE) == 0 {
                    break;
                }
                fts3_get_delta_position(&p_phrase.list, &mut p_csr, &mut i_csr);
            }
        }
    }

    (i_start, i_score, m_cover, m_highlight)
}

/// `fts3BestSnippet`: o melhor trecho de `n_snippet` tokens da coluna `i_col`.
fn fts3_best_snippet(
    ev: &mut Eval<'_>,
    n_snippet: i32,
    i_col: i32,
    m_covered: u64,
    pm_seen: &mut u64,
    p_fragment: &mut SnippetFragment,
    pi_score: &mut i32,
) -> i32 {
    let mut i_best_score = -1;
    let root = ev.tree.root;

    /* As frases da expressão (a carga das doclists do C não faz nada além de contar). */
    let phrases = fts3_expr_phrases(ev.tree, root);
    let n_list = phrases.len();

    let mut s_iter = SnippetIter {
        i_col,
        n_snippet,
        a_phrase: vec![SnippetPhrase::default(); n_list],
        i_current: -1,
    };

    /* Preenche o vetor aPhrase[] (`fts3SnippetFindPositions`). */
    let mut rc = SQLITE_OK;
    for (i_phrase, &id) in phrases.iter().enumerate() {
        let n_token = ev.tree[id].p_phrase.as_ref().map_or(0, |p| p.n_token());
        s_iter.a_phrase[i_phrase].n_token = n_token;
        match ev.phrase_poslist(id, s_iter.i_col) {
            Err(e) => {
                rc = e;
                break;
            }
            Ok(Some(list)) => {
                let mut p = 0usize;
                let mut i_first: i64 = 0;
                fts3_get_delta_position(&list, &mut p, &mut i_first);
                if i_first < 0 {
                    rc = FTS_CORRUPT_VTAB;
                    break;
                }
                let ph = &mut s_iter.a_phrase[i_phrase];
                ph.list = list;
                ph.p_head = Some(p);
                ph.p_tail = Some(p);
                ph.i_head = i_first;
                ph.i_tail = i_first;
            }
            Ok(None) => {}
        }
    }

    if rc == SQLITE_OK {
        /* Grava `*pmSeen`. */
        for (i, p) in s_iter.a_phrase.iter().enumerate() {
            if p.p_head.is_some() {
                *pm_seen |= 1u64 << (i % 64);
            }
        }

        /* Percorre todos os trechos candidatos e guarda o melhor em `*pFragment`. */
        p_fragment.i_col = i_col;
        while !fts3_snippet_next_candidate(&mut s_iter) {
            let (i_pos, i_score, m_cover, m_highlite) = fts3_snippet_details(&s_iter, m_covered);
            if i_score > i_best_score {
                p_fragment.i_pos = i_pos;
                p_fragment.hlmask = m_highlite;
                p_fragment.covered = m_cover;
                i_best_score = i_score;
            }
        }
        *pi_score = i_best_score;
    }
    rc
}

/// `fts3SnippetShift`.
fn fts3_snippet_shift(
    p_tab: &Fts3Table,
    i_langid: i32,
    n_snippet: i32,
    z_doc: &[u8],
    pi_pos: &mut i32,
    p_hlmask: &mut u64,
) -> i32 {
    let hlmask = *p_hlmask;
    if hlmask != 0 {
        let bit = |n: i32| -> u64 { 1u64.checked_shl(n.max(0) as u32).unwrap_or(0) };
        let mut n_left = 0i32;
        while n_left < 64 && (hlmask & bit(n_left)) == 0 {
            n_left += 1;
        }
        let mut n_right = 0i32;
        while n_right < 64 && n_snippet - 1 - n_right >= 0 && (hlmask & bit(n_snippet - 1 - n_right)) == 0 {
            n_right += 1;
        }
        let n_desired = (n_left - n_right) / 2;

        /* O início do trecho idealmente avança nDesired tokens no documento. Confere se há
        ** mesmo nDesired tokens à direita; senão, avança o que houver. */
        if n_desired > 0 {
            let mut i_current = 0i32;
            let Some(tok) = p_tab.p_tokenizer.as_ref() else { return SQLITE_ERROR };

            /* Abre um cursor sobre zDoc e vê se há (nSnippet+nDesired) tokens ou mais. */
            let mut p_c = match fts3_open_tokenizer(&**tok, i_langid, z_doc) {
                Ok(c) => c,
                Err(rc) => return rc,
            };
            let mut rc = SQLITE_OK;
            let (mut b, mut e) = (0, 0);
            while rc == SQLITE_OK && i_current < (n_snippet + n_desired) {
                rc = tok_next(&mut *p_c, &mut b, &mut e, &mut i_current);
            }
            drop(p_c);
            if rc != SQLITE_OK && rc != SQLITE_DONE {
                return rc;
            }
            let n_shift = (rc == SQLITE_DONE) as i32 + i_current - n_snippet;
            if n_shift > 0 {
                *pi_pos += n_shift;
                *p_hlmask = hlmask >> n_shift;
            }
        }
    }
    SQLITE_OK
}

/// `fts3SnippetText`: acrescenta a `p_out` o texto do trecho.
fn fts3_snippet_text(
    ev: &mut Eval<'_>,
    p_fragment: &SnippetFragment,
    i_fragment: i32,
    is_last: bool,
    n_snippet: i32,
    z_open: &[u8],
    z_close: &[u8],
    z_ellipsis: &[u8],
    p_out: &mut StrBuffer,
) -> i32 {
    let mut i_current = 0i32;
    let mut i_end: i32 = 0;
    let mut is_shift_done = false;
    let mut i_pos = p_fragment.i_pos;
    let mut hlmask = p_fragment.hlmask;
    let i_col = p_fragment.i_col + 1; /* a coluna da consulta de que extrair o texto */
    let mut rc;

    let Some(st) = ev.csr.p_stmt else { return SQLITE_OK };
    let z_doc: Vec<u8> = match column_text(ev.db, st, i_col) {
        Some(d) => d.to_vec(),
        None => {
            if column_type(ev.db, st, i_col) != SQLITE_NULL {
                return crate::consts::SQLITE_NOMEM;
            }
            return SQLITE_OK;
        }
    };

    /* Abre um cursor de tokens sobre o documento. */
    let i_langid = ev.csr.i_langid;
    let Some(tok) = ev.p.p_tokenizer.clone() else { return SQLITE_ERROR };
    let mut p_c = match fts3_open_tokenizer(&*tok, i_langid, &z_doc) {
        Ok(c) => c,
        Err(rc) => return rc,
    };
    rc = SQLITE_OK;
    while rc == SQLITE_OK {
        let mut i_begin = 0i32; /* deslocamento em zDoc do começo do token */
        let mut i_fin = 0i32; /* deslocamento em zDoc do fim do token */

        rc = tok_next(&mut *p_c, &mut i_begin, &mut i_fin, &mut i_current);
        if rc != SQLITE_OK {
            if rc == SQLITE_DONE {
                /* Caso especial: o último token do trecho também é o último da coluna. Acrescenta
                ** a pontuação entre o fim do token anterior e o fim do documento, e sai. */
                p_out.append_cstr(sl(&z_doc, i_end.max(0) as usize));
                rc = SQLITE_OK;
            }
            break;
        }

        if i_current < i_pos {
            continue;
        }

        if !is_shift_done {
            rc = fts3_snippet_shift(
                ev.p,
                i_langid,
                n_snippet,
                sl(&z_doc, i_begin.max(0) as usize),
                &mut i_pos,
                &mut hlmask,
            );
            is_shift_done = true;

            /* Feito o deslocamento, confere se as reticências iniciais são precisas: se este
            ** não é o primeiro trecho ou se não começa na posição 0 da coluna. */
            if rc == SQLITE_OK {
                if i_pos > 0 || i_fragment > 0 {
                    p_out.append_cstr(z_ellipsis);
                } else if i_begin != 0 {
                    p_out.append(&z_doc[..(i_begin.max(0) as usize).min(z_doc.len())]);
                }
            }

            if rc != SQLITE_OK || i_current < i_pos {
                continue;
            }
        }

        if i_current >= (i_pos + n_snippet) {
            if is_last {
                p_out.append_cstr(z_ellipsis);
            }
            break;
        }

        /* `isHighlight` é verdadeiro se este termo deve ser destacado. */
        let is_highlight = (hlmask & 1u64.checked_shl((i_current - i_pos) as u32).unwrap_or(0)) != 0;

        if i_current > i_pos {
            let n = i_begin - i_end;
            if n > 0 {
                let a = (i_end.max(0) as usize).min(z_doc.len());
                let b = (a + n as usize).min(z_doc.len());
                p_out.append(&z_doc[a..b]);
            } else {
                p_out.append(&[]);
            }
        }
        if is_highlight {
            p_out.append_cstr(z_open);
        }
        {
            let a = (i_begin.max(0) as usize).min(z_doc.len());
            let b = (i_fin.max(0) as usize).min(z_doc.len()).max(a);
            p_out.append(&z_doc[a..b]);
        }
        if is_highlight {
            p_out.append_cstr(z_close);
        }
        i_end = i_fin;
    }

    rc
}

/// `sqlite3Fts3Snippet`.
pub fn fts3_snippet(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_csr: &mut Fts3Cursor,
    z_start: &[u8],
    z_end: &[u8],
    z_ellipsis: &[u8],
    i_col: i32,
    n_token: i32,
) -> Fts3Result {
    let Some(mut tree) = p_csr.p_expr.take() else {
        return Fts3Result::Text(Vec::new());
    };
    let mut rc = SQLITE_OK;
    let mut res = StrBuffer::default();
    let mut n_token = n_token;

    /* O texto devolvido tem até quatro trechos extraídos da linha corrente. A primeira volta do
    ** laço abaixo procura um único trecho de nToken tokens com ao menos uma instância de todas as
    ** frases da consulta que aparecem na linha; se não acha, a segunda procura um par de
    ** trechos, e assim por diante. */
    let n_col = p.n_column();
    {
        let mut ev = Eval { db: &mut *db, p: &mut *p, csr: &mut *p_csr, tree: &mut tree };
        let mut a_snippet: [SnippetFragment; 4] = [SnippetFragment::default(); 4];
        let mut n_snippet: i32 = 1;
        let mut n_ftoken: i32;

        /* Limita o tamanho do trecho a 64 tokens. */
        if n_token < -64 {
            n_token = -64;
        }
        if n_token > 64 {
            n_token = 64;
        }

        'outer: {
            loop {
                let mut m_covered: u64 = 0; /* máscara das frases cobertas */
                let mut m_seen: u64 = 0; /* máscara das frases vistas por BestSnippet() */

                if n_token >= 0 {
                    n_ftoken = (n_token + n_snippet - 1) / n_snippet;
                } else {
                    n_ftoken = -n_token;
                }

                for i_snip in 0..n_snippet as usize {
                    let mut i_best_score = -1;
                    let mut fragment = SnippetFragment::default();

                    /* Percorre as colunas da tabela; com iCol negativo, todas. */
                    for i_read in 0..n_col {
                        let mut s_f = SnippetFragment::default();
                        let mut i_s = 0;
                        if i_col >= 0 && i_read != i_col {
                            continue;
                        }

                        /* Acha o melhor trecho de nFToken tokens na coluna iRead. */
                        rc = fts3_best_snippet(&mut ev, n_ftoken, i_read, m_covered, &mut m_seen, &mut s_f, &mut i_s);
                        if rc != SQLITE_OK {
                            break 'outer;
                        }
                        if i_s > i_best_score {
                            fragment = s_f;
                            i_best_score = i_s;
                        }
                    }
                    m_covered |= fragment.covered;
                    a_snippet[i_snip] = fragment;
                }

                /* Se todas as frases vistas estão em ao menos um trecho, sai. */
                if m_seen == m_covered || n_snippet == a_snippet.len() as i32 {
                    break;
                }
                n_snippet += 1;
            }

            let mut i = 0;
            while i < n_snippet && rc == SQLITE_OK {
                rc = fts3_snippet_text(
                    &mut ev,
                    &a_snippet[i as usize],
                    i,
                    i == n_snippet - 1,
                    n_ftoken,
                    z_start,
                    z_end,
                    z_ellipsis,
                    &mut res,
                );
                i += 1;
            }
        }
    }
    p_csr.p_expr = Some(tree);

    fts3_segments_close(db, p);
    if rc != SQLITE_OK {
        Fts3Result::ErrorCode(rc)
    } else if !res.allocated {
        Fts3Result::Null
    } else {
        let n = res.z.iter().position(|&c| c == 0).unwrap_or(res.z.len());
        res.z.truncate(n);
        Fts3Result::Text(res.z)
    }
}

// ---------------------------------------------------------------------------------------------
// offsets()
// ---------------------------------------------------------------------------------------------

/// `TermOffset`.
#[derive(Default, Clone)]
struct TermOffset {
    /// A lista de posições (compartilhada pelos tokens da frase) e o deslocamento nela.
    list: Option<Rc<Vec<u8>>>,
    off: usize,
    /// `iPos`: a posição lida.
    i_pos: i64,
    /// `iOff`: o deslocamento deste termo em relação às posições lidas.
    i_off: i64,
}

/// `sqlite3Fts3Offsets`.
pub fn fts3_offsets(db: &mut Connection, p: &mut Fts3Table, p_csr: &mut Fts3Cursor) -> Fts3Result {
    let Some(mut tree) = p_csr.p_expr.take() else {
        return Fts3Result::Text(Vec::new());
    };
    let mut rc = SQLITE_OK;
    let mut res = StrBuffer::default();
    let n_col = p.n_column();
    let tokenizer = p.p_tokenizer.clone();
    let z_content_tbl = p.z_content_tbl.is_some();

    {
        let mut ev = Eval { db: &mut *db, p: &mut *p, csr: &mut *p_csr, tree: &mut tree };
        debug_assert!(!ev.csr.is_require_seek);
        let root = ev.tree.root;
        let phrases = fts3_expr_phrases(ev.tree, root);

        /* Conta os termos da consulta. */
        let n_token: usize = phrases
            .iter()
            .map(|&id| ev.tree[id].p_phrase.as_ref().map_or(0, |p| p.n_token() as usize))
            .sum();
        let mut a_term: Vec<TermOffset> = vec![TermOffset::default(); n_token];

        /* Percorre as colunas da tabela e acrescenta a `res` as informações de deslocamento de
        ** cada uma. */
        'cols: for i_col in 0..n_col {
            /* Inicia sCtx.aTerm[] para a coluna iCol. Isso pode falhar com registros corrompidos. */
            let mut i_term = 0usize;
            for &id in phrases.iter() {
                let r = ev.phrase_poslist(id, i_col);
                let n_term = ev.tree[id].p_phrase.as_ref().map_or(0, |p| p.n_token());
                let (list, off, i_pos) = match &r {
                    Ok(Some(l)) => {
                        let mut off = 0usize;
                        let mut i_pos: i64 = 0;
                        fts3_get_delta_position(l, &mut off, &mut i_pos);
                        (Some(Rc::new(l.clone())), off, i_pos)
                    }
                    _ => (None, 0, 0),
                };
                for i_t in 0..n_term {
                    if let Some(t) = a_term.get_mut(i_term) {
                        t.i_off = (n_term - i_t - 1) as i64;
                        t.list = list.clone();
                        t.off = off;
                        t.i_pos = i_pos;
                    }
                    i_term += 1;
                }
                if let Err(e) = r {
                    rc = e;
                    break 'cols;
                }
            }

            /* O texto da coluna iCol. Com NULL, passa à próxima volta. */
            let Some(st) = ev.csr.p_stmt else { continue };
            let z_doc: Vec<u8> = match column_text(ev.db, st, i_col + 1) {
                Some(d) => d.to_vec(),
                None => {
                    if column_type(ev.db, st, i_col + 1) == SQLITE_NULL {
                        continue;
                    }
                    rc = crate::consts::SQLITE_NOMEM;
                    break 'cols;
                }
            };

            /* Inicia um iterador de tokens sobre a coluna iCol. */
            let Some(tok) = tokenizer.as_ref() else {
                rc = SQLITE_ERROR;
                break 'cols;
            };
            let mut p_c = match fts3_open_tokenizer(&**tok, ev.csr.i_langid, &z_doc) {
                Ok(c) => c,
                Err(e) => {
                    rc = e;
                    break 'cols;
                }
            };
            let (mut i_start, mut i_end, mut i_current) = (0i32, 0i32, 0i32);
            rc = tok_next(&mut *p_c, &mut i_start, &mut i_end, &mut i_current);
            while rc == SQLITE_OK {
                let mut i_min_pos: i32 = 0x7FFF_FFFF; /* posição do próximo token */
                let mut p_term: Option<usize> = None; /* o TermOffset do próximo token */

                for (i, t) in a_term.iter().enumerate() {
                    if t.list.is_some() && ((t.i_pos - t.i_off) < i_min_pos as i64) {
                        i_min_pos = (t.i_pos - t.i_off) as i32;
                        p_term = Some(i);
                    }
                }

                match p_term {
                    None => {
                        /* Todos os deslocamentos desta coluna foram juntados. */
                        rc = SQLITE_DONE;
                    }
                    Some(pt) => {
                        {
                            let t = &mut a_term[pt];
                            let list = t.list.clone().unwrap_or_default();
                            if (0xFE & at(&list, t.off)) == 0 {
                                t.list = None;
                            } else {
                                fts3_get_delta_position(&list, &mut t.off, &mut t.i_pos);
                            }
                        }
                        while rc == SQLITE_OK && i_current < i_min_pos {
                            rc = tok_next(&mut *p_c, &mut i_start, &mut i_end, &mut i_current);
                        }
                        if rc == SQLITE_OK {
                            if let Some(s) = mprintf(
                                b"%d %d %d %d ",
                                &[
                                    PrintfArg::Int(i_col as i64),
                                    PrintfArg::Int(pt as i64),
                                    PrintfArg::Int(i_start as i64),
                                    PrintfArg::Int((i_end - i_start) as i64),
                                ],
                            ) {
                                let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
                                res.append(&s[..n]);
                            }
                        } else if rc == SQLITE_DONE && !z_content_tbl {
                            rc = FTS_CORRUPT_VTAB;
                        }
                    }
                }
            }
            if rc == SQLITE_DONE {
                rc = SQLITE_OK;
            }
            drop(p_c);
            if rc != SQLITE_OK {
                break 'cols;
            }
        }
    }
    p_csr.p_expr = Some(tree);

    debug_assert!(rc != SQLITE_DONE);
    fts3_segments_close(db, p);
    if rc != SQLITE_OK {
        Fts3Result::ErrorCode(rc)
    } else if !res.allocated {
        Fts3Result::Null
    } else {
        /* `res.n-1`: sem o espaço final. */
        res.z.pop();
        Fts3Result::Text(res.z)
    }
}

// ---------------------------------------------------------------------------------------------
// matchinfo()
// ---------------------------------------------------------------------------------------------

/// `MatchInfo`.
struct MatchInfo {
    n_col: i32,
    n_phrase: i32,
    n_doc: i64,
    flag: u8,
    /// O deslocamento de `aMatchinfo` em `a_out` (o C avança o ponteiro).
    off: usize,
}

/// Grava `a[idx]` se existe.
fn set_at(a: &mut [u32], idx: usize, v: u32) {
    if let Some(s) = a.get_mut(idx) {
        *s = v;
    }
}

/// `fts3ColumnlistCount`: o número de entradas de uma lista de coluna; `*pp` vai ao terminador.
fn fts3_columnlist_count(buf: &[u8], pp: &mut usize) -> i32 {
    let mut p_end = *pp;
    let mut c: u8 = 0;
    let mut n_entry = 0;

    /* Uma lista de coluna termina com 0x01 ou 0x00. */
    while (0xFE & (at(buf, p_end) | c)) != 0 {
        c = at(buf, p_end) & 0x80;
        p_end += 1;
        if c == 0 {
            n_entry += 1;
        }
    }

    *pp = p_end;
    n_entry
}

/// `fts3ExprLHits`.
fn fts3_expr_lhits(ev: &Eval<'_>, id: ExprId, p: &MatchInfo, a_out: &mut [u32]) -> i32 {
    let n_column = ev.p.n_column();
    let Some(p_phrase) = ev.tree[id].p_phrase.as_ref() else { return SQLITE_OK };
    let i_start: usize = if p.flag == FTS3_MATCHINFO_LHITS {
        (ev.tree[id].i_phrase * p.n_col) as usize
    } else {
        (ev.tree[id].i_phrase * ((p.n_col + 31) / 32)) as usize
    };
    let mut i_col: i32 = 0;

    if p_phrase.doclist.p_list.is_some() {
        let buf = p_phrase.doclist.list_tail();
        let mut p_iter = 0usize;
        loop {
            let n_hit = fts3_columnlist_count(buf, &mut p_iter);
            if p_phrase.i_column >= n_column || p_phrase.i_column == i_col {
                if p.flag == FTS3_MATCHINFO_LHITS {
                    set_at(a_out, p.off + i_start + i_col as usize, n_hit as u32);
                } else if n_hit != 0 {
                    let idx = p.off + i_start + ((i_col + 1) / 32) as usize;
                    if let Some(s) = a_out.get_mut(idx) {
                        *s |= 1u32 << (i_col & 0x1F);
                    }
                }
            }

            if at(buf, p_iter) != 0x01 {
                break;
            }
            p_iter += 1;
            let (n, v) = fts3_get_varint32(sl(buf, p_iter));
            p_iter += n as usize;
            i_col = v;
            if i_col >= p.n_col {
                return FTS_CORRUPT_VTAB;
            }
        }
    }
    SQLITE_OK
}

/// `fts3ExprLHitGather`.
fn fts3_expr_lhit_gather(ev: &Eval<'_>, id: ExprId, p: &MatchInfo, a_out: &mut [u32]) -> i32 {
    let mut rc = SQLITE_OK;
    if !ev.tree[id].b_eof && ev.tree[id].i_docid == ev.csr.i_prev_id {
        if let Some(l) = ev.tree[id].p_left {
            rc = fts3_expr_lhit_gather(ev, l, p, a_out);
            if rc == SQLITE_OK {
                if let Some(r) = ev.tree[id].p_right {
                    rc = fts3_expr_lhit_gather(ev, r, p, a_out);
                }
            }
        } else {
            rc = fts3_expr_lhits(ev, id, p, a_out);
        }
    }
    rc
}

/// `fts3MatchinfoCheck`: a mensagem de erro se o pedido não é reconhecido.
fn fts3_matchinfo_check(p_tab: &Fts3Table, c_arg: u8) -> Result<(), Vec<u8>> {
    if c_arg == FTS3_MATCHINFO_NPHRASE
        || c_arg == FTS3_MATCHINFO_NCOL
        || (c_arg == FTS3_MATCHINFO_NDOC && p_tab.b_fts4)
        || (c_arg == FTS3_MATCHINFO_AVGLENGTH && p_tab.b_fts4)
        || (c_arg == FTS3_MATCHINFO_LENGTH && p_tab.b_has_docsize)
        || c_arg == FTS3_MATCHINFO_LCS
        || c_arg == FTS3_MATCHINFO_HITS
        || c_arg == FTS3_MATCHINFO_LHITS
        || c_arg == FTS3_MATCHINFO_LHITS_BM
    {
        return Ok(());
    }
    Err(mprintf(b"unrecognized matchinfo request: %c", &[PrintfArg::Char(c_arg as u32)]).unwrap_or_default())
}

/// `fts3MatchinfoSize`.
fn fts3_matchinfo_size(p_info: &MatchInfo, c_arg: u8) -> usize {
    let n_col = p_info.n_col.max(0) as usize;
    let n_phrase = p_info.n_phrase.max(0) as usize;
    match c_arg {
        FTS3_MATCHINFO_NDOC | FTS3_MATCHINFO_NPHRASE | FTS3_MATCHINFO_NCOL => 1,
        FTS3_MATCHINFO_AVGLENGTH | FTS3_MATCHINFO_LENGTH | FTS3_MATCHINFO_LCS => n_col,
        FTS3_MATCHINFO_LHITS => n_col * n_phrase,
        FTS3_MATCHINFO_LHITS_BM => n_phrase * ((n_col + 31) / 32),
        _ => n_col * n_phrase * 3,
    }
}

/// `fts3MatchinfoSelectDoctotal`: o número de documentos e o resto do blob (depois do primeiro
/// varint).
fn fts3_matchinfo_select_doctotal(
    db: &mut Connection,
    p_tab: &mut Fts3Table,
    pp_stmt: &mut Option<crate::connection::StmtId>,
) -> Result<(i64, Vec<u8>), i32> {
    if pp_stmt.is_none() {
        *pp_stmt = Some(fts3_select_doctotal(db, p_tab)?);
    }
    let Some(st) = *pp_stmt else { return Err(FTS_CORRUPT_VTAB) };
    let blob: Vec<u8> = match column_blob(db, st, 0) {
        Some(b) if !b.is_empty() => b.to_vec(),
        _ => return Err(FTS_CORRUPT_VTAB),
    };
    let (n, n_doc) = fts3_get_varint_bounded(&blob);
    let a = n as usize;
    if n_doc <= 0 || a > blob.len() {
        return Err(FTS_CORRUPT_VTAB);
    }
    Ok((n_doc, blob[a..].to_vec()))
}

/// `LcsIterator`.
#[derive(Default, Clone)]
struct LcsIterator {
    expr: Option<ExprId>,
    i_pos_offset: i32,
    list: Vec<u8>,
    p_read: Option<usize>,
    i_pos: i32,
}

/// `fts3LcsIteratorAdvance`: verdadeiro se o iterador chegou ao fim.
fn fts3_lcs_iterator_advance(p_iter: &mut LcsIterator) -> bool {
    let Some(p) = p_iter.p_read else { return true };
    let (n, i_read) = fts3_get_varint(sl(&p_iter.list, p));
    if i_read == 0 || i_read == 1 {
        p_iter.p_read = None;
        true
    } else {
        p_iter.i_pos = p_iter.i_pos.wrapping_add((i_read - 2) as i32);
        p_iter.p_read = Some(p + n as usize);
        false
    }
}

/// `fts3MatchinfoLcs`.
fn fts3_matchinfo_lcs(ev: &mut Eval<'_>, p_info: &MatchInfo, a_out: &mut [u32]) -> i32 {
    let mut n_token = 0i32;
    let mut rc = SQLITE_OK;
    let root = ev.tree.root;
    let phrases = fts3_expr_phrases(ev.tree, root);

    /* O vetor de LcsIterator tem um elemento para cada frase da consulta. */
    let mut a_iter: Vec<LcsIterator> = vec![LcsIterator::default(); ev.csr.n_phrase.max(0) as usize];
    for (i, &id) in phrases.iter().enumerate() {
        if let Some(it) = a_iter.get_mut(i) {
            it.expr = Some(id);
        }
    }

    for i in 0..p_info.n_phrase.max(0) as usize {
        let Some(it) = a_iter.get_mut(i) else { break };
        if let Some(id) = it.expr {
            n_token -= ev.tree[id].p_phrase.as_ref().map_or(0, |p| p.n_token());
        }
        it.i_pos_offset = n_token;
    }

    for i_col in 0..p_info.n_col {
        let mut n_lcs = 0i32; /* o valor LCS desta coluna */
        let mut n_live = 0i32; /* iteradores que não estão no fim */

        for i in 0..p_info.n_phrase.max(0) as usize {
            let Some(id) = a_iter.get(i).and_then(|it| it.expr) else { continue };
            match ev.phrase_poslist(id, i_col) {
                Err(e) => return e,
                Ok(list) => {
                    let it = &mut a_iter[i];
                    it.p_read = None;
                    if let Some(l) = list {
                        it.list = l;
                        it.p_read = Some(0);
                        it.i_pos = it.i_pos_offset;
                        fts3_lcs_iterator_advance(it);
                        if it.p_read.is_none() {
                            rc = FTS_CORRUPT_VTAB;
                            return rc;
                        }
                        n_live += 1;
                    }
                }
            }
        }

        while n_live > 0 {
            let mut p_adv: Option<usize> = None; /* o iterador a avançar uma posição */
            let mut n_this_lcs = 0i32; /* o LCS das posições correntes dos iteradores */

            for i in 0..p_info.n_phrase.max(0) as usize {
                if a_iter[i].p_read.is_none() {
                    /* Este iterador já está no fim nesta coluna. */
                    n_this_lcs = 0;
                } else {
                    if p_adv.map_or(true, |a| a_iter[i].i_pos < a_iter[a].i_pos) {
                        p_adv = Some(i);
                    }
                    if n_this_lcs == 0 || (i > 0 && a_iter[i].i_pos == a_iter[i - 1].i_pos) {
                        n_this_lcs += 1;
                    } else {
                        n_this_lcs = 1;
                    }
                    if n_this_lcs > n_lcs {
                        n_lcs = n_this_lcs;
                    }
                }
            }
            if let Some(adv) = p_adv {
                if fts3_lcs_iterator_advance(&mut a_iter[adv]) {
                    n_live -= 1;
                }
            } else {
                break;
            }
        }
        set_at(a_out, p_info.off + i_col as usize, n_lcs as u32);
    }
    rc
}

/// `fts3MatchinfoValues`.
fn fts3_matchinfo_values(
    ev: &mut Eval<'_>,
    b_global: bool,
    p_info: &mut MatchInfo,
    z_arg: &[u8],
    a_out: &mut [u32],
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut p_select: Option<crate::connection::StmtId> = None;
    let root = ev.tree.root;

    let mut i = 0usize;
    while rc == SQLITE_OK && i < z_arg.len() {
        let c = z_arg[i];
        p_info.flag = c;
        match c {
            FTS3_MATCHINFO_NPHRASE => {
                if b_global {
                    set_at(a_out, p_info.off, p_info.n_phrase as u32);
                }
            }
            FTS3_MATCHINFO_NCOL => {
                if b_global {
                    set_at(a_out, p_info.off, p_info.n_col as u32);
                }
            }
            FTS3_MATCHINFO_NDOC => {
                if b_global {
                    let mut n_doc = 0i64;
                    match fts3_matchinfo_select_doctotal(ev.db, ev.p, &mut p_select) {
                        Ok((n, _)) => n_doc = n,
                        Err(e) => rc = e,
                    }
                    set_at(a_out, p_info.off, n_doc as u32);
                }
            }
            FTS3_MATCHINFO_AVGLENGTH => {
                if b_global {
                    match fts3_matchinfo_select_doctotal(ev.db, ev.p, &mut p_select) {
                        Err(e) => rc = e,
                        Ok((n_doc, a)) => {
                            let mut pos = 0usize;
                            for i_col in 0..p_info.n_col {
                                let (n, n_token) = fts3_get_varint(sl(&a, pos));
                                pos += n as usize;
                                if pos > a.len() {
                                    rc = crate::consts::SQLITE_CORRUPT_VTAB;
                                    break;
                                }
                                let i_val = (((n_token & 0xffff_ffff) as u32 as i64 + n_doc / 2) / n_doc) as u32;
                                set_at(a_out, p_info.off + i_col as usize, i_val);
                            }
                        }
                    }
                }
            }
            FTS3_MATCHINFO_LENGTH => {
                let i_prev = ev.csr.i_prev_id;
                match fts3_select_docsize(ev.db, ev.p, i_prev) {
                    Err(e) => rc = e,
                    Ok(st) => {
                        let blob: Vec<u8> = column_blob(ev.db, st, 0).map(|b| b.to_vec()).unwrap_or_default();
                        let mut pos = 0usize;
                        for i_col in 0..p_info.n_col {
                            let (n, n_token) = fts3_get_varint_bounded(sl(&blob, pos));
                            pos += n as usize;
                            if pos > blob.len() {
                                rc = crate::consts::SQLITE_CORRUPT_VTAB;
                                break;
                            }
                            set_at(a_out, p_info.off + i_col as usize, n_token as u32);
                        }
                        reset(ev.db, st);
                    }
                }
            }
            FTS3_MATCHINFO_LCS => {
                rc = fts3_matchinfo_lcs(ev, p_info, a_out);
            }
            FTS3_MATCHINFO_LHITS_BM | FTS3_MATCHINFO_LHITS => {
                let n_zero = fts3_matchinfo_size(p_info, c);
                for k in 0..n_zero {
                    set_at(a_out, p_info.off + k, 0);
                }
                if let Some(r) = root {
                    rc = fts3_expr_lhit_gather(ev, r, p_info, a_out);
                }
            }
            _ => {
                debug_assert!(c == FTS3_MATCHINFO_HITS);
                let phrases = fts3_expr_phrases(ev.tree, root);
                if b_global {
                    if !ev.csr.p_deferred.is_empty() {
                        match fts3_matchinfo_select_doctotal(ev.db, ev.p, &mut p_select) {
                            Ok((n, _)) => p_info.n_doc = n,
                            Err(e) => {
                                rc = e;
                                break;
                            }
                        }
                    }
                    for (i_phrase, &id) in phrases.iter().enumerate() {
                        let base = p_info.off + 3 * i_phrase * p_info.n_col.max(0) as usize;
                        if base <= a_out.len() {
                            rc = ev.phrase_stats(id, &mut a_out[base..]);
                        } else {
                            rc = SQLITE_ERROR;
                        }
                        if rc != SQLITE_OK {
                            break;
                        }
                    }
                    if rc == SQLITE_OK {
                        ev.test_deferred(&mut rc);
                    }
                    if rc != SQLITE_OK {
                        break;
                    }
                }
                /* Os acertos locais: o `rc` do C é descartado. */
                let n_col = p_info.n_col;
                for (i_phrase, &id) in phrases.iter().enumerate() {
                    let i_start = p_info.off + i_phrase * n_col.max(0) as usize * 3;
                    let mut rc2 = SQLITE_OK;
                    let mut i_c = 0;
                    while i_c < n_col && rc2 == SQLITE_OK {
                        match ev.phrase_poslist(id, i_c) {
                            Err(e) => rc2 = e,
                            Ok(Some(l)) => {
                                let mut q = 0usize;
                                let n = fts3_columnlist_count(&l, &mut q);
                                set_at(a_out, i_start + i_c as usize * 3, n as u32);
                            }
                            Ok(None) => set_at(a_out, i_start + i_c as usize * 3, 0),
                        }
                        i_c += 1;
                    }
                    if rc2 != SQLITE_OK {
                        break;
                    }
                }
            }
        }
        p_info.off += fts3_matchinfo_size(p_info, c);
        i += 1;
    }

    reset_opt(ev.db, p_select);
    rc
}

/// `fts3GetMatchinfo`.
fn fts3_get_matchinfo(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_csr: &mut Fts3Cursor,
    z_arg: &[u8],
) -> Fts3Result {
    let mut s_info = MatchInfo { n_col: p.n_column(), n_phrase: 0, n_doc: 0, flag: 0, off: 0 };
    let mut b_global = false; /* junta as estatísticas globais além das locais */
    let mut tree = p_csr.p_expr.take().unwrap_or_default();

    /* Se há dados em cache de um formato diferente do pedido, descarta-os. */
    if p_csr.p_mi_buffer.as_ref().is_some_and(|b| b.z_matchinfo != z_arg) {
        p_csr.p_mi_buffer = None;
    }

    /* Se `pMIBuffer` é nulo, é a primeira chamada de matchinfo desta consulta: aloca o vetor e
    ** inicia os elementos constantes em toda linha. */
    if p_csr.p_mi_buffer.is_none() {
        let mut n_matchinfo = 0usize;

        let root = tree.root;
        p_csr.n_phrase = fts3_expr_phrase_count(&mut tree, root);
        s_info.n_phrase = p_csr.n_phrase;

        for &c in z_arg {
            if let Err(z_err) = fts3_matchinfo_check(p, c) {
                p_csr.p_expr = Some(tree);
                return Fts3Result::Error(z_err);
            }
            n_matchinfo += fts3_matchinfo_size(&s_info, c);
        }

        p_csr.p_mi_buffer = Some(MatchinfoBuffer::new(n_matchinfo, z_arg));
        p_csr.is_matchinfo_needed = true;
        b_global = true;
    }

    let mut a_out = p_csr.p_mi_buffer.as_ref().map_or_else(Vec::new, |b| b.alloc());
    s_info.n_phrase = p_csr.n_phrase;
    let rc = {
        let mut ev = Eval { db: &mut *db, p: &mut *p, csr: &mut *p_csr, tree: &mut tree };
        fts3_matchinfo_values(&mut ev, b_global, &mut s_info, z_arg, &mut a_out)
    };
    if b_global {
        if let Some(b) = p_csr.p_mi_buffer.as_mut() {
            b.set_global(&a_out);
        }
    }
    p_csr.p_expr = Some(tree);

    if rc != SQLITE_OK {
        Fts3Result::ErrorCode(rc)
    } else {
        let mut blob = Vec::with_capacity(a_out.len() * 4);
        for v in a_out.iter() {
            blob.extend_from_slice(&v.to_ne_bytes());
        }
        Fts3Result::Blob(blob)
    }
}

/// `sqlite3Fts3Matchinfo`.
pub fn fts3_matchinfo(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_csr: &mut Fts3Cursor,
    z_arg: Option<&[u8]>,
) -> Fts3Result {
    let z_format: &[u8] = z_arg.unwrap_or(FTS3_MATCHINFO_DEFAULT);
    if p_csr.p_expr.is_none() {
        Fts3Result::Blob(Vec::new())
    } else {
        /* Obtém os dados do matchinfo(). */
        let r = fts3_get_matchinfo(db, p, p_csr, z_format);
        fts3_segments_close(db, p);
        r
    }
}
