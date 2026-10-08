//! `fts5_expr.c`: a árvore de expressão de uma consulta MATCH (nós `AND`, `OR`, `NOT`, `NEAR`,
//! frase e termo), a tokenização do texto da consulta, a construção da árvore a partir do parser
//! (`fts5parse.y`, ver `parse.rs`) e a iteração dos documentos que casam sobre o índice.
//!
//! No Debian (sem `SQLITE_TEST` nem `SQLITE_FTS5_DEBUG`) somem `fts5_expr()`, `fts5_expr_tcl()`,
//! `fts5_isalnum()`, `fts5_fold()` e os impressores da árvore: `sqlite3Fts5ExprInit` não registra
//! nada (ver [`fts5_expr_init`]).
//!
//! # Modelo v2
//!
//! **Arena.** Os nós e as frases não são ponteiros: vivem em dois `Vec` do [`Fts5Expr`] e se
//! referem por índice ([`NodeId`], [`PhraseId`]). `Fts5ExprNode.apChild` é um `Vec<NodeId>`, o
//! `Fts5ExprNearset.apPhrase` um `Vec<PhraseId>` e o `Fts5ExprPhrase.pNode` um `NodeId`. O
//! `Fts5Expr.apExprPhrase` (e o `apPhrase` do `Fts5Parse`, que é o mesmo vetor durante o parse) é
//! um `Vec<PhraseId>`. Nós e frases descartados pelo parser (o `ParseNodeFree` e o `PhraseFree`
//! do C) ficam órfãos na arena até o `Drop` do `Fts5Expr`: o C só liberava memória ali.
//!
//! **Sem `pIndex`/`pConfig`.** O `Fts5Expr` não guarda o índice nem a configuração (que são da
//! `Fts5Table`). Quem itera passa `db`, `idx` e `cfg` a cada chamada, e o `b_desc` fica no
//! `Fts5Expr` (o `sqlite3Fts5ExprFirst` o grava). O `db` e o índice não se guardam em ninguém.
//!
//! **Poslist.** O `Fts5ExprPhrase.poslist` é um [`Fts5Buffer`] (`n` é o comprimento do vetor). O
//! C, num nó `TERM` com `detail=full`, apontava `poslist.p` direto para o `pData` do iterador do
//! índice; aqui os bytes são copiados (a leitura só vale na linha corrente, e o C sempre refaz o
//! teste antes de ler). Nos nós que o C só usa o `poslist.n` como "tem ou não tem" (`detail` não
//! `full`), o vetor ganha `n` bytes zero: só o comprimento importa ali.
//!
//! **Sem `NOMEM`.** Alocação falha abortando em Rust, então as funções que só falhavam por
//! `SQLITE_NOMEM` perdem o `pRc`.

use crate::connection::Connection;
use crate::consts::{SQLITE_ERROR, SQLITE_OK, SQLITE_RANGE};
use crate::printf::{mprintf, PrintfArg};
use crate::util::{at, str_icmp};

use super::buffer::{
    fts5_is_bareword, fts5_poslist_next64, fts5_pos2offset, Fts5Buffer, Fts5PoslistReader,
    Fts5PoslistWriter,
};
use super::config::fts5_dequote;
use super::index::{Fts5Index, Fts5IndexIter};
use super::int::{
    Fts5Colset, Fts5Config, FTS5INDEX_QUERY_DESC, FTS5INDEX_QUERY_PREFIX, FTS5_DEFAULT_NEARDIST,
    FTS5_DETAIL_FULL, FTS5_DETAIL_NONE, FTS5_MAX_TOKEN_SIZE, FTS5_TOKENIZE_DOCUMENT,
    FTS5_TOKENIZE_PREFIX, FTS5_TOKENIZE_QUERY, FTS5_TOKEN_COLOCATED,
};
use super::parse::{
    fts5_parser, YyParser, FTS5_AND, FTS5_CARET, FTS5_COLON, FTS5_COMMA, FTS5_EOF, FTS5_LCP,
    FTS5_LP, FTS5_MINUS, FTS5_NOT, FTS5_OR, FTS5_PLUS, FTS5_RCP, FTS5_RP, FTS5_STAR, FTS5_STRING,
    FTS5_TERM,
};

/// Profundidade máxima da árvore (`SQLITE_FTS5_MAX_EXPR_DEPTH`).
const SQLITE_FTS5_MAX_EXPR_DEPTH: i32 = 256;

/// `FTS5_LARGEST_INT64`.
const FTS5_LARGEST_INT64: i64 = i64::MAX;

/// `FTS5_LOOKAHEAD_EOF`.
const FTS5_LOOKAHEAD_EOF: i64 = 1 << 62;

/// O handle de um nó na arena do [`Fts5Expr`].
pub type NodeId = usize;

/// O handle de uma frase na arena do [`Fts5Expr`].
pub type PhraseId = usize;

// ---------------------------------------------------------------------------------------------
// Estruturas
// ---------------------------------------------------------------------------------------------

/// O `xNext` de um nó: qual função avança o nó (a tabela de ponteiros de função do C).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum XNext {
    /// `xNext==0`: o nó nunca casa (tipo `FTS5_EOF`, ou a raiz vazia).
    #[default]
    Nil,
    /// `fts5ExprNodeNext_TERM`.
    Term,
    /// `fts5ExprNodeNext_STRING`.
    String,
    /// `fts5ExprNodeNext_OR`.
    Or,
    /// `fts5ExprNodeNext_AND`.
    And,
    /// `fts5ExprNodeNext_NOT`.
    Not,
}

/// `Fts5ExprTerm`: um termo de busca ou prefixo de termo.
#[derive(Debug, Default)]
pub struct Fts5ExprTerm {
    /// Verdadeiro para um termo de prefixo.
    pub b_prefix: bool,
    /// Verdadeiro se o token precisa ser o primeiro da coluna.
    pub b_first: bool,
    /// O texto do termo (`nFullTerm` bytes).
    pub p_term: Vec<u8>,
    /// Tamanho efetivo do termo em bytes (até o primeiro NUL, com `tokendata`).
    pub n_query_term: i32,
    /// Tamanho do termo em bytes, com o tokendata.
    pub n_full_term: i32,
    /// O iterador deste termo sobre o índice.
    pub p_iter: Option<Fts5IndexIter>,
    /// O primeiro da lista de sinônimos.
    pub p_synonym: Option<Box<Fts5ExprTerm>>,
}

/// `Fts5ExprPhrase`: um ou mais termos que precisam aparecer numa sequência contígua.
#[derive(Debug, Default)]
pub struct Fts5ExprPhrase {
    /// O nó `FTS5_STRING` de que a frase faz parte.
    pub p_node: NodeId,
    /// A lista de posições corrente.
    pub poslist: Fts5Buffer,
    /// Os termos (`nTerm` é `len()`).
    pub a_term: Vec<Fts5ExprTerm>,
}

/// `Fts5ExprNearset`: frases que precisam estar a uma certa distância em cada documento que casa.
#[derive(Debug, Default, Clone)]
pub struct Fts5ExprNearset {
    /// O parâmetro NEAR.
    pub n_near: i32,
    /// As colunas a pesquisar (`None`: todas).
    pub p_colset: Option<Fts5Colset>,
    /// As frases (`nPhrase` é `len()`).
    pub ap_phrase: Vec<PhraseId>,
}

/// O nearset que `Fts5ExprNode::near` devolve num nó sem nearset.
static EMPTY_NEARSET: Fts5ExprNearset =
    Fts5ExprNearset { n_near: 0, p_colset: None, ap_phrase: Vec::new() };

/// `Fts5ExprNode`. `e_type` é sempre um de `FTS5_AND`, `FTS5_OR`, `FTS5_NOT` (`ap_child` vale),
/// `FTS5_STRING`, `FTS5_TERM` (`p_near` vale) ou `FTS5_EOF` (nunca casa).
#[derive(Debug, Default)]
pub struct Fts5ExprNode {
    /// O tipo do nó.
    pub e_type: i32,
    /// Verdadeiro no fim.
    pub b_eof: bool,
    /// Verdadeiro se a entrada corrente não é um casamento.
    pub b_nomatch: bool,
    /// Distância até a folha mais longe (0 em `STRING` e `TERM`).
    pub i_height: i32,
    /// O método de avanço do nó.
    pub x_next: XNext,
    /// O rowid corrente.
    pub i_rowid: i64,
    /// Para `STRING` e `TERM`: o aglomerado de frases.
    pub p_near: Option<Fts5ExprNearset>,
    /// Os filhos. Um `NOT` sempre tem 2; `AND` e `OR` têm 2 ou mais.
    pub ap_child: Vec<NodeId>,
}

impl Fts5ExprNode {
    /// O nearset do nó (vazio num nó que não é `STRING` nem `TERM`).
    fn near(&self) -> &Fts5ExprNearset {
        self.p_near.as_ref().unwrap_or(&EMPTY_NEARSET)
    }

    /// `Fts5NodeIsString`.
    fn is_string(&self) -> bool {
        self.e_type == FTS5_TERM || self.e_type == FTS5_STRING
    }
}

/// `Fts5Expr`: uma expressão MATCH compilada, com a arena de nós e frases.
#[derive(Debug, Default)]
pub struct Fts5Expr {
    /// A arena de nós.
    pub nodes: Vec<Fts5ExprNode>,
    /// A arena de frases.
    pub phrases: Vec<Fts5ExprPhrase>,
    /// A raiz.
    pub p_root: NodeId,
    /// Verdadeiro para iterar em ordem decrescente de rowid.
    pub b_desc: bool,
    /// As frases da expressão, na ordem do texto (`nPhrase` é `len()`).
    pub ap_expr_phrase: Vec<PhraseId>,
}

/// `Fts5PoslistPopulator`: o estado de uma frase em `populate_poslists` (só `detail=col` e
/// `detail=none`).
#[derive(Debug, Default, Clone, Copy)]
pub struct Fts5PoslistPopulator {
    /// O escritor da poslist da frase.
    pub writer: Fts5PoslistWriter,
    /// Verdadeiro se é para preencher.
    pub b_ok: bool,
    /// Verdadeiro se a frase não casou (a linha já estava "viva" sem poslist).
    pub b_miss: bool,
}

/// O contexto de iteração que o C guardava em `pExpr->pIndex`/`pConfig` e no `sqlite3 *`.
struct Cx<'a> {
    db: &'a mut Connection,
    idx: &'a mut Fts5Index,
    cfg: &'a mut Fts5Config,
}

/// `fts5QueryTerm`: o número de bytes de `token` antes do primeiro NUL (ou todos).
fn query_term_len(token: &[u8]) -> usize {
    token.iter().position(|&b| b == 0).unwrap_or(token.len())
}

// ---------------------------------------------------------------------------------------------
// O parse
// ---------------------------------------------------------------------------------------------

/// `Fts5Parse`: o contexto do parse. A arena (`ex`) já é a do `Fts5Expr` que sai do parse, e o
/// `apPhrase` do C é `ex.ap_expr_phrase`.
pub struct Fts5Parse<'c> {
    p_config: &'c Fts5Config,
    /// A mensagem de erro (`zErr`).
    pub z_err: Option<Vec<u8>>,
    /// O código de erro.
    pub rc: i32,
    /// A arena e a lista de frases.
    pub ex: Fts5Expr,
    /// O resultado de um parse bem sucedido.
    pub p_expr: Option<NodeId>,
    /// Converter `a+b` em `a AND b`.
    pub b_phrase_to_and: bool,
}

