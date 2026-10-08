//! `fts5_aux.c`: as funções auxiliares embutidas do FTS5: `snippet()`, `highlight()` e `bm25()`.
//!
//! Modelo v2: cada função é um tipo que implementa [`Fts5ExtensionFunction`]; o `pApi`+`pFts`
//! do C é o objeto `&mut dyn Fts5ExtensionApi` (o cursor). O `xTokenize` da API devolve esse mesmo
//! objeto ao callback (veja [`Fts5ApiTokenFn`]), o que permite ao callback do `highlight()`
//! consultar as ocorrências (`xInst`) durante a tokenização sem emprestar o cursor duas vezes.
//! O resultado (`sqlite3_result_*`) volta como [`Fts5AuxResult`], porque a API e o contexto SQL
//! precisariam da mesma conexão ao mesmo tempo; quem chama a função o aplica ao contexto.
//! O dado auxiliar do `bm25()` é um `Rc<Fts5Bm25Data>` imutável (o `aFreq` do C, que é só
//! rascunho recalculado a cada linha, é um vetor local).

use std::any::Any;
use std::rc::Rc;

use crate::consts::{SQLITE_NOMEM, SQLITE_OK, SQLITE_RANGE};
use crate::mem::Mem;
use crate::vdbeapi::text_of;
use crate::vdbeapi::{value_double, value_int};

use super::int::{
    Fts5Api, Fts5AuxResult, Fts5ExtensionApi, Fts5ExtensionFunction, FTS5_CORRUPT,
    FTS5_TOKEN_COLOCATED,
};

/// `CInstIter`: percorre todas as "instâncias de frase coalescidas" de uma coluna da linha
/// corrente. Se as instâncias de frase de uma coluna não se sobrepõem, o iterador só as percorre;
/// se se sobrepõem (compartilham um ou mais tokens), cada conjunto de instâncias sobrepostas vale
/// como um único casamento (veja a documentação do `highlight()`).
#[derive(Default)]
struct CInstIter {
    /// Coluna pesquisada.
    i_col: i32,
    /// Índice da próxima instância de frase.
    i_inst: i32,
    /// Total de instâncias de frase.
    n_inst: i32,
    /// Primeiro token da instância coalescida (saída).
    i_start: i32,
    /// Último token da instância coalescida (saída).
    i_end: i32,
}

impl CInstIter {
    /// `fts5CInstIterNext`: avança para a próxima instância coalescida.
    fn next(&mut self, api: &mut dyn Fts5ExtensionApi) -> i32 {
        let mut rc = SQLITE_OK;
        self.i_start = -1;
        self.i_end = -1;

        while rc == SQLITE_OK && self.i_inst < self.n_inst {
            let (mut ip, mut ic, mut io) = (0, 0, 0);
            rc = api.x_inst(self.i_inst, &mut ip, &mut ic, &mut io);
            if rc == SQLITE_OK {
                if ic == self.i_col {
                    let i_end = io - 1 + api.x_phrase_size(ip);
                    if self.i_start < 0 {
                        self.i_start = io;
                        self.i_end = i_end;
                    } else if io <= self.i_end {
                        if i_end > self.i_end {
                            self.i_end = i_end;
                        }
                    } else {
                        break;
                    }
                }
                self.i_inst += 1;
            }
        }

        rc
    }

    /// `fts5CInstIterInit`: reinicia o iterador sobre as instâncias coalescidas da coluna `i_col`.
    fn init(&mut self, api: &mut dyn Fts5ExtensionApi, i_col: i32) -> i32 {
        *self = CInstIter { i_col, ..CInstIter::default() };
        let mut n_inst = 0;
        let rc = api.x_inst_count(&mut n_inst);
        self.n_inst = n_inst;
        if rc == SQLITE_OK {
            self.next(api)
        } else {
            rc
        }
    }
}

// ---------------------------------------------------------------------------------------------
// highlight()
// ---------------------------------------------------------------------------------------------