/// `fts5ParseTokenize`: o callback de tokenização de `ParseTerm` e de `ClonePhrase`. `p_phrase` é
/// o `sCtx.pPhrase`: criado no primeiro token se ainda não existe. O C só falhava por `NOMEM`.
fn parse_tokenize(
    phrases: &mut Vec<Fts5ExprPhrase>,
    p_phrase: &mut Option<PhraseId>,
    b_tokendata: bool,
    tflags: i32,
    token: &[u8],
) -> i32 {
    let n_token = token.len().min(FTS5_MAX_TOKEN_SIZE);
    let token = &token[..n_token];
    let n_query = if b_tokendata { query_term_len(token) } else { n_token };

    if let Some(pid) = *p_phrase {
        if !phrases[pid].a_term.is_empty() && (tflags & FTS5_TOKEN_COLOCATED) != 0 {
            let last = phrases[pid].a_term.last_mut().expect("termo anterior");
            let syn = Fts5ExprTerm {
                p_term: token.to_vec(),
                n_full_term: n_token as i32,
                n_query_term: n_query as i32,
                p_synonym: last.p_synonym.take(),
                ..Default::default()
            };
            last.p_synonym = Some(Box::new(syn));
            return SQLITE_OK;
        }
    }

    let pid = match *p_phrase {
        Some(pid) => pid,
        None => {
            phrases.push(Fts5ExprPhrase::default());
            let pid = phrases.len() - 1;
            *p_phrase = Some(pid);
            pid
        }
    };
    phrases[pid].a_term.push(Fts5ExprTerm {
        p_term: token.to_vec(),
        n_full_term: n_token as i32,
        n_query_term: n_query as i32,
        ..Default::default()
    });
    SQLITE_OK
}

/// `fts5MergeColset`: tira de `colset` as colunas que não estão também em `merge`.
fn merge_colset(colset: &mut Fts5Colset, merge: &Fts5Colset) {
    let mut i_in = 0usize;
    let mut i_merge = 0usize;
    let mut i_out = 0usize;
    while i_in < colset.ai_col.len() && i_merge < merge.ai_col.len() {
        let i_diff = colset.ai_col[i_in] - merge.ai_col[i_merge];
        if i_diff == 0 {
            colset.ai_col[i_out] = merge.ai_col[i_merge];
            i_out += 1;
            i_merge += 1;
            i_in += 1;
        } else if i_diff > 0 {
            i_merge += 1;
        } else {
            i_in += 1;
        }
    }
    colset.ai_col.truncate(i_out);
}

impl Fts5Expr {
    /// `fts5ExprAssignXNext`: escolhe o `xNext` do nó, e rebaixa um `STRING` de uma frase de um
    /// termo só (sem sinônimo e sem `^`) para `TERM`.
    fn assign_xnext(&mut self, id: NodeId) {
        match self.nodes[id].e_type {
            FTS5_STRING => {
                let near = self.nodes[id].near();
                let is_term = near.ap_phrase.len() == 1 && {
                    let ph = &self.phrases[near.ap_phrase[0]];
                    ph.a_term.len() == 1
                        && ph.a_term[0].p_synonym.is_none()
                        && !ph.a_term[0].b_first
                };
                if is_term {
                    self.nodes[id].e_type = FTS5_TERM;
                    self.nodes[id].x_next = XNext::Term;
                } else {
                    self.nodes[id].x_next = XNext::String;
                }
            }
            FTS5_OR => self.nodes[id].x_next = XNext::Or,
            FTS5_AND => self.nodes[id].x_next = XNext::And,
            _ => {
                debug_assert!(self.nodes[id].e_type == FTS5_NOT);
                self.nodes[id].x_next = XNext::Not;
            }
        }
    }

    /// `fts5ExprAddChildren`: acrescenta `sub` aos filhos de `p` (os filhos de `sub` no lugar,
    /// se `sub` é do mesmo tipo e `p` não é um `NOT`) e atualiza a altura de `p`.
    fn add_children(&mut self, p: NodeId, sub: NodeId) {
        let ii = self.nodes[p].ap_child.len();
        if self.nodes[p].e_type != FTS5_NOT && self.nodes[sub].e_type == self.nodes[p].e_type {
            let kids = std::mem::take(&mut self.nodes[sub].ap_child);
            self.nodes[p].ap_child.extend(kids);
        } else {
            self.nodes[p].ap_child.push(sub);
        }
        for k in ii..self.nodes[p].ap_child.len() {
            let h = self.nodes[self.nodes[p].ap_child[k]].i_height + 1;
            if h > self.nodes[p].i_height {
                self.nodes[p].i_height = h;
            }
        }
    }

    /// Acrescenta a arena de `other` à deste (reindexando os handles) e devolve a raiz e a lista
    /// de frases de `other` já no espaço de índices deste. Usado por `fts5_expr_and`.
    fn absorb(&mut self, other: Fts5Expr) -> (NodeId, Vec<PhraseId>) {
        let node_off = self.nodes.len();
        let phrase_off = self.phrases.len();
        for mut n in other.nodes {
            for c in n.ap_child.iter_mut() {
                *c += node_off;
            }
            if let Some(near) = n.p_near.as_mut() {
                for ph in near.ap_phrase.iter_mut() {
                    *ph += phrase_off;
                }
            }
            self.nodes.push(n);
        }
        for mut ph in other.phrases {
            ph.p_node += node_off;
            self.phrases.push(ph);
        }
        let aps = other.ap_expr_phrase.iter().map(|p| p + phrase_off).collect();
        (other.p_root + node_off, aps)
    }
}

impl<'c> Fts5Parse<'c> {
    /// Um contexto de parse novo (`memset(&sParse, 0, ...)` e `pConfig`).
    pub fn new(p_config: &'c Fts5Config, b_phrase_to_and: bool) -> Fts5Parse<'c> {
        Fts5Parse {
            p_config,
            z_err: None,
            rc: SQLITE_OK,
            ex: Fts5Expr::default(),
            p_expr: None,
            b_phrase_to_and,
        }
    }

    /// `sqlite3Fts5ParseError`: grava a primeira mensagem de erro (as seguintes são ignoradas).
    pub fn error(&mut self, fmt: &[u8], args: &[PrintfArg]) {
        if self.rc == SQLITE_OK {
            debug_assert!(self.z_err.is_none());
            self.z_err = mprintf(fmt, args);
            self.rc = SQLITE_ERROR;
        }
    }

    /// `sqlite3Fts5ParseFinished`: o resultado do parse.
    pub fn finished(&mut self, p: Option<NodeId>) {
        debug_assert!(self.p_expr.is_none());
        self.p_expr = p;
    }

    /// `sqlite3Fts5ParseSetCaret`: liga o `bFirst` do primeiro termo da frase.
    pub fn set_caret(&mut self, p_phrase: Option<PhraseId>) {
        if let Some(pid) = p_phrase {
            if let Some(t) = self.ex.phrases[pid].a_term.first_mut() {
                t.b_first = true;
            }
        }
    }

    /// `sqlite3Fts5ParseNearset`: acrescenta a frase ao nearset (que nasce se `p_near` é `None`).
    /// Uma frase sem termos some, a não ser que seja a única.
    pub fn nearset(
        &mut self,
        p_near: Option<Fts5ExprNearset>,
        p_phrase: Option<PhraseId>,
    ) -> Option<Fts5ExprNearset> {
        let mut p_ret: Option<Fts5ExprNearset> = None;
        if self.rc == SQLITE_OK {
            if p_phrase.is_none() {
                return p_near;
            }
            p_ret = Some(p_near.unwrap_or_default());
        }

        let mut ret = p_ret?;
        let mut p_phrase = p_phrase?;
        if let Some(&p_last) = ret.ap_phrase.last() {
            let n_ap = self.ex.ap_expr_phrase.len();
            debug_assert!(n_ap >= 2 && self.ex.ap_expr_phrase[n_ap - 2] == p_last);
            if self.ex.phrases[p_phrase].a_term.is_empty() {
                ret.ap_phrase.pop();
                self.ex.ap_expr_phrase.pop();
                p_phrase = p_last;
            } else if self.ex.phrases[p_last].a_term.is_empty() {
                self.ex.ap_expr_phrase[n_ap - 2] = p_phrase;
                self.ex.ap_expr_phrase.pop();
                ret.ap_phrase.pop();
            }
        }
        ret.ap_phrase.push(p_phrase);
        Some(ret)
    }

    /// `sqlite3Fts5ParseTerm`: tokeniza o texto do token (com aspas ou sem) e acrescenta os
    /// termos à frase `p_append`, ou a uma frase nova. `b_prefix` diz que há um `*` depois.
    pub fn term(
        &mut self,
        p_append: Option<PhraseId>,
        token: &[u8],
        b_prefix: bool,
    ) -> Option<PhraseId> {
        let cfg = self.p_config;
        let b_tokendata = cfg.b_tokendata != 0;
        let mut p_phrase = p_append;

        let mut z = token.to_vec();
        let flags = FTS5_TOKENIZE_QUERY | if b_prefix { FTS5_TOKENIZE_PREFIX } else { 0 };
        fts5_dequote(&mut z);
        let n = query_term_len(&z);
        let phrases = &mut self.ex.phrases;
        let rc = cfg.tokenize(flags, Some(&z[..n]), &mut |tflags, tok, _, _| {
            parse_tokenize(phrases, &mut p_phrase, b_tokendata, tflags, tok)
        });
        if rc != SQLITE_OK {
            self.rc = rc;
            return None;
        }

        if p_append.is_none() {
            self.ex.ap_expr_phrase.push(0);
        }
        let ret = match p_phrase {
            None => {
                /* Um token ou frase entre aspas sem nenhum caractere de token (`MATCH '""'`). */
                if self.rc == SQLITE_OK {
                    self.ex.phrases.push(Fts5ExprPhrase::default());
                    Some(self.ex.phrases.len() - 1)
                } else {
                    None
                }
            }
            Some(pid) => {
                if let Some(t) = self.ex.phrases[pid].a_term.last_mut() {
                    t.b_prefix = b_prefix;
                }
                Some(pid)
            }
        };
        if let Some(last) = self.ex.ap_expr_phrase.last_mut() {
            *last = ret.unwrap_or(0);
        }
        ret
    }

    /// `sqlite3Fts5ParseNear`: o token que veio onde o NEAR é esperado precisa ser `NEAR`.
    pub fn near(&mut self, tok: &[u8]) {
        if tok != b"NEAR" {
            self.error(
                b"fts5: syntax error near \"%s\"",
                &[PrintfArg::Text(Some(tok.to_vec()))],
            );
        }
    }

    /// `sqlite3Fts5ParseSetDistance`: o `, <inteiro>` opcional no fim do NEAR.
    pub fn set_distance(&mut self, p_near: Option<&mut Fts5ExprNearset>, p: &[u8]) {
        if let Some(near) = p_near {
            let mut n_near: i32 = 0;
            if !p.is_empty() {
                for &c in p {
                    if !c.is_ascii_digit() {
                        self.error(
                            b"expected integer, got \"%s\"",
                            &[PrintfArg::Text(Some(p.to_vec()))],
                        );
                        return;
                    }
                    n_near = n_near.wrapping_mul(10).wrapping_add((c - b'0') as i32);
                }
            } else {
                n_near = FTS5_DEFAULT_NEARDIST;
            }
            near.n_near = n_near;
        }
    }

    /// `sqlite3Fts5ParseColset`: o colset `p_colset` com a coluna nomeada pelo token acrescentada
    /// (em ordem, sem repetir).
    pub fn colset(&mut self, p_colset: Option<Fts5Colset>, p: &[u8]) -> Option<Fts5Colset> {
        let mut ret = None;
        if self.rc == SQLITE_OK {
            let cfg = self.p_config;
            let mut z = p.to_vec();
            fts5_dequote(&mut z);
            z.truncate(query_term_len(&z));
            match cfg.az_col.iter().position(|c| str_icmp(c, &z) == 0) {
                None => {
                    self.error(b"no such column: %s", &[PrintfArg::Text(Some(z))]);
                }
                Some(i_col) => {
                    /* fts5ParseColset */
                    let mut colset = p_colset.unwrap_or_default();
                    let i_col = i_col as i32;
                    let mut i = 0usize;
                    let mut dup = false;
                    while i < colset.ai_col.len() {
                        if colset.ai_col[i] == i_col {
                            dup = true;
                            break;
                        }
                        if colset.ai_col[i] > i_col {
                            break;
                        }
                        i += 1;
                    }
                    if !dup {
                        colset.ai_col.insert(i, i_col);
                    }
                    ret = Some(colset);
                }
            }
        }
        ret
    }

    /// `sqlite3Fts5ParseColsetInvert`: o complemento do colset (as colunas da tabela que ele não
    /// tem).
    pub fn colset_invert(&mut self, p: Option<Fts5Colset>) -> Option<Fts5Colset> {
        if self.rc != SQLITE_OK {
            return None;
        }
        let n_col = self.p_config.n_col();
        let p = p.unwrap_or_default();
        let mut ret = Fts5Colset::default();
        let mut i_old = 0usize;
        for i in 0..n_col {
            if i_old >= p.ai_col.len() || p.ai_col[i_old] != i {
                ret.ai_col.push(i);
            } else {
                i_old += 1;
            }
        }
        Some(ret)
    }

    /// `fts5ParseSetColset`: aplica o colset ao nó e a todos os descendentes.
    fn set_colset_rec(&mut self, node: NodeId, colset: &Fts5Colset) {
        if self.rc != SQLITE_OK {
            return;
        }
        let n = &mut self.ex.nodes[node];
        debug_assert!(
            n.e_type == FTS5_TERM
                || n.e_type == FTS5_STRING
                || n.e_type == FTS5_AND
                || n.e_type == FTS5_OR
                || n.e_type == FTS5_NOT
                || n.e_type == FTS5_EOF
        );
        if n.is_string() {
            if let Some(near) = n.p_near.as_mut() {
                match near.p_colset.as_mut() {
                    Some(existing) => {
                        merge_colset(existing, colset);
                        if existing.ai_col.is_empty() {
                            n.e_type = FTS5_EOF;
                            n.x_next = XNext::Nil;
                        }
                    }
                    None => near.p_colset = Some(colset.clone()),
                }
            }
        } else {
            let kids = n.ap_child.clone();
            for k in kids {
                self.set_colset_rec(k, colset);
            }
        }
    }

    /// `sqlite3Fts5ParseSetColset`: aplica o colset à expressão inteira.
    pub fn set_colset(&mut self, p_expr: Option<NodeId>, p_colset: Option<Fts5Colset>) {
        if self.p_config.e_detail == FTS5_DETAIL_NONE {
            self.error(b"fts5: column queries are not supported (detail=none)", &[]);
        } else if let (Some(node), Some(colset)) = (p_expr, p_colset) {
            self.set_colset_rec(node, &colset);
        }
    }

    /// `fts5ParsePhraseToAnd`: converte a frase `abc + def + ghi` na árvore `abc AND def AND ghi`.
    fn phrase_to_and(&mut self, near: Fts5ExprNearset) -> Option<NodeId> {
        debug_assert!(near.ap_phrase.len() == 1 && self.b_phrase_to_and);
        let first = near.ap_phrase[0];
        let n_term = self.ex.phrases[first].a_term.len();

        self.ex.nodes.push(Fts5ExprNode { e_type: FTS5_AND, i_height: 1, ..Default::default() });
        let ret = self.ex.nodes.len() - 1;
        self.ex.assign_xnext(ret);
        self.ex.ap_expr_phrase.pop();

        let mut kids: Vec<Option<NodeId>> = Vec::with_capacity(n_term);
        for ii in 0..n_term {
            let p = &self.ex.phrases[first].a_term[ii];
            let to = Fts5ExprTerm {
                p_term: p.p_term[..p.n_full_term as usize].to_vec(),
                n_query_term: p.n_query_term,
                n_full_term: p.n_full_term,
                ..Default::default()
            };
            self.ex.phrases.push(Fts5ExprPhrase { a_term: vec![to], ..Default::default() });
            let pid = self.ex.phrases.len() - 1;
            self.ex.ap_expr_phrase.push(pid);
            let near1 = self.nearset(None, Some(pid));
            kids.push(self.node(FTS5_STRING, None, None, near1));
        }

        if self.rc != SQLITE_OK {
            return None;
        }
        self.ex.nodes[ret].ap_child = kids.into_iter().flatten().collect();
        Some(ret)
    }

    /// `sqlite3Fts5ParseNode`: um nó novo. `e_type` é `FTS5_STRING` (com `p_near`), `FTS5_AND`,
    /// `FTS5_OR` ou `FTS5_NOT` (com os dois filhos). Um filho ausente faz o nó ser o outro filho.
    pub fn node(
        &mut self,
        e_type: i32,
        p_left: Option<NodeId>,
        p_right: Option<NodeId>,
        p_near: Option<Fts5ExprNearset>,
    ) -> Option<NodeId> {
        let mut p_ret: Option<NodeId> = None;

        if self.rc == SQLITE_OK {
            debug_assert!(
                (e_type != FTS5_STRING && p_near.is_none())
                    || (e_type == FTS5_STRING && p_left.is_none() && p_right.is_none())
            );
            if e_type == FTS5_STRING && p_near.is_none() {
                return None;
            }
            if e_type != FTS5_STRING && p_left.is_none() {
                return p_right;
            }
            if e_type != FTS5_STRING && p_right.is_none() {
                return p_left;
            }

            if e_type == FTS5_STRING
                && self.b_phrase_to_and
                && p_near
                    .as_ref()
                    .is_some_and(|n| self.ex.phrases[n.ap_phrase[0]].a_term.len() > 1)
            {
                p_ret = self.phrase_to_and(p_near.expect("nearset"));
            } else {
                self.ex.nodes.push(Fts5ExprNode { e_type, p_near, ..Default::default() });
                let id = self.ex.nodes.len() - 1;
                self.ex.assign_xnext(id);
                if e_type == FTS5_STRING {
                    let phrases: Vec<PhraseId> = self.ex.nodes[id].near().ap_phrase.clone();
                    for &ph in &phrases {
                        self.ex.phrases[ph].p_node = id;
                        if self.ex.phrases[ph].a_term.is_empty() {
                            self.ex.nodes[id].x_next = XNext::Nil;
                            self.ex.nodes[id].e_type = FTS5_EOF;
                        }
                    }

                    p_ret = Some(id);
                    if self.p_config.e_detail != FTS5_DETAIL_FULL {
                        let ph = &self.ex.phrases[phrases[0]];
                        if phrases.len() != 1
                            || ph.a_term.len() > 1
                            || (!ph.a_term.is_empty() && ph.a_term[0].b_first)
                        {
                            let what: &[u8] = if phrases.len() == 1 { b"phrase" } else { b"NEAR" };
                            self.error(
                                b"fts5: %s queries are not supported (detail!=full)",
                                &[PrintfArg::Text(Some(what.to_vec()))],
                            );
                            p_ret = None;
                        }
                    }
                } else {
                    self.ex.add_children(id, p_left.expect("filho esquerdo"));
                    self.ex.add_children(id, p_right.expect("filho direito"));
                    p_ret = Some(id);
                    if self.ex.nodes[id].i_height > SQLITE_FTS5_MAX_EXPR_DEPTH {
                        self.error(
                            b"fts5 expression tree is too large (maximum depth %d)",
                            &[PrintfArg::Int(SQLITE_FTS5_MAX_EXPR_DEPTH as i64)],
                        );
                        p_ret = None;
                    }
                }
            }
        }

        debug_assert!(p_ret.is_some() || self.rc != SQLITE_OK);
        p_ret
    }

    /// `sqlite3Fts5ParseImplicitAnd`: o AND implícito entre expressões vizinhas (`a b c`). Frases
    /// vazias (`""`) são engolidas e tiradas da lista de frases.
    pub fn implicit_and(
        &mut self,
        p_left: Option<NodeId>,
        p_right: Option<NodeId>,
    ) -> Option<NodeId> {
        if self.rc != SQLITE_OK {
            return None;
        }
        let (left, right) = (p_left?, p_right?);
        let nodes = &self.ex.nodes;
        debug_assert!(
            nodes[left].is_string() || nodes[left].e_type == FTS5_EOF || nodes[left].e_type == FTS5_AND
        );

        let p_prev = if nodes[left].e_type == FTS5_AND {
            *nodes[left].ap_child.last()?
        } else {
            left
        };

        if nodes[right].e_type == FTS5_EOF {
            debug_assert!(self.ex.ap_expr_phrase.last() == nodes[right].near().ap_phrase.first());
            self.ex.ap_expr_phrase.pop();
            Some(left)
        } else if nodes[p_prev].e_type == FTS5_EOF {
            let ret = if p_prev == left {
                right
            } else {
                *self.ex.nodes[left].ap_child.last_mut()? = right;
                left
            };
            let n_right = self.ex.nodes[right].near().ap_phrase.len();
            let n_ap = self.ex.ap_expr_phrase.len();
            if let Some(i) = (n_ap - 1).checked_sub(n_right) {
                self.ex.ap_expr_phrase.remove(i);
            }
            Some(ret)
        } else {
            self.node(FTS5_AND, Some(left), Some(right), None)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tokenização do texto da consulta
// ---------------------------------------------------------------------------------------------

/// `fts5ExprIsspace`.
fn expr_isspace(t: u8) -> bool {
    t == b' ' || t == b'\t' || t == b'\n' || t == b'\r'
}

/// `fts5ExprGetToken`: lê o primeiro token de `z[*pos..]` e avança `*pos`. Devolve o código do
/// token e o seu texto. Nos erros devolve `FTS5_EOF` (com o erro gravado no parse) sem avançar.
fn expr_get_token<'a>(parse: &mut Fts5Parse<'_>, z: &'a [u8], pos: &mut usize) -> (i32, &'a [u8]) {
    let mut i = *pos;
    /* Pula os espaços */
    while expr_isspace(at(z, i)) {
        i += 1;
    }
    let start = i;
    let mut n = 1usize;
    let slice = |n: usize| z.get(start..start + n).unwrap_or(&[]);

    let tok = match at(z, i) {
        b'(' => FTS5_LP,
        b')' => FTS5_RP,
        b'{' => FTS5_LCP,
        b'}' => FTS5_RCP,
        b':' => FTS5_COLON,
        b',' => FTS5_COMMA,
        b'+' => FTS5_PLUS,
        b'*' => FTS5_STAR,
        b'-' => FTS5_MINUS,
        b'^' => FTS5_CARET,
        0 => FTS5_EOF,
        b'"' => {
            let mut z2 = start + 1;
            loop {
                if at(z, z2) == b'"' {
                    z2 += 1;
                    if at(z, z2) != b'"' {
                        break;
                    }
                }
                if at(z, z2) == 0 {
                    parse.error(b"unterminated string", &[]);
                    return (FTS5_EOF, slice(1));
                }
                z2 += 1;
            }
            n = z2 - start;
            FTS5_STRING
        }
        c => {
            if !fts5_is_bareword(c) {
                parse.error(
                    b"fts5: syntax error near \"%s\"",
                    &[PrintfArg::Text(Some(vec![c]))],
                );
                return (FTS5_EOF, slice(1));
            }
            let mut z2 = start + 1;
            while fts5_is_bareword(at(z, z2)) {
                z2 += 1;
            }
            n = z2 - start;
            match &z[start..start + n] {
                b"OR" => FTS5_OR,
                b"NOT" => FTS5_NOT,
                b"AND" => FTS5_AND,
                _ => FTS5_STRING,
            }
        }
    };

    *pos = start + n;
    (tok, slice(n))
}

// ---------------------------------------------------------------------------------------------
// Construção e liberação
// ---------------------------------------------------------------------------------------------

/// `sqlite3Fts5ExprNew`: compila `z_expr` numa expressão. `b_phrase_to_and` converte `a+b` em
/// `a AND b`. Se `i_col < nCol`, a expressão inteira é restrita à coluna `i_col`. Em erro devolve
/// o código, `*pp_new` fica `None` e a mensagem (se há) vai em `*pz_err`.
pub fn fts5_expr_new(
    cfg: &Fts5Config,
    b_phrase_to_and: bool,
    i_col: i32,
    z_expr: &[u8],
    pp_new: &mut Option<Fts5Expr>,
    pz_err: &mut Option<Vec<u8>>,
) -> i32 {
    *pp_new = None;
    *pz_err = None;
    let z_expr = &z_expr[..query_term_len(z_expr)];
    let mut parse = Fts5Parse::new(cfg, b_phrase_to_and);
    let mut engine = YyParser::new();
    let mut pos = 0usize;

    loop {
        let (t, token) = expr_get_token(&mut parse, z_expr, &mut pos);
        fts5_parser(&mut engine, t, token, &mut parse);
        if !(parse.rc == SQLITE_OK && t != FTS5_EOF) {
            break;
        }
    }
    drop(engine);

    /* Se o lado esquerdo do MATCH era uma coluna do usuário, aplica o filtro de coluna. */
    if i_col < cfg.n_col() && parse.p_expr.is_some() && parse.rc == SQLITE_OK {
        let colset = Fts5Colset { ai_col: vec![i_col] };
        let p_expr = parse.p_expr;
        parse.set_colset(p_expr, Some(colset));
    }

    debug_assert!(parse.rc != SQLITE_OK || parse.z_err.is_none());
    if parse.rc == SQLITE_OK {
        let mut ex = std::mem::take(&mut parse.ex);
        match parse.p_expr {
            None => {
                ex.nodes.push(Fts5ExprNode { b_eof: true, ..Default::default() });
                ex.p_root = ex.nodes.len() - 1;
            }
            Some(root) => ex.p_root = root,
        }
        ex.b_desc = false;
        *pp_new = Some(ex);
    }

    *pz_err = parse.z_err.take();
    parse.rc
}

/// `fts5ExprCountChar`: o número de caracteres UTF-8 de `z`.
fn expr_count_char(z: &[u8]) -> usize {
    z.iter().filter(|&&b| (b & 0xC0) != 0x80).count()
}

/// `sqlite3Fts5ExprPattern`: só para o tokenizador `trigram`. `z_text` é o padrão de um LIKE ou
/// GLOB contra a coluna `i_col`; cria uma expressão MATCH que casa um superconjunto das linhas.
/// `*pp` fica `None` se o padrão não tem nenhum trecho de 3 caracteres. A mensagem de erro vai
/// para `cfg.errmsg` quando `cfg.errmsg_target` está armado (o `pzErrmsg` do C).
pub fn fts5_expr_pattern(
    cfg: &mut Fts5Config,
    b_glob: bool,
    mut i_col: i32,
    z_text: &[u8],
    pp: &mut Option<Fts5Expr>,
) -> i32 {
    let z_text = &z_text[..query_term_len(z_text)];
    let n_text = z_text.len() as i64;
    let mut z_expr: Vec<u8> = Vec::with_capacity(z_text.len() * 4 + 1);
    let a_spec: [u8; 3] = if !b_glob { [b'_', b'%', 0] } else { [b'*', b'?', b'['] };

    let mut i: i64 = 0;
    let mut i_first: i64 = 0;
    let tx = |k: i64| at(z_text, k as usize);

    while i <= n_text {
        if i == n_text || tx(i) == a_spec[0] || tx(i) == a_spec[1] || tx(i) == a_spec[2] {
            if expr_count_char(&z_text[i_first as usize..i as usize]) >= 3 {
                z_expr.push(b'"');
                for jj in i_first..i {
                    z_expr.push(tx(jj));
                    if tx(jj) == b'"' {
                        z_expr.push(b'"');
                    }
                }
                z_expr.push(b'"');
                z_expr.push(b' ');
            }
            if tx(i) == a_spec[2] {
                i += 2;
                if tx(i - 1) == b'^' {
                    i += 1;
                }
                while i < n_text && tx(i) != b']' {
                    i += 1;
                }
            }
            i_first = i + 1;
        }
        i += 1;
    }

    if !z_expr.is_empty() {
        let mut b_and = false;
        if cfg.e_detail != FTS5_DETAIL_FULL {
            b_and = true;
            if cfg.e_detail == FTS5_DETAIL_NONE {
                i_col = cfg.n_col();
            }
        }
        let mut err = None;
        let rc = fts5_expr_new(cfg, b_and, i_col, &z_expr, pp, &mut err);
        if cfg.errmsg_target {
            cfg.errmsg = err;
        }
        rc
    } else {
        *pp = None;
        SQLITE_OK
    }
}

/// `sqlite3Fts5ExprAnd`: faz de `*pp1` o AND de `*pp1` e `p2` (as frases de `p2` vêm primeiro na
/// lista de frases). Se `*pp1` é `None`, passa a ser `p2`. Em erro (árvore funda demais) `*pp1`
/// fica `None`: o C o deixava sem raiz, para ser só liberado.
pub fn fts5_expr_and(cfg: &Fts5Config, pp1: &mut Option<Fts5Expr>, p2: Option<Fts5Expr>) -> i32 {
    let mut parse = Fts5Parse::new(cfg, false);

    match (pp1.take(), p2) {
        (Some(p1), Some(p2)) => {
            let old_aps = p1.ap_expr_phrase.clone();
            let root1 = p1.p_root;
            parse.ex = p1;
            let (root2, mut aps) = parse.ex.absorb(p2);

            let new_root = parse.node(FTS5_AND, Some(root1), Some(root2), None);
            if parse.rc == SQLITE_OK {
                if let Some(root) = new_root {
                    parse.ex.p_root = root;
                }
                aps.extend(old_aps);
                parse.ex.ap_expr_phrase = aps;
                *pp1 = Some(parse.ex);
            }
        }
        (p1, Some(p2)) => {
            debug_assert!(p1.is_none());
            *pp1 = Some(p2);
        }
        (p1, None) => *pp1 = p1,
    }

    parse.rc
}

/// `sqlite3Fts5ExprInit`: sem `SQLITE_TEST` nem `SQLITE_FTS5_DEBUG` não registra nenhuma função.
pub fn fts5_expr_init(_db: &mut Connection) -> i32 {
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// Sinônimos e frases
// ---------------------------------------------------------------------------------------------

/// `fts5ExprSynonymRowid`: o rowid corrente da lista de sinônimos que começa em `p_term`
/// (o menor, ou o maior se `b_desc`). Liga `*pb_eof` se nenhum iterador está válido.
fn synonym_rowid(p_term: &Fts5ExprTerm, b_desc: bool, pb_eof: Option<&mut bool>) -> i64 {
    let mut i_ret = 0i64;
    let mut b_ret_valid = false;
    let mut p = Some(p_term);
    while let Some(t) = p {
        if let Some(it) = t.p_iter.as_ref() {
            if !it.eof() {
                let i_rowid = it.rowid();
                if !b_ret_valid || (b_desc != (i_rowid < i_ret)) {
                    i_ret = i_rowid;
                    b_ret_valid = true;
                }
            }
        }
        p = t.p_synonym.as_deref();
    }
    if let Some(pb) = pb_eof {
        if !b_ret_valid {
            *pb = true;
        }
    }
    i_ret
}

/// `fts5ExprSynonymList`: a poslist da lista de sinônimos de `p_term` no rowid `i_rowid` (a
/// união das poslists dos iteradores nesse rowid).
fn synonym_list(p_term: &Fts5ExprTerm, i_rowid: i64) -> Vec<u8> {
    let mut a_iter: Vec<Fts5PoslistReader<'_>> = Vec::new();
    let mut p = Some(p_term);
    while let Some(t) = p {
        if let Some(it) = t.p_iter.as_ref() {
            if !it.eof() && it.rowid() == i_rowid {
                if !it.data().is_empty() {
                    a_iter.push(Fts5PoslistReader::init(it.data()));
                    debug_assert!(a_iter.last().is_some_and(|r| r.b_eof == 0));
                }
            }
        }
        p = t.p_synonym.as_deref();
    }

    if a_iter.len() == 1 {
        return a_iter[0].a.to_vec();
    }
    let mut writer = Fts5PoslistWriter::default();
    let mut buf = Fts5Buffer::new();
    let mut i_prev: i64 = -1;
    loop {
        let mut i_min = FTS5_LARGEST_INT64;
        for r in a_iter.iter_mut() {
            if r.b_eof == 0 {
                if r.i_pos == i_prev && r.next() {
                    continue;
                }
                if r.i_pos < i_min {
                    i_min = r.i_pos;
                }
            }
        }
        if i_min == FTS5_LARGEST_INT64 {
            break;
        }
        writer.append(&mut buf, i_min);
        i_prev = i_min;
    }
    buf.p
}

/// `fts5ExprPhraseIsMatch`: todos os iteradores de termo da frase apontam para o rowid
/// `i_rowid`. Confere se é mesmo um casamento e, se é, preenche a poslist da frase. Devolve se
/// casou.
fn phrase_is_match(i_rowid: i64, ph: &mut Fts5ExprPhrase) -> bool {
    let mut writer = Fts5PoslistWriter::default();
    let b_first = ph.a_term[0].b_first;
    let Fts5ExprPhrase { poslist, a_term, .. } = ph;
    poslist.zero();

    let n_term = a_term.len();
    /* As listas dos termos com sinônimo; as dos outros são fatias dos iteradores. */
    let owned: Vec<Option<Vec<u8>>> = a_term
        .iter()
        .map(|t| t.p_synonym.as_ref().map(|_| synonym_list(t, i_rowid)))
        .collect();

    let mut a_iter: Vec<Fts5PoslistReader<'_>> = Vec::with_capacity(n_term);
    for (i, t) in a_term.iter().enumerate() {
        let a: &[u8] = match &owned[i] {
            Some(v) => v,
            None => t.p_iter.as_ref().map_or(&[][..], |it| it.data()),
        };
        let r = Fts5PoslistReader::init(a);
        let eof = r.b_eof != 0;
        a_iter.push(r);
        if eof {
            return !poslist.p.is_empty();
        }
    }

    'outer: loop {
        let mut i_pos = a_iter[0].i_pos;
        loop {
            let mut b_match = true;
            for i in 0..n_term {
                let i_adj = i_pos + i as i64;
                if a_iter[i].i_pos != i_adj {
                    b_match = false;
                    while a_iter[i].i_pos < i_adj {
                        if a_iter[i].next() {
                            break 'outer;
                        }
                    }
                    if a_iter[i].i_pos > i_adj {
                        i_pos = a_iter[i].i_pos - i as i64;
                    }
                }
            }
            if b_match {
                break;
            }
        }

        /* Acrescenta a posição i_pos à saída */
        if !b_first || fts5_pos2offset(i_pos) == 0 {
            writer.append(poslist, i_pos);
        }

        for r in a_iter.iter_mut() {
            if r.next() {
                break 'outer;
            }
        }
    }
    !poslist.p.is_empty()
}

/// `Fts5LookaheadReader`.
struct LookaheadReader<'a> {
    a: &'a [u8],
    i: i32,
    i_pos: i64,
    i_lookahead: i64,
}

impl<'a> LookaheadReader<'a> {
    /// `fts5LookaheadReaderInit`.
    fn new(a: &'a [u8]) -> LookaheadReader<'a> {
        let mut p = LookaheadReader { a, i: 0, i_pos: 0, i_lookahead: 0 };
        p.next();
        p.next();
        p
    }

    /// `fts5LookaheadReaderNext`: verdadeiro no fim.
    fn next(&mut self) -> bool {
        self.i_pos = self.i_lookahead;
        if fts5_poslist_next64(self.a, &mut self.i, &mut self.i_lookahead) != 0 {
            self.i_lookahead = FTS5_LOOKAHEAD_EOF;
        }
        self.i_pos == FTS5_LOOKAHEAD_EOF
    }
}

/// `fts5ExprNearIsMatch`: o nearset tem mais de uma frase e todas apontam para a mesma linha, com
/// as poslists preenchidas. Testa se a linha tem ocorrências de cada frase próximas o bastante.
/// Se tem, a poslist de cada frase fica só com as entradas que cumprem a restrição.
fn near_is_match(
    phrases: &mut [Fts5ExprPhrase],
    ap_phrase: &[PhraseId],
    n_near: i32,
) -> bool {
    let n = ap_phrase.len();
    debug_assert!(n > 1);

    /* No C a nova poslist é escrita por cima da velha enquanto é lida; aqui a velha é movida. */
    let olds: Vec<Vec<u8>> = ap_phrase
        .iter()
        .map(|&pid| std::mem::take(&mut phrases[pid].poslist.p))
        .collect();
    let mut readers: Vec<LookaheadReader<'_>> = olds.iter().map(|o| LookaheadReader::new(o)).collect();
    let mut writers = vec![Fts5PoslistWriter::default(); n];
    let mut outs: Vec<Fts5Buffer> = vec![Fts5Buffer::new(); n];
    let n_terms: Vec<i64> = ap_phrase.iter().map(|&pid| phrases[pid].a_term.len() as i64).collect();

    'outer: loop {
        /* Avança os iteradores até um conjunto de entradas que juntas formam um casamento. */
        let mut i_max = readers[0].i_pos;
        loop {
            let mut b_match = true;
            for i in 0..n {
                let i_min = i_max - n_terms[i] - n_near as i64;
                let pos = &mut readers[i];
                if pos.i_pos < i_min || pos.i_pos > i_max {
                    b_match = false;
                    while pos.i_pos < i_min {
                        if pos.next() {
                            break 'outer;
                        }
                    }
                    if pos.i_pos > i_max {
                        i_max = pos.i_pos;
                    }
                }
            }
            if b_match {
                break;
            }
        }

        /* Acrescenta uma entrada a cada lista de saída */
        for i in 0..n {
            let i_pos = readers[i].i_pos;
            if outs[i].p.is_empty() || i_pos != writers[i].i_prev {
                writers[i].append(&mut outs[i], i_pos);
            }
        }

        let mut i_adv = 0usize;
        let mut i_min = readers[0].i_lookahead;
        for (i, r) in readers.iter().enumerate() {
            if r.i_lookahead < i_min {
                i_min = r.i_lookahead;
                i_adv = i;
            }
        }
        if readers[i_adv].next() {
            break;
        }
    }

    let b_ret = !outs[0].p.is_empty();
    drop(readers);
    for (i, &pid) in ap_phrase.iter().enumerate() {
        phrases[pid].poslist = std::mem::take(&mut outs[i]);
    }
    b_ret
}

/// `fts5ExprAdvanceto`: avança `it` até um valor igual ou depois de `*pi_last`. Se passou, atualiza
/// `*pi_last`. Devolve verdadeiro (com `*pb_eof` ou `*prc`) se chegou ao fim ou houve erro.
fn expr_advanceto(
    cx: &mut Cx<'_>,
    it: &mut Fts5IndexIter,
    b_desc: bool,
    pi_last: &mut i64,
    prc: &mut i32,
    pb_eof: &mut bool,
) -> bool {
    let i_last = *pi_last;
    let mut i_rowid = it.rowid();
    if (!b_desc && i_last > i_rowid) || (b_desc && i_last < i_rowid) {
        let rc = it.next_from(cx.idx, cx.db, cx.cfg, i_last);
        if rc != SQLITE_OK || it.eof() {
            *prc = rc;
            *pb_eof = true;
            return true;
        }
        i_rowid = it.rowid();
        debug_assert!((!b_desc && i_rowid >= i_last) || (b_desc && i_rowid <= i_last));
    }
    *pi_last = i_rowid;
    false
}

/// `fts5ExprSynonymAdvanceto`: o equivalente de `expr_advanceto` para uma lista de sinônimos.
/// Devolve verdadeiro no fim ou em erro.
fn synonym_advanceto(
    cx: &mut Cx<'_>,
    p_term: &mut Fts5ExprTerm,
    b_desc: bool,
    pi_last: &mut i64,
    prc: &mut i32,
) -> bool {
    let mut rc = SQLITE_OK;
    let i_last = *pi_last;
    let mut b_eof = false;

    let mut p = Some(&mut *p_term);
    while let Some(t) = p {
        if rc != SQLITE_OK {
            break;
        }
        if let Some(it) = t.p_iter.as_mut() {
            if !it.eof() {
                let i_rowid = it.rowid();
                if (!b_desc && i_last > i_rowid) || (b_desc && i_last < i_rowid) {
                    rc = it.next_from(cx.idx, cx.db, cx.cfg, i_last);
                }
            }
        }
        p = t.p_synonym.as_deref_mut();
    }

    if rc != SQLITE_OK {
        *prc = rc;
        b_eof = true;
    } else {
        *pi_last = synonym_rowid(p_term, b_desc, Some(&mut b_eof));
    }
    b_eof
}

/// `fts5RowidCmp`: o sinal de `l - r` em ordem crescente, ou o oposto em ordem decrescente.
fn rowid_cmp(b_desc: bool, l: i64, r: i64) -> i32 {
    if !b_desc {
        if l < r {
            return -1;
        }
        (l > r) as i32
    } else {
        if l > r {
            return -1;
        }
        (l < r) as i32
    }
}

// ---------------------------------------------------------------------------------------------
// Os nós
// ---------------------------------------------------------------------------------------------

/// `fts5ExprNearTest`: o nó `STRING` aponta para a linha `i_rowid`. Testa se a linha casa e
/// preenche as poslists das frases. Devolve se casou.
fn near_test(ex: &mut Fts5Expr, e_detail: i32, node: NodeId) -> bool {
    let Fts5Expr { nodes, phrases, .. } = ex;
    let i_rowid = nodes[node].i_rowid;
    let near = nodes[node].near();

    if e_detail != FTS5_DETAIL_FULL {
        let ph = &mut phrases[near.ap_phrase[0]];
        let mut hit = false;
        let mut p = ph.a_term.first();
        while let Some(t) = p {
            if let Some(it) = t.p_iter.as_ref() {
                if !it.eof() && it.rowid() == i_rowid && !it.data().is_empty() {
                    hit = true;
                }
            }
            p = t.p_synonym.as_deref();
        }
        ph.poslist.p.clear();
        if hit {
            ph.poslist.p.push(0);
        }
        hit
    } else {
        /* Confere que cada frase do nearset casa a linha corrente, preenchendo as poslists. */
        let n_phrase = near.ap_phrase.len();
        let mut i = 0usize;
        while i < n_phrase {
            let ph = &mut phrases[near.ap_phrase[i]];
            if ph.a_term.len() > 1
                || ph.a_term[0].p_synonym.is_some()
                || near.p_colset.is_some()
                || ph.a_term[0].b_first
            {
                if !phrase_is_match(i_rowid, ph) {
                    break;
                }
            } else {
                let data = ph.a_term[0].p_iter.as_ref().map_or(&[][..], |it| it.data()).to_vec();
                ph.poslist.set(&data);
            }
            i += 1;
        }

        i == n_phrase && (i == 1 || near_is_match(phrases, &near.ap_phrase, near.n_near))
    }
}

/// `fts5ExprNearInitAll`: abre os iteradores de todos os termos do nó. Se algum termo não casa
/// nenhum documento, para na hora e liga o `bEof`.
fn near_init_all(ex: &mut Fts5Expr, cx: &mut Cx<'_>, node: NodeId) -> i32 {
    let Fts5Expr { nodes, phrases, b_desc, .. } = ex;
    let b_desc = *b_desc;
    debug_assert!(!nodes[node].b_nomatch);
    let near = nodes[node].near().clone();

    for &pid in &near.ap_phrase {
        let ph = &mut phrases[pid];
        if ph.a_term.is_empty() {
            nodes[node].b_eof = true;
            return SQLITE_OK;
        }
        for pterm in ph.a_term.iter_mut() {
            let flags = (if pterm.b_prefix { FTS5INDEX_QUERY_PREFIX } else { 0 })
                | (if b_desc { FTS5INDEX_QUERY_DESC } else { 0 });
            let mut b_hit = false;

            let mut p = Some(&mut *pterm);
            while let Some(t) = p {
                if let Some(old) = t.p_iter.take() {
                    old.close(cx.idx, cx.db);
                }
                let term = &t.p_term[..t.n_query_term as usize];
                match cx.idx.query(cx.db, cx.cfg, term, flags, near.p_colset.as_ref()) {
                    Err(rc) => return rc,
                    Ok(it) => {
                        if !it.eof() {
                            b_hit = true;
                        }
                        t.p_iter = Some(it);
                    }
                }
                p = t.p_synonym.as_deref_mut();
            }

            if !b_hit {
                nodes[node].b_eof = true;
                return SQLITE_OK;
            }
        }
    }

    nodes[node].b_eof = false;
    SQLITE_OK
}

/// `fts5ExprSetEof`: liga o `bEof` do nó e de todos os descendentes.
fn set_eof(ex: &mut Fts5Expr, node: NodeId) {
    ex.nodes[node].b_eof = true;
    ex.nodes[node].b_nomatch = false;
    for i in 0..ex.nodes[node].ap_child.len() {
        let c = ex.nodes[node].ap_child[i];
        set_eof(ex, c);
    }
}

/// `fts5ExprNodeZeroPoslist`: zera a poslist de todas as frases do nó (e dos descendentes).
fn node_zero_poslist(ex: &mut Fts5Expr, node: NodeId) {
    if ex.nodes[node].is_string() {
        for i in 0..ex.nodes[node].near().ap_phrase.len() {
            let pid = ex.nodes[node].near().ap_phrase[i];
            ex.phrases[pid].poslist.p.clear();
        }
    } else {
        for i in 0..ex.nodes[node].ap_child.len() {
            let c = ex.nodes[node].ap_child[i];
            node_zero_poslist(ex, c);
        }
    }
}

/// `fts5NodeCompare`: `*p1 - *p2` na ordem da iteração; um nó no fim é o maior de todos.
fn node_compare(ex: &Fts5Expr, p1: NodeId, p2: NodeId) -> i32 {
    let (n1, n2) = (&ex.nodes[p1], &ex.nodes[p2]);
    if n2.b_eof {
        return -1;
    }
    if n1.b_eof {
        return 1;
    }
    rowid_cmp(ex.b_desc, n1.i_rowid, n2.i_rowid)
}

/// `fts5ExprNodeTest_STRING`: todos os iteradores de termo do nó são válidos. Confere se apontam
/// para o mesmo rowid e, se não, avança até apontarem. Chegar ao fim não é erro.
fn node_test_string(ex: &mut Fts5Expr, cx: &mut Cx<'_>, node: NodeId) -> i32 {
    let mut rc = SQLITE_OK;
    let e_detail = cx.cfg.e_detail;
    let b_desc = ex.b_desc;
    let i_last_final;

    {
        let Fts5Expr { nodes, phrases, .. } = &mut *ex;
        let near = nodes[node].near().clone();
        let p_left = &phrases[near.ap_phrase[0]];
        debug_assert!(
            near.ap_phrase.len() > 1
                || p_left.a_term.len() > 1
                || p_left.a_term[0].p_synonym.is_some()
                || p_left.a_term[0].b_first
        );

        /* O rowid "mais adiantado" que algum iterador aponta. */
        let mut i_last = if p_left.a_term[0].p_synonym.is_some() {
            synonym_rowid(&p_left.a_term[0], b_desc, None)
        } else {
            p_left.a_term[0].p_iter.as_ref().map_or(0, |it| it.rowid())
        };

        loop {
            let mut b_match = true;
            for &pid in &near.ap_phrase {
                for p_term in phrases[pid].a_term.iter_mut() {
                    if p_term.p_synonym.is_some() {
                        let i_rowid = synonym_rowid(p_term, b_desc, None);
                        if i_rowid == i_last {
                            continue;
                        }
                        b_match = false;
                        if synonym_advanceto(cx, p_term, b_desc, &mut i_last, &mut rc) {
                            nodes[node].b_nomatch = false;
                            nodes[node].b_eof = true;
                            return rc;
                        }
                    } else {
                        let Some(it) = p_term.p_iter.as_mut() else {
                            continue;
                        };
                        if it.rowid() == i_last || it.eof() {
                            continue;
                        }
                        b_match = false;
                        if expr_advanceto(
                            cx,
                            it,
                            b_desc,
                            &mut i_last,
                            &mut rc,
                            &mut nodes[node].b_eof,
                        ) {
                            return rc;
                        }
                    }
                }
            }
            if b_match {
                break;
            }
        }
        i_last_final = i_last;
    }

    ex.nodes[node].i_rowid = i_last_final;
    let matched = near_test(ex, e_detail, node);
    ex.nodes[node].b_nomatch = !matched;
    debug_assert!(!ex.nodes[node].b_eof || !ex.nodes[node].b_nomatch);
    rc
}

/// `fts5ExprNodeNext_STRING`: avança o primeiro iterador de termo da primeira frase e retesta.
fn node_next_string(
    ex: &mut Fts5Expr,
    cx: &mut Cx<'_>,
    node: NodeId,
    b_from_valid: bool,
    i_from: i64,
) -> i32 {
    let b_desc = ex.b_desc;
    let mut rc = SQLITE_OK;
    {
        let Fts5Expr { nodes, phrases, .. } = &mut *ex;
        let pid = nodes[node].near().ap_phrase[0];
        let p_term = &mut phrases[pid].a_term[0];
        nodes[node].b_nomatch = false;

        if p_term.p_synonym.is_some() {
            let mut b_eof = true;
            /* O menor rowid em que algum sinônimo aponta. */
            let i_rowid = synonym_rowid(p_term, b_desc, None);

            /* Avança cada iterador que aponta para i_rowid, ou (com `i_from`) os que apontam
            ** antes de `i_from`. */
            let mut p = Some(&mut *p_term);
            while let Some(t) = p {
                if let Some(it) = t.p_iter.as_mut() {
                    if !it.eof() {
                        let ii = it.rowid();
                        if ii == i_rowid
                            || (b_from_valid && ii != i_from && ((ii > i_from) == b_desc))
                        {
                            rc = if b_from_valid {
                                it.next_from(cx.idx, cx.db, cx.cfg, i_from)
                            } else {
                                it.next(cx.idx, cx.db, cx.cfg)
                            };
                            if rc != SQLITE_OK {
                                break;
                            }
                            if !it.eof() {
                                b_eof = false;
                            }
                        } else {
                            b_eof = false;
                        }
                    }
                }
                p = t.p_synonym.as_deref_mut();
            }

            /* Liga o EOF se todos os iteradores de sinônimo acabaram ou houve erro. */
            nodes[node].b_eof = rc != SQLITE_OK || b_eof;
        } else if let Some(it) = p_term.p_iter.as_mut() {
            debug_assert!(nodes[node].is_string());
            rc = if b_from_valid {
                it.next_from(cx.idx, cx.db, cx.cfg, i_from)
            } else {
                it.next(cx.idx, cx.db, cx.cfg)
            };
            nodes[node].b_eof = rc != SQLITE_OK || it.eof();
        }
    }

    if !ex.nodes[node].b_eof {
        debug_assert!(rc == SQLITE_OK);
        rc = node_test_string(ex, cx, node);
    }
    rc
}

/// `fts5ExprNodeTest_TERM`: o nó é um único termo; usa a poslist do iterador do índice.
fn node_test_term(ex: &mut Fts5Expr, e_detail: i32, node: NodeId) -> i32 {
    let Fts5Expr { nodes, phrases, .. } = ex;
    debug_assert!(nodes[node].e_type == FTS5_TERM);
    let pid = nodes[node].near().ap_phrase[0];
    let ph = &mut phrases[pid];
    debug_assert!(nodes[node].near().ap_phrase.len() == 1 && ph.a_term.len() == 1);
    debug_assert!(ph.a_term[0].p_synonym.is_none());

    let (n_data, i_rowid) =
        ph.a_term[0].p_iter.as_ref().map_or((0, 0), |it| (it.data().len(), it.rowid()));
    if e_detail == FTS5_DETAIL_FULL {
        let data = ph.a_term[0].p_iter.as_ref().map_or(&[][..], |it| it.data()).to_vec();
        ph.poslist.p = data;
    } else {
        /* Só o comprimento importa fora de detail=full. */
        ph.poslist.p.clear();
        ph.poslist.p.resize(n_data, 0);
    }
    nodes[node].i_rowid = i_rowid;
    nodes[node].b_nomatch = ph.poslist.p.is_empty();
    SQLITE_OK
}

/// `fts5ExprNodeNext_TERM`.
fn node_next_term(
    ex: &mut Fts5Expr,
    cx: &mut Cx<'_>,
    node: NodeId,
    b_from_valid: bool,
    i_from: i64,
) -> i32 {
    let pid = ex.nodes[node].near().ap_phrase[0];
    debug_assert!(!ex.nodes[node].b_eof);
    let (rc, at_eof) = match ex.phrases[pid].a_term[0].p_iter.as_mut() {
        Some(it) => {
            let rc = if b_from_valid {
                it.next_from(cx.idx, cx.db, cx.cfg, i_from)
            } else {
                it.next(cx.idx, cx.db, cx.cfg)
            };
            (rc, it.eof())
        }
        None => (SQLITE_OK, true),
    };
    if rc == SQLITE_OK && !at_eof {
        node_test_term(ex, cx.cfg.e_detail, node)
    } else {
        ex.nodes[node].b_eof = true;
        ex.nodes[node].b_nomatch = false;
        rc
    }
}

/// `fts5ExprNodeTest_OR`.
fn node_test_or(ex: &mut Fts5Expr, node: NodeId) {
    let mut p_next = ex.nodes[node].ap_child[0];
    for i in 1..ex.nodes[node].ap_child.len() {
        let p_child = ex.nodes[node].ap_child[i];
        let cmp = node_compare(ex, p_next, p_child);
        if cmp > 0 || (cmp == 0 && !ex.nodes[p_child].b_nomatch) {
            p_next = p_child;
        }
    }
    let (rowid, eof, nomatch) =
        (ex.nodes[p_next].i_rowid, ex.nodes[p_next].b_eof, ex.nodes[p_next].b_nomatch);
    let n = &mut ex.nodes[node];
    n.i_rowid = rowid;
    n.b_eof = eof;
    n.b_nomatch = nomatch;
}

/// `fts5ExprNodeNext_OR`.
fn node_next_or(
    ex: &mut Fts5Expr,
    cx: &mut Cx<'_>,
    node: NodeId,
    b_from_valid: bool,
    i_from: i64,
) -> i32 {
    let i_last = ex.nodes[node].i_rowid;
    for i in 0..ex.nodes[node].ap_child.len() {
        let p1 = ex.nodes[node].ap_child[i];
        debug_assert!(
            ex.nodes[p1].b_eof || rowid_cmp(ex.b_desc, ex.nodes[p1].i_rowid, i_last) >= 0
        );
        if !ex.nodes[p1].b_eof
            && (ex.nodes[p1].i_rowid == i_last
                || (b_from_valid && rowid_cmp(ex.b_desc, ex.nodes[p1].i_rowid, i_from) < 0))
        {
            let rc = node_next(ex, cx, p1, b_from_valid, i_from);
            if rc != SQLITE_OK {
                ex.nodes[node].b_nomatch = false;
                return rc;
            }
        }
    }
    node_test_or(ex, node);
    SQLITE_OK
}

/// `fts5ExprNodeTest_AND`.
fn node_test_and(ex: &mut Fts5Expr, cx: &mut Cx<'_>, p_and: NodeId) -> i32 {
    let mut i_last = ex.nodes[p_and].i_rowid;
    debug_assert!(!ex.nodes[p_and].b_eof);
    loop {
        ex.nodes[p_and].b_nomatch = false;
        let mut b_match = true;
        for i_child in 0..ex.nodes[p_and].ap_child.len() {
            let p_child = ex.nodes[p_and].ap_child[i_child];
            let cmp = rowid_cmp(ex.b_desc, i_last, ex.nodes[p_child].i_rowid);
            if cmp > 0 {
                /* Avança o filho até apontar para i_last ou depois */
                let rc = node_next(ex, cx, p_child, true, i_last);
                if rc != SQLITE_OK {
                    ex.nodes[p_and].b_nomatch = false;
                    return rc;
                }
            }

            /* Se o filho chegou ao fim, o AND também. Senão o filho avançou pelo menos até
            ** i_last, e se não está em i_last o rowid dele é o novo mais adiantado. */
            debug_assert!(
                ex.nodes[p_child].b_eof
                    || rowid_cmp(ex.b_desc, i_last, ex.nodes[p_child].i_rowid) <= 0
            );
            if ex.nodes[p_child].b_eof {
                set_eof(ex, p_and);
                b_match = true;
                break;
            } else if i_last != ex.nodes[p_child].i_rowid {
                b_match = false;
                i_last = ex.nodes[p_child].i_rowid;
            }

            if ex.nodes[p_child].b_nomatch {
                ex.nodes[p_and].b_nomatch = true;
            }
        }
        if b_match {
            break;
        }
    }

    if ex.nodes[p_and].b_nomatch && p_and != ex.p_root {
        node_zero_poslist(ex, p_and);
    }
    ex.nodes[p_and].i_rowid = i_last;
    SQLITE_OK
}

/// `fts5ExprNodeNext_AND`.
fn node_next_and(
    ex: &mut Fts5Expr,
    cx: &mut Cx<'_>,
    node: NodeId,
    b_from_valid: bool,
    i_from: i64,
) -> i32 {
    let c0 = ex.nodes[node].ap_child[0];
    let mut rc = node_next(ex, cx, c0, b_from_valid, i_from);
    if rc == SQLITE_OK {
        rc = node_test_and(ex, cx, node);
    } else {
        ex.nodes[node].b_nomatch = false;
    }
    rc
}

/// `fts5ExprNodeTest_NOT`.
fn node_test_not(ex: &mut Fts5Expr, cx: &mut Cx<'_>, node: NodeId) -> i32 {
    let mut rc = SQLITE_OK;
    let p1 = ex.nodes[node].ap_child[0];
    let p2 = ex.nodes[node].ap_child[1];
    debug_assert!(ex.nodes[node].ap_child.len() == 2);

    while rc == SQLITE_OK && !ex.nodes[p1].b_eof {
        let mut cmp = node_compare(ex, p1, p2);
        if cmp > 0 {
            let i_from = ex.nodes[p1].i_rowid;
            rc = node_next(ex, cx, p2, true, i_from);
            cmp = node_compare(ex, p1, p2);
        }
        debug_assert!(rc != SQLITE_OK || cmp <= 0);
        if cmp != 0 || ex.nodes[p2].b_nomatch {
            break;
        }
        rc = node_next(ex, cx, p1, false, 0);
    }
    let (eof, nomatch, rowid) =
        (ex.nodes[p1].b_eof, ex.nodes[p1].b_nomatch, ex.nodes[p1].i_rowid);
    let n = &mut ex.nodes[node];
    n.b_eof = eof;
    n.b_nomatch = nomatch;
    n.i_rowid = rowid;
    if eof {
        node_zero_poslist(ex, p2);
    }
    rc
}

/// `fts5ExprNodeNext_NOT`.
fn node_next_not(
    ex: &mut Fts5Expr,
    cx: &mut Cx<'_>,
    node: NodeId,
    b_from_valid: bool,
    i_from: i64,
) -> i32 {
    let c0 = ex.nodes[node].ap_child[0];
    let mut rc = node_next(ex, cx, c0, b_from_valid, i_from);
    if rc == SQLITE_OK {
        rc = node_test_not(ex, cx, node);
    }
    if rc != SQLITE_OK {
        ex.nodes[node].b_nomatch = false;
    }
    rc
}

/// `fts5ExprNodeNext`: o `(b)->xNext((a), (b), (c), (d))` do C.
fn node_next(
    ex: &mut Fts5Expr,
    cx: &mut Cx<'_>,
    node: NodeId,
    b_from_valid: bool,
    i_from: i64,
) -> i32 {
    match ex.nodes[node].x_next {
        XNext::Term => node_next_term(ex, cx, node, b_from_valid, i_from),
        XNext::String => node_next_string(ex, cx, node, b_from_valid, i_from),
        XNext::Or => node_next_or(ex, cx, node, b_from_valid, i_from),
        XNext::And => node_next_and(ex, cx, node, b_from_valid, i_from),
        XNext::Not => node_next_not(ex, cx, node, b_from_valid, i_from),
        XNext::Nil => {
            debug_assert!(false, "xNext nulo nunca é chamado");
            SQLITE_OK
        }
    }
}

/// `fts5ExprNodeTest`: se o nó aponta para um casamento não faz nada; senão o avança até um
/// casamento ou o fim.
fn node_test(ex: &mut Fts5Expr, cx: &mut Cx<'_>, node: NodeId) -> i32 {
    let mut rc = SQLITE_OK;
    if !ex.nodes[node].b_eof {
        match ex.nodes[node].e_type {
            FTS5_STRING => rc = node_test_string(ex, cx, node),
            FTS5_TERM => rc = node_test_term(ex, cx.cfg.e_detail, node),
            FTS5_AND => rc = node_test_and(ex, cx, node),
            FTS5_OR => node_test_or(ex, node),
            _ => {
                debug_assert!(ex.nodes[node].e_type == FTS5_NOT);
                rc = node_test_not(ex, cx, node);
            }
        }
    }
    rc
}

/// `fts5ExprNodeFirst`: põe o nó no primeiro casamento (ou liga o `bEof`).
fn node_first(ex: &mut Fts5Expr, cx: &mut Cx<'_>, node: NodeId) -> i32 {
    let mut rc = SQLITE_OK;
    ex.nodes[node].b_eof = false;
    ex.nodes[node].b_nomatch = false;

    if ex.nodes[node].is_string() {
        /* Inicia todos os iteradores de termo do NEAR. */
        rc = near_init_all(ex, cx, node);
    } else if ex.nodes[node].x_next == XNext::Nil {
        ex.nodes[node].b_eof = true;
    } else {
        let mut n_eof = 0;
        let n_child = ex.nodes[node].ap_child.len();
        let mut i = 0;
        while i < n_child && rc == SQLITE_OK {
            let p_child = ex.nodes[node].ap_child[i];
            rc = node_first(ex, cx, p_child);
            n_eof += ex.nodes[p_child].b_eof as usize;
            i += 1;
        }
        ex.nodes[node].i_rowid = ex.nodes[ex.nodes[node].ap_child[0]].i_rowid;

        match ex.nodes[node].e_type {
            FTS5_AND => {
                if n_eof > 0 {
                    set_eof(ex, node);
                }
            }
            FTS5_OR => {
                if n_child == n_eof {
                    set_eof(ex, node);
                }
            }
            _ => {
                debug_assert!(ex.nodes[node].e_type == FTS5_NOT);
                ex.nodes[node].b_eof = ex.nodes[ex.nodes[node].ap_child[0]].b_eof;
            }
        }
    }

    if rc == SQLITE_OK {
        rc = node_test(ex, cx, node);
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// A interface do Fts5Expr
// ---------------------------------------------------------------------------------------------

/// `fts5ExprColsetTest`: a coluna `i_col` está no colset?
fn expr_colset_test(colset: &Fts5Colset, i_col: i32) -> bool {
    colset.ai_col.contains(&i_col)
}

/// `fts5ExprClearPoslists`.
fn expr_clear_poslists(ex: &mut Fts5Expr, node: NodeId) {
    if ex.nodes[node].is_string() {
        let pid = ex.nodes[node].near().ap_phrase[0];
        ex.phrases[pid].poslist.p.clear();
    } else {
        for i in 0..ex.nodes[node].ap_child.len() {
            let c = ex.nodes[node].ap_child[i];
            expr_clear_poslists(ex, c);
        }
    }
}

/// `fts5ExprCheckPoslists`.
fn expr_check_poslists(ex: &mut Fts5Expr, node: NodeId, i_rowid: i64) -> bool {
    ex.nodes[node].i_rowid = i_rowid;
    ex.nodes[node].b_eof = false;
    match ex.nodes[node].e_type {
        FTS5_TERM | FTS5_STRING => {
            let pid = ex.nodes[node].near().ap_phrase[0];
            return !ex.phrases[pid].poslist.p.is_empty();
        }
        FTS5_AND => {
            for i in 0..ex.nodes[node].ap_child.len() {
                let c = ex.nodes[node].ap_child[i];
                if !expr_check_poslists(ex, c, i_rowid) {
                    expr_clear_poslists(ex, node);
                    return false;
                }
            }
        }
        FTS5_OR => {
            let mut b_ret = false;
            for i in 0..ex.nodes[node].ap_child.len() {
                let c = ex.nodes[node].ap_child[i];
                if expr_check_poslists(ex, c, i_rowid) {
                    b_ret = true;
                }
            }
            return b_ret;
        }
        _ => {
            debug_assert!(ex.nodes[node].e_type == FTS5_NOT);
            let (c0, c1) = (ex.nodes[node].ap_child[0], ex.nodes[node].ap_child[1]);
            if !expr_check_poslists(ex, c0, i_rowid) || expr_check_poslists(ex, c1, i_rowid) {
                expr_clear_poslists(ex, node);
                return false;
            }
        }
    }
    true
}

impl Fts5Expr {
    /// `sqlite3Fts5ExprFirst`: começa a iterar pelos documentos do índice `idx` que casam com a
    /// expressão, em ordem decrescente de rowid se `b_desc`. O primeiro documento é o de menor
    /// rowid maior ou igual a `i_first` (ou, em ordem decrescente, o de maior rowid menor ou igual).
    /// Não casar nada não é erro.
    pub fn first(
        &mut self,
        db: &mut Connection,
        idx: &mut Fts5Index,
        cfg: &mut Fts5Config,
        i_first: i64,
        b_desc: bool,
    ) -> i32 {
        let mut cx = Cx { db, idx, cfg };
        let root = self.p_root;
        self.b_desc = b_desc;
        let mut rc = node_first(self, &mut cx, root);

        /* Se não está no fim mas o rowid corrente vem antes de i_first na ordem da iteração, vai
        ** para o documento i_first ou depois. */
        if rc == SQLITE_OK
            && !self.nodes[root].b_eof
            && rowid_cmp(b_desc, self.nodes[root].i_rowid, i_first) < 0
        {
            rc = node_next(self, &mut cx, root, true, i_first);
        }

        /* Se o iterador não está num casamento de verdade, avança até estar. */
        while self.nodes[root].b_nomatch && rc == SQLITE_OK {
            debug_assert!(!self.nodes[root].b_eof);
            rc = node_next(self, &mut cx, root, false, 0);
        }
        rc
    }

    /// `sqlite3Fts5ExprNext`: o próximo documento; o que passa de `i_last` na ordem da iteração
    /// liga o fim.
    pub fn next(
        &mut self,
        db: &mut Connection,
        idx: &mut Fts5Index,
        cfg: &mut Fts5Config,
        i_last: i64,
    ) -> i32 {
        let mut cx = Cx { db, idx, cfg };
        let root = self.p_root;
        debug_assert!(!self.nodes[root].b_eof && !self.nodes[root].b_nomatch);
        let mut rc;
        loop {
            rc = node_next(self, &mut cx, root, false, 0);
            debug_assert!(
                !self.nodes[root].b_nomatch || (rc == SQLITE_OK && !self.nodes[root].b_eof)
            );
            if !self.nodes[root].b_nomatch {
                break;
            }
        }
        if rowid_cmp(self.b_desc, self.nodes[root].i_rowid, i_last) > 0 {
            self.nodes[root].b_eof = true;
        }
        rc
    }

    /// `sqlite3Fts5ExprEof`.
    pub fn eof(&self) -> bool {
        self.nodes[self.p_root].b_eof
    }

    /// `sqlite3Fts5ExprRowid`.
    pub fn rowid(&self) -> i64 {
        self.nodes[self.p_root].i_rowid
    }

    /// `sqlite3Fts5ExprFree`: fecha os iteradores de termo (o que fecha o leitor do índice) e
    /// solta a expressão. Uma expressão que nunca iterou pode ser só solta (`Drop`).
    pub fn free(mut self, db: &mut Connection, idx: &mut Fts5Index) {
        for ph in self.phrases.iter_mut() {
            for term in ph.a_term.iter_mut() {
                let mut p = Some(term);
                while let Some(t) = p {
                    if let Some(it) = t.p_iter.take() {
                        it.close(idx, db);
                    }
                    p = t.p_synonym.as_deref_mut();
                }
            }
        }
    }

    /// `sqlite3Fts5ExprPhraseCount`.
    pub fn phrase_count(&self) -> i32 {
        self.ap_expr_phrase.len() as i32
    }

    /// `sqlite3Fts5ExprPhraseSize`: o número de termos da frase `i_phrase` (0 fora da faixa).
    pub fn phrase_size(&self, i_phrase: i32) -> i32 {
        match usize::try_from(i_phrase).ok().and_then(|i| self.ap_expr_phrase.get(i)) {
            Some(&pid) => self.phrases[pid].a_term.len() as i32,
            None => 0,
        }
    }

    /// `sqlite3Fts5ExprPoslist`: a poslist corrente da frase `i_phrase` (vazia se a frase não
    /// casa a linha corrente). `i_phrase` precisa estar na faixa.
    pub fn poslist(&self, i_phrase: i32) -> &[u8] {
        let ph = &self.phrases[self.ap_expr_phrase[i_phrase as usize]];
        let node = &self.nodes[ph.p_node];
        if !node.b_eof && node.i_rowid == self.nodes[self.p_root].i_rowid {
            &ph.poslist.p[..]
        } else {
            &[]
        }
    }

    /// `sqlite3Fts5ExprClearPoslists`: zera as poslists de todas as frases (só `detail=col` e
    /// `detail=none`, em que toda frase tem no máximo um token). `b_live` diz que a expressão
    /// pode apontar para uma entrada real. Devolve o estado de cada frase para
    /// `populate_poslists`.
    pub fn clear_poslists(&mut self, b_live: bool) -> Vec<Fts5PoslistPopulator> {
        let mut ret = vec![Fts5PoslistPopulator::default(); self.ap_expr_phrase.len()];
        let root_rowid = self.nodes[self.p_root].i_rowid;
        for (i, &pid) in self.ap_expr_phrase.iter().enumerate() {
            let node = &self.nodes[self.phrases[pid].p_node];
            debug_assert!(self.phrases[pid].a_term.len() <= 1);
            let buf = &mut self.phrases[pid].poslist;
            if b_live && (buf.p.is_empty() || node.i_rowid != root_rowid || node.b_eof) {
                ret[i].b_miss = true;
            } else {
                buf.p.clear();
            }
        }
        ret
    }

    /// `sqlite3Fts5ExprPopulatePoslists`: tokeniza o texto `z` da coluna `i_col` da linha
    /// corrente e acrescenta as posições dos tokens que casam às poslists das frases.
    pub fn populate_poslists(
        &mut self,
        idx: &mut Fts5Index,
        cfg: &Fts5Config,
        a_populator: &mut [Fts5PoslistPopulator],
        i_col: i32,
        z: Option<&[u8]>,
    ) -> i32 {
        let mut i_off: i64 = ((i_col as i64) << 32) - 1;
        let b_tokendata = cfg.b_tokendata != 0;
        let i_rowid = self.nodes[self.p_root].i_rowid;

        for (i, &pid) in self.ap_expr_phrase.iter().enumerate() {
            let colset = self.nodes[self.phrases[pid].p_node].near().p_colset.as_ref();
            a_populator[i].b_ok = !(colset.is_some_and(|c| !expr_colset_test(c, i_col))
                || a_populator[i].b_miss);
        }

        let Fts5Expr { phrases, ap_expr_phrase, .. } = self;
        cfg.tokenize(FTS5_TOKENIZE_DOCUMENT, z, &mut |tflags, p_token, _, _| {
            let mut n_query = p_token.len();
            if n_query > FTS5_MAX_TOKEN_SIZE {
                n_query = FTS5_MAX_TOKEN_SIZE;
            }
            if b_tokendata {
                n_query = query_term_len(&p_token[..n_query]);
            }
            if (tflags & FTS5_TOKEN_COLOCATED) == 0 {
                i_off += 1;
            }
            for (i, &pid) in ap_expr_phrase.iter().enumerate() {
                if !a_populator[i].b_ok {
                    continue;
                }
                let Fts5ExprPhrase { poslist, a_term, .. } = &mut phrases[pid];
                let mut p = a_term.first_mut();
                while let Some(p_t) = p {
                    let nq = p_t.n_query_term as usize;
                    if (nq == n_query || (nq < n_query && p_t.b_prefix))
                        && p_t.p_term[..nq] == p_token[..nq]
                    {
                        let mut rc = a_populator[i].writer.append(poslist, i_off);
                        if rc == SQLITE_OK && b_tokendata && !p_t.b_prefix {
                            let i_c = (i_off >> 32) as i32;
                            let i_tok_off = (i_off & 0x7FFF_FFFF) as i32;
                            if let Some(it) = p_t.p_iter.as_mut() {
                                rc = it.write_tokendata(idx, p_token, i_rowid, i_c, i_tok_off);
                            }
                        }
                        if rc != SQLITE_OK {
                            return rc;
                        }
                        break;
                    }
                    p = p_t.p_synonym.as_deref_mut();
                }
            }
            SQLITE_OK
        })
    }

    /// `sqlite3Fts5ExprCheckPoslists`: confere, para o rowid `i_rowid`, se as poslists
    /// preenchidas por `populate_poslists` formam um casamento da expressão.
    pub fn check_poslists(&mut self, i_rowid: i64) {
        let root = self.p_root;
        expr_check_poslists(self, root, i_rowid);
    }

    /// `sqlite3Fts5ExprPhraseCollist`: a lista de colunas da frase `i_phrase` na linha corrente
    /// (só `detail=columns`); vazia se a frase não casa a linha.
    pub fn phrase_collist(&self, i_phrase: i32) -> Vec<u8> {
        let ph = &self.phrases[self.ap_expr_phrase[i_phrase as usize]];
        let node = &self.nodes[ph.p_node];
        debug_assert!(i_phrase >= 0 && (i_phrase as usize) < self.ap_expr_phrase.len());

        if !node.b_eof
            && node.i_rowid == self.nodes[self.p_root].i_rowid
            && !ph.poslist.p.is_empty()
        {
            let p_term = &ph.a_term[0];
            if p_term.p_synonym.is_some() {
                synonym_list(p_term, node.i_rowid)
            } else {
                p_term.p_iter.as_ref().map_or(Vec::new(), |it| it.data().to_vec())
            }
        } else {
            Vec::new()
        }
    }

    /// `sqlite3Fts5ExprQueryToken`: o token `i_token` da frase `i_phrase` da consulta, ou
    /// `Err(SQLITE_RANGE)`.
    pub fn query_token(&self, i_phrase: i32, i_token: i32) -> Result<Vec<u8>, i32> {
        let ph = usize::try_from(i_phrase)
            .ok()
            .and_then(|i| self.ap_expr_phrase.get(i))
            .map(|&pid| &self.phrases[pid])
            .ok_or(SQLITE_RANGE)?;
        let t = usize::try_from(i_token).ok().and_then(|i| ph.a_term.get(i)).ok_or(SQLITE_RANGE)?;
        Ok(t.p_term[..t.n_full_term as usize].to_vec())
    }

    /// `sqlite3Fts5ExprInstToken`: o token `i_token` da ocorrência da frase `i_phrase` no
    /// documento (`i_rowid`, coluna `i_col`, deslocamento `i_off`). `Ok(None)` é o termo de
    /// prefixo (sem token) ou o tokendata sem mapeamento; `Err(SQLITE_RANGE)` fora da faixa.
    pub fn inst_token(
        &self,
        cfg: &Fts5Config,
        i_rowid: i64,
        i_phrase: i32,
        i_col: i32,
        i_off: i32,
        i_token: i32,
    ) -> Result<Option<Vec<u8>>, i32> {
        let ph = usize::try_from(i_phrase)
            .ok()
            .and_then(|i| self.ap_expr_phrase.get(i))
            .map(|&pid| &self.phrases[pid])
            .ok_or(SQLITE_RANGE)?;
        let t = usize::try_from(i_token).ok().and_then(|i| ph.a_term.get(i)).ok_or(SQLITE_RANGE)?;
        if t.b_prefix {
            return Ok(None);
        }
        if cfg.b_tokendata != 0 {
            Ok(t.p_iter.as_ref().and_then(|it| it.token(i_rowid, i_col, i_off + i_token)))
        } else {
            Ok(Some(t.p_term[..t.n_full_term as usize].to_vec()))
        }
    }

    /// `sqlite3Fts5ExprClearTokens`: esvazia os mapas de token de todos os iteradores.
    pub fn clear_tokens(&mut self) {
        for &pid in &self.ap_expr_phrase {
            let mut p = self.phrases[pid].a_term.first_mut();
            while let Some(t) = p {
                if let Some(it) = t.p_iter.as_mut() {
                    it.clear_tokendata();
                }
                p = t.p_synonym.as_deref_mut();
            }
        }
    }

    /// `sqlite3Fts5ExprClonePhrase`: uma expressão nova com a frase `i_phrase` desta. `cfg` é a
    /// configuração da tabela (o `pExpr->pConfig`). Devolve `Err(SQLITE_RANGE)` fora da faixa.
    pub fn clone_phrase(&self, cfg: &Fts5Config, i_phrase: i32) -> Result<Fts5Expr, i32> {
        let p_orig = usize::try_from(i_phrase)
            .ok()
            .and_then(|i| self.ap_expr_phrase.get(i))
            .map(|&pid| &self.phrases[pid])
            .ok_or(SQLITE_RANGE)?;

        let mut new = Fts5Expr::default();
        let mut near = Fts5ExprNearset::default();
        near.p_colset = self.nodes[p_orig.p_node].near().p_colset.clone();

        let b_tokendata = cfg.b_tokendata != 0;
        let mut p_phrase: Option<PhraseId> = None;
        if !p_orig.a_term.is_empty() {
            for (i, term) in p_orig.a_term.iter().enumerate() {
                let mut tflags = 0;
                let mut p = Some(term);
                while let Some(t) = p {
                    parse_tokenize(
                        &mut new.phrases,
                        &mut p_phrase,
                        b_tokendata,
                        tflags,
                        &t.p_term[..t.n_full_term as usize],
                    );
                    tflags = FTS5_TOKEN_COLOCATED;
                    p = t.p_synonym.as_deref();
                }
                let pid = p_phrase.expect("frase");
                new.phrases[pid].a_term[i].b_prefix = term.b_prefix;
                new.phrases[pid].a_term[i].b_first = term.b_first;
            }
        } else {
            /* Um token ou frase entre aspas sem nenhum caractere de token (`MATCH '""'`). */
            new.phrases.push(Fts5ExprPhrase::default());
            p_phrase = Some(0);
        }

        let pid = p_phrase.expect("frase");
        near.ap_phrase.push(pid);
        let (e_type, x_next) = if p_orig.a_term.len() == 1
            && p_orig.a_term[0].p_synonym.is_none()
            && !p_orig.a_term[0].b_first
        {
            (FTS5_TERM, XNext::Term)
        } else {
            (FTS5_STRING, XNext::String)
        };
        new.nodes.push(Fts5ExprNode { e_type, x_next, p_near: Some(near), ..Default::default() });
        new.p_root = 0;
        new.phrases[pid].p_node = 0;
        new.ap_expr_phrase.push(pid);
        Ok(new)
    }
}