/// `HighlightContext`.
struct HighlightContext<'a> {
    /* Parâmetros constantes do fts5HighlightCb() */
    /// Primeiro token a incluir.
    i_range_start: i32,
    /// Se não é negativo, o último token a incluir.
    i_range_end: i32,
    /// Abertura do destaque.
    z_open: Option<&'a [u8]>,
    /// Fechamento do destaque.
    z_close: Option<&'a [u8]>,
    /// Texto de entrada.
    z_in: &'a [u8],

    /* Variáveis alteradas pelo fts5HighlightCb() */
    /// Iterador de instâncias coalescidas.
    iter: CInstIter,
    /// Deslocamento (em tokens) corrente.
    i_pos: i32,
    /// Já se copiou até este deslocamento (em bytes) de `z_in`.
    i_off: i32,
    /// Verdadeiro se o destaque está aberto.
    b_open: bool,
    /// Valor de saída.
    z_out: Option<Vec<u8>>,
}

impl<'a> HighlightContext<'a> {
    fn new(z_in: &'a [u8]) -> HighlightContext<'a> {
        HighlightContext {
            i_range_start: 0,
            i_range_end: -1,
            z_open: None,
            z_close: None,
            z_in,
            iter: CInstIter::default(),
            i_pos: 0,
            i_off: 0,
            b_open: false,
            z_out: None,
        }
    }

    /// O `&p->zIn[p->iOff]` do C: o texto de entrada a partir de `i_off`.
    fn in_tail(&self) -> &'a [u8] {
        self.z_in.get(self.i_off as usize..).unwrap_or(&[])
    }

    /// `fts5HighlightAppend`: acrescenta `n` bytes de `z` à saída (`n` negativo: tudo até o
    /// primeiro NUL). Como o `%.*s` do C, para no primeiro NUL. Sem efeito se `*rc` é um erro.
    fn append(&mut self, rc: &mut i32, z: Option<&[u8]>, n: i32) {
        if *rc == SQLITE_OK {
            if let Some(z) = z {
                let lim = if n < 0 { z.len() } else { (n as usize).min(z.len()) };
                let end = z[..lim].iter().position(|&b| b == 0).unwrap_or(lim);
                self.z_out.get_or_insert_with(Vec::new).extend_from_slice(&z[..end]);
            }
        }
    }
}

/// `fts5HighlightCb`: o callback de tokenização do `highlight()` e do `snippet()`.
fn highlight_cb(
    api: &mut dyn Fts5ExtensionApi,
    p: &mut HighlightContext<'_>,
    tflags: i32,
    i_start_off: i32,
    i_end_off: i32,
) -> i32 {
    let mut rc = SQLITE_OK;

    if tflags & FTS5_TOKEN_COLOCATED != 0 {
        return SQLITE_OK;
    }
    let i_pos = p.i_pos;
    p.i_pos += 1;

    if p.i_range_end >= 0 {
        if i_pos < p.i_range_start || i_pos > p.i_range_end {
            return SQLITE_OK;
        }
        if p.i_range_start != 0 && i_pos == p.i_range_start {
            p.i_off = i_start_off;
        }
    }

    /* Se o parêntese está aberto, este token não faz parte da frase corrente e o deslocamento
    ** inicial dele passa do ponto já copiado para a saída, fecha o parêntese. */
    if p.b_open && (i_pos <= p.iter.i_start || p.iter.i_start < 0) && i_start_off > p.i_off {
        p.append(&mut rc, p.z_close, -1);
        p.b_open = false;
    }

    /* Se é o começo de uma frase nova e o destaque não está aberto: copia o texto da entrada até
    ** o começo da frase e abre o destaque. */
    if i_pos == p.iter.i_start && !p.b_open {
        p.append(&mut rc, Some(p.in_tail()), i_start_off - p.i_off);
        p.append(&mut rc, p.z_open, -1);
        p.i_off = i_start_off;
        p.b_open = true;
    }

    if i_pos == p.iter.i_end {
        if !p.b_open {
            debug_assert!(p.i_range_end >= 0);
            p.append(&mut rc, p.z_open, -1);
            p.b_open = true;
        }
        p.append(&mut rc, Some(p.in_tail()), i_end_off - p.i_off);
        p.i_off = i_end_off;
        if rc == SQLITE_OK {
            rc = p.iter.next(api);
        }
    }

    if i_pos == p.i_range_end {
        if p.b_open {
            if p.iter.i_start >= 0 && i_pos >= p.iter.i_start {
                p.append(&mut rc, Some(p.in_tail()), i_end_off - p.i_off);
                p.i_off = i_end_off;
            }
            p.append(&mut rc, p.z_close, -1);
            p.b_open = false;
        }
        p.append(&mut rc, Some(p.in_tail()), i_end_off - p.i_off);
        p.i_off = i_end_off;
    }

    rc
}

/// A função `highlight()` (`fts5HighlightFunction`).
struct HighlightFunction;

impl Fts5ExtensionFunction for HighlightFunction {
    fn call(&self, api: &mut dyn Fts5ExtensionApi, args: &[Mem]) -> Fts5AuxResult {
        if args.len() != 3 {
            return Fts5AuxResult::Error(b"wrong number of arguments to function highlight()".to_vec());
        }

        let i_col = value_int(&args[0]);
        let z_open = text_of(&args[1]);
        let z_close = text_of(&args[2]);

        let mut rc = SQLITE_OK;
        let mut result = Fts5AuxResult::Null;
        match api.x_column_text(i_col) {
            Err(e) if e == SQLITE_RANGE => {
                result = Fts5AuxResult::Text(Vec::new());
            }
            Err(e) => rc = e,
            Ok(None) => {}
            Ok(Some(z_in)) => {
                let mut hctx = HighlightContext::new(&z_in);
                hctx.z_open = z_open.as_deref();
                hctx.z_close = z_close.as_deref();
                rc = hctx.iter.init(api, i_col);
                if rc == SQLITE_OK {
                    rc = api.x_tokenize(&z_in, &mut |api2, tflags, _tok, i_start, i_end| {
                        highlight_cb(api2, &mut hctx, tflags, i_start, i_end)
                    });
                }
                if hctx.b_open {
                    hctx.append(&mut rc, hctx.z_close, -1);
                }
                hctx.append(&mut rc, Some(hctx.in_tail()), z_in.len() as i32 - hctx.i_off);

                if rc == SQLITE_OK {
                    if let Some(out) = hctx.z_out.as_deref() {
                        result = Fts5AuxResult::Text(out.to_vec());
                    }
                }
            }
        }
        if rc != SQLITE_OK {
            result = Fts5AuxResult::ErrorCode(rc);
        }
        result
    }
}


// ---------------------------------------------------------------------------------------------
// snippet()
// ---------------------------------------------------------------------------------------------

/// `Fts5SFinder`: usado para achar os começos de sentença.
#[derive(Default)]
struct Fts5SFinder {
    /// Posição (em tokens) corrente.
    i_pos: i32,
    /// O primeiro token de cada sentença.
    a_first: Vec<i32>,
}

/// `fts5SentenceFinderCb`: callback de tokenização do `snippet()` que identifica os tokens que
/// abrem uma sentença e os guarda em `a_first`.
fn sentence_finder_cb(p: &mut Fts5SFinder, z_doc: &[u8], tflags: i32, i_start_off: i32) -> i32 {
    let rc = SQLITE_OK;
    if (tflags & FTS5_TOKEN_COLOCATED) == 0 {
        if p.i_pos > 0 {
            let mut c = 0u8;
            let mut i = i_start_off - 1;
            while i >= 0 {
                c = z_doc[i as usize];
                if c != b' ' && c != b'\t' && c != b'\n' && c != b'\r' {
                    break;
                }
                i -= 1;
            }
            if i != i_start_off - 1 && (c == b'.' || c == b':') {
                p.a_first.push(p.i_pos);
            }
        } else {
            p.a_first.push(0);
        }
        p.i_pos += 1;
    }
    rc
}

/// `fts5SnippetScore`: pontua a janela de `n_token` tokens da coluna `i_col` que começa em
/// `i_pos`. Devolve a pontuação em `*pn_score` e, se `pi_pos` existe, o deslocamento ajustado.
#[allow(clippy::too_many_arguments)]
fn snippet_score(
    api: &mut dyn Fts5ExtensionApi,
    n_docsize: i32,
    a_seen: &mut [u8],
    i_col: i32,
    i_pos: i32,
    n_token: i32,
    pn_score: &mut i32,
    pi_pos: Option<&mut i32>,
) -> i32 {
    let (mut ip, mut ic, mut i_off) = (0, 0, 0);
    let mut i_first = -1;
    let mut n_inst = 0;
    let mut n_score = 0;
    let mut i_last = 0;
    let i_end = i_pos as i64 + n_token as i64;

    let mut rc = api.x_inst_count(&mut n_inst);
    let mut i = 0;
    while i < n_inst && rc == SQLITE_OK {
        rc = api.x_inst(i, &mut ip, &mut ic, &mut i_off);
        if rc == SQLITE_OK && ic == i_col && i_off >= i_pos && (i_off as i64) < i_end {
            n_score += if a_seen[ip as usize] != 0 { 1 } else { 1000 };
            a_seen[ip as usize] = 1;
            if i_first < 0 {
                i_first = i_off;
            }
            i_last = i_off + api.x_phrase_size(ip);
        }
        i += 1;
    }

    *pn_score = n_score;
    if let Some(pi_pos) = pi_pos {
        let mut i_adj: i64 =
            i_first.wrapping_sub(n_token.wrapping_sub(i_last.wrapping_sub(i_first)) / 2) as i64;
        if (i_adj + n_token as i64) > n_docsize as i64 {
            i_adj = n_docsize as i64 - n_token as i64;
        }
        if i_adj < 0 {
            i_adj = 0;
        }
        *pi_pos = i_adj as i32;
    }
    rc
}

/// `fts5ValueToText`: o valor como texto UTF-8; NULL vira o texto vazio.
fn value_to_text(p_val: &Mem) -> Vec<u8> {
    text_of(p_val).map(|z| z.into_owned()).unwrap_or_default()
}

/// A função `snippet()` (`fts5SnippetFunction`).
struct SnippetFunction;

impl Fts5ExtensionFunction for SnippetFunction {
    fn call(&self, api: &mut dyn Fts5ExtensionApi, args: &[Mem]) -> Fts5AuxResult {
        let mut rc = SQLITE_OK; /* Código de retorno */
        let mut n_inst = 0; /* Número de casamentos de instância nesta linha */
        let mut n_best_score = 0; /* Pontuação do melhor trecho */
        let mut i_best_start = 0; /* Primeiro token do melhor trecho */
        let mut n_col_size = 0; /* Tamanho total de i_best_col em tokens */

        if args.len() != 5 {
            return Fts5AuxResult::Error(b"wrong number of arguments to function snippet()".to_vec());
        }

        let n_col = api.x_column_count();
        let i_col = value_int(&args[0]); /* 1º argumento do snippet() */
        let z_open = value_to_text(&args[1]);
        let z_close = value_to_text(&args[2]);
        let z_ellips = value_to_text(&args[3]); /* 4º argumento do snippet() */
        let n_token = value_int(&args[4]); /* 5º argumento do snippet() */
        let mut i_best_col = if i_col >= 0 { i_col } else { 0 }; /* Coluna do melhor trecho */
        let n_phrase = api.x_phrase_count();
        /* O sqlite3_malloc(0) do C devolve NULL, que o snippet() trata como falta de memória. */
        let mut a_seen: Vec<u8> = vec![0; n_phrase.max(0) as usize];
        if n_phrase <= 0 {
            rc = SQLITE_NOMEM;
        }
        if rc == SQLITE_OK {
            rc = api.x_inst_count(&mut n_inst);
        }

        let mut s_finder = Fts5SFinder::default(); /* Acha os começos de sentença */
        for i in 0..n_col {
            if i_col < 0 || i_col == i {
                s_finder.i_pos = 0;
                s_finder.a_first.clear();
                let z_doc = match api.x_column_text(i) {
                    Ok(z) => z,
                    Err(e) => {
                        rc = e;
                        break;
                    }
                };
                if let Some(z_doc) = &z_doc {
                    rc = api.x_tokenize(z_doc, &mut |_api, tflags, _tok, i_start, _i_end| {
                        sentence_finder_cb(&mut s_finder, z_doc, tflags, i_start)
                    });
                    if rc != SQLITE_OK {
                        break;
                    }
                }
                let mut n_docsize = 0;
                rc = api.x_column_size(i, &mut n_docsize);
                if rc != SQLITE_OK {
                    break;
                }

                for ii in 0..n_inst {
                    if rc != SQLITE_OK {
                        break;
                    }
                    let (mut ip, mut ic, mut io) = (0, 0, 0);
                    let mut i_adj = 0;
                    let mut n_score = 0;

                    rc = api.x_inst(ii, &mut ip, &mut ic, &mut io);
                    if ic != i {
                        continue;
                    }
                    if io > n_docsize {
                        rc = FTS5_CORRUPT;
                    }
                    if rc != SQLITE_OK {
                        continue;
                    }
                    a_seen.iter_mut().for_each(|b| *b = 0);
                    rc = snippet_score(
                        api,
                        n_docsize,
                        &mut a_seen,
                        i,
                        io,
                        n_token,
                        &mut n_score,
                        Some(&mut i_adj),
                    );
                    if rc == SQLITE_OK && n_score > n_best_score {
                        n_best_score = n_score;
                        i_best_col = i;
                        i_best_start = i_adj;
                        n_col_size = n_docsize;
                    }

                    if rc == SQLITE_OK && !s_finder.a_first.is_empty() && n_docsize > n_token {
                        let n_first = s_finder.a_first.len();
                        let mut jj = 0;
                        while jj + 1 < n_first {
                            if s_finder.a_first[jj + 1] > io {
                                break;
                            }
                            jj += 1;
                        }
                        if s_finder.a_first[jj] < io {
                            a_seen.iter_mut().for_each(|b| *b = 0);
                            rc = snippet_score(
                                api,
                                n_docsize,
                                &mut a_seen,
                                i,
                                s_finder.a_first[jj],
                                n_token,
                                &mut n_score,
                                None,
                            );
                            n_score += if s_finder.a_first[jj] == 0 { 120 } else { 100 };
                            if rc == SQLITE_OK && n_score > n_best_score {
                                n_best_score = n_score;
                                i_best_col = i;
                                i_best_start = s_finder.a_first[jj];
                                n_col_size = n_docsize;
                            }
                        }
                    }
                }
            }
        }

        let mut z_in: Option<Vec<u8>> = None;
        if rc == SQLITE_OK {
            match api.x_column_text(i_best_col) {
                Ok(z) => z_in = z,
                Err(e) => rc = e,
            }
        }
        if rc == SQLITE_OK && n_col_size == 0 {
            rc = api.x_column_size(i_best_col, &mut n_col_size);
        }
        let mut z_out: Option<Vec<u8>> = None;
        if let Some(z_in) = &z_in {
            let mut hctx = HighlightContext::new(z_in);
            hctx.z_open = Some(z_open.as_slice());
            hctx.z_close = Some(z_close.as_slice());
            if rc == SQLITE_OK {
                rc = hctx.iter.init(api, i_best_col);
            }

            hctx.i_range_start = i_best_start;
            hctx.i_range_end = i_best_start + n_token - 1;

            if i_best_start > 0 {
                hctx.append(&mut rc, Some(&z_ellips), -1);
            }

            /* Avança o iterador para que aponte para a primeira instância de frase coalescida
            ** em i_best_start ou depois dele. */
            while hctx.iter.i_start >= 0 && hctx.iter.i_start < i_best_start && rc == SQLITE_OK {
                rc = hctx.iter.next(api);
            }

            if rc == SQLITE_OK {
                rc = api.x_tokenize(z_in, &mut |api2, tflags, _tok, i_start, i_end| {
                    highlight_cb(api2, &mut hctx, tflags, i_start, i_end)
                });
            }
            if hctx.b_open {
                hctx.append(&mut rc, hctx.z_close, -1);
            }
            if hctx.i_range_end >= (n_col_size - 1) {
                hctx.append(&mut rc, Some(hctx.in_tail()), z_in.len() as i32 - hctx.i_off);
            } else {
                hctx.append(&mut rc, Some(&z_ellips), -1);
            }
            z_out = hctx.z_out.take();
        }

        if rc == SQLITE_OK {
            match z_out {
                Some(z) => Fts5AuxResult::Text(z),
                None => Fts5AuxResult::Null,
            }
        } else {
            Fts5AuxResult::ErrorCode(rc)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// bm25()
// ---------------------------------------------------------------------------------------------

/// `Fts5Bm25Data`: alocado e preenchido na primeira chamada do `bm25()` para uma consulta.
struct Fts5Bm25Data {
    /// Número de frases da consulta.
    n_phrase: i32,
    /// Número médio de tokens por linha.
    avgdl: f64,
    /// IDF de cada frase.
    a_idf: Vec<f64>,
}

/// `fts5Bm25GetData`: o `Fts5Bm25Data` da consulta corrente; se ainda não existe, calcula-o e o
/// guarda como dado auxiliar.
fn bm25_get_data(api: &mut dyn Fts5ExtensionApi) -> Result<Rc<Fts5Bm25Data>, i32> {
    if let Some(aux) = api.x_get_auxdata(false) {
        if let Ok(p) = aux.downcast::<Fts5Bm25Data>() {
            return Ok(p);
        }
    }

    let mut n_row: i64 = 0; /* Número de linhas da tabela */
    let mut n_token: i64 = 0; /* Número de tokens da tabela */

    let n_phrase = api.x_phrase_count();
    let mut p = Fts5Bm25Data { n_phrase, avgdl: 0.0, a_idf: vec![0.0; n_phrase.max(0) as usize] };

    /* Calcula o tamanho médio de documento da tabela FTS5 */
    let mut rc = api.x_row_count(&mut n_row);
    debug_assert!(rc != SQLITE_OK || n_row > 0);
    if rc == SQLITE_OK {
        rc = api.x_column_total_size(-1, &mut n_token);
    }
    if rc == SQLITE_OK {
        p.avgdl = n_token as f64 / n_row as f64;
    }

    /* Calcula o IDF de cada frase da consulta */
    let mut i = 0;
    while rc == SQLITE_OK && i < n_phrase {
        let mut n_hit: i64 = 0;
        rc = api.x_query_phrase(i, &mut |_api| {
            n_hit += 1;
            SQLITE_OK
        });
        if rc == SQLITE_OK {
            /* O IDF (Inverse Document Frequency) da frase i, pela fórmula padrão do BM25 da
            ** wikipedia:
            **
            **   IDF = log( (N - nHit + 0.5) / (nHit + 0.5) )
            **
            ** em que "N" é o total de documentos e nHit é o número dos que contêm pelo menos
            ** uma instância da frase.
            **
            ** O problema é que se (N < 2*nHit) o IDF é negativo, o que é indesejável. Então o
            ** IDF mínimo permitido é 1e-6, aproximadamente o de um termo que aparece em pouco
            ** mais da metade de um conjunto de 5.000.000 de documentos. */
            let mut idf = (((n_row - n_hit) as f64 + 0.5) / (n_hit as f64 + 0.5)).ln();
            if idf <= 0.0 {
                idf = 1e-6;
            }
            p.a_idf[i as usize] = idf;
        }
        i += 1;
    }

    if rc != SQLITE_OK {
        return Err(rc);
    }
    let p = Rc::new(p);
    let aux: Rc<dyn Any> = p.clone();
    let rc = api.x_set_auxdata(Some(aux));
    if rc != SQLITE_OK {
        return Err(rc);
    }
    Ok(p)
}

/// A função `bm25()` (`fts5Bm25Function`).
struct Bm25Function;

impl Fts5ExtensionFunction for Bm25Function {
    fn call(&self, api: &mut dyn Fts5ExtensionApi, args: &[Mem]) -> Fts5AuxResult {
        let k1: f64 = 1.2; /* Constante "k1" da fórmula do BM25 */
        let b: f64 = 0.75; /* Constante "b" da fórmula do BM25 */
        let mut score: f64 = 0.0; /* Valor de retorno da função SQL */
        let mut n_inst = 0; /* Valor devolvido por xInstCount() */
        let mut d: f64 = 0.0; /* Total de tokens da linha */
        let mut a_freq: Vec<f64> = Vec::new(); /* Frequência de cada frase na linha corrente */

        /* Calcula a frequência de frase (símbolo "f(qi,D)" da documentação) de cada frase da
        ** consulta para a linha corrente. */
        let p_data = bm25_get_data(api);
        let mut rc = match &p_data {
            Ok(p_data) => {
                a_freq = vec![0.0; p_data.n_phrase.max(0) as usize];
                api.x_inst_count(&mut n_inst)
            }
            Err(e) => *e,
        };
        let mut i = 0;
        while rc == SQLITE_OK && i < n_inst {
            let (mut ip, mut ic, mut io) = (0, 0, 0);
            rc = api.x_inst(i, &mut ip, &mut ic, &mut io);
            if rc == SQLITE_OK {
                let w = if (args.len() as i32) > ic { value_double(&args[ic as usize]) } else { 1.0 };
                a_freq[ip as usize] += w;
            }
            i += 1;
        }

        /* Calcula o tamanho total da linha corrente em tokens. */
        if rc == SQLITE_OK {
            let mut n_tok = 0;
            rc = api.x_column_size(-1, &mut n_tok);
            d = n_tok as f64;
        }

        /* Determina e devolve o BM25 da linha corrente. Ou, se houve erro, lança a exceção. */
        match (rc, &p_data) {
            (SQLITE_OK, Ok(p_data)) => {
                for i in 0..p_data.n_phrase as usize {
                    score += p_data.a_idf[i]
                        * ((a_freq[i] * (k1 + 1.0)) / (a_freq[i] + k1 * (1.0 - b + b * d / p_data.avgdl)));
                }
                if score.is_nan() {
                    Fts5AuxResult::Null
                } else {
                    Fts5AuxResult::Double(-1.0 * score)
                }
            }
            _ => Fts5AuxResult::ErrorCode(rc),
        }
    }
}

/// `sqlite3Fts5AuxInit`: registra as funções auxiliares embutidas.
pub fn fts5_aux_init(api: &mut dyn Fts5Api) -> i32 {
    let a_builtin: [(&[u8], Rc<dyn Fts5ExtensionFunction>); 3] = [
        (b"snippet", Rc::new(SnippetFunction)),
        (b"highlight", Rc::new(HighlightFunction)),
        (b"bm25", Rc::new(Bm25Function)),
    ];
    let mut rc = SQLITE_OK;
    for (z_func, x_func) in a_builtin {
        if rc != SQLITE_OK {
            break;
        }
        rc = api.create_function(z_func, x_func);
    }
    rc
}
