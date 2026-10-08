//! `fts5Int.h` e `fts5.h`: constantes, a configuração (`Fts5Config`), e os traits públicos da API
//! de extensão (funções auxiliares) e de tokenizador do FTS5.
//!
//! O que sobrou do cabeçalho do C e onde foi parar:
//!
//! * `Fts5Buffer`, `Fts5PoslistReader`/`Writer`, `Fts5Termset`: `buffer.rs`.
//! * `Fts5Hash`: `hash.rs`. `Fts5Index`/`Fts5IndexIter`/`Fts5Structure`: `index.rs`.
//! * `Fts5Expr*`, `Fts5Parse`, `Fts5Token` (vira `&[u8]`): `expr.rs`/`parse.rs`.
//! * `Fts5Storage`: `storage.rs`. `Fts5Table`, `Fts5Global`, `Fts5Cursor`: `main.rs`.
//! * `sqlite3Fts5IsBareword`, `Dequote`, `Tokenize`, `ConfigXxx`: `buffer.rs` e `config.rs`.
//! * `FTS5_CORRUPT` sem `SQLITE_DEBUG` é a constante `SQLITE_CORRUPT_VTAB`; `assert_nc` é
//!   `debug_assert!` sem a exceção de `sqlite3_fts5_may_be_corrupt`.

use std::any::Any;
use std::rc::Rc;

use crate::consts::SQLITE_CORRUPT_VTAB;
use crate::mem::Mem;

// ---------------------------------------------------------------------------------------------
// Constantes de fts5Int.h
// ---------------------------------------------------------------------------------------------

/// Tokens maiores que isto (em bytes) são truncados. O limite duro é 65521; o fator limitante é
/// o deslocamento de 16 bits no começo de cada página folha.
pub const FTS5_MAX_TOKEN_SIZE: usize = 32768;
/// Máximo de índices de prefixo por tabela FTS5 (precisa ser menor que 32).
pub const FTS5_MAX_PREFIX_INDEXES: usize = 31;
/// Máximo de segmentos permitidos num índice.
pub const FTS5_MAX_SEGMENT: i32 = 2000;
/// Distância padrão do NEAR.
pub const FTS5_DEFAULT_NEARDIST: i32 = 10;
/// Função de rank padrão.
pub const FTS5_DEFAULT_RANK: &[u8] = b"bm25";
/// Nome da coluna oculta de rank.
pub const FTS5_RANK_NAME: &[u8] = b"rank";
/// Nome da coluna de rowid.
pub const FTS5_ROWID_NAME: &[u8] = b"rowid";
/// `FTS5_CORRUPT` (sem `SQLITE_DEBUG`).
pub const FTS5_CORRUPT: i32 = SQLITE_CORRUPT_VTAB;

/// Versão esperada do formato do arquivo (campo `version` da tabela `%_config`).
pub const FTS5_CURRENT_VERSION: i32 = 4;
/// Versão esperada se a opção `secure-delete` já foi ligada alguma vez na tabela.
pub const FTS5_CURRENT_VERSION_SECUREDELETE: i32 = 5;

/// `Fts5Config.eContent`: conteúdo normal (`%_content`).
pub const FTS5_CONTENT_NORMAL: i32 = 0;
/// `Fts5Config.eContent`: sem conteúdo (`content=''`).
pub const FTS5_CONTENT_NONE: i32 = 1;
/// `Fts5Config.eContent`: tabela externa (`content=tbl`).
pub const FTS5_CONTENT_EXTERNAL: i32 = 2;

/// `Fts5Config.eDetail`: `detail=full`.
pub const FTS5_DETAIL_FULL: i32 = 0;
/// `Fts5Config.eDetail`: `detail=none`.
pub const FTS5_DETAIL_NONE: i32 = 1;
/// `Fts5Config.eDetail`: `detail=columns`.
pub const FTS5_DETAIL_COLUMNS: i32 = 2;

/// `Fts5Config.ePattern`: o tokenizador não serve para LIKE/GLOB.
pub const FTS5_PATTERN_NONE: i32 = 0;
/// `Fts5Config.ePattern`: igual a `SQLITE_INDEX_CONSTRAINT_LIKE`.
pub const FTS5_PATTERN_LIKE: i32 = 65;
/// `Fts5Config.ePattern`: igual a `SQLITE_INDEX_CONSTRAINT_GLOB`.
pub const FTS5_PATTERN_GLOB: i32 = 66;

/// Flags de `Fts5Index::query` (`FTS5INDEX_QUERY_*`): consulta de prefixo.
pub const FTS5INDEX_QUERY_PREFIX: i32 = 0x0001;
/// Documentos em ordem decrescente de rowid.
pub const FTS5INDEX_QUERY_DESC: i32 = 0x0002;
/// Não usar o índice de prefixo.
pub const FTS5INDEX_QUERY_TEST_NOIDX: i32 = 0x0004;
/// Consulta de varredura (fts5vocab).
pub const FTS5INDEX_QUERY_SCAN: i32 = 0x0008;
/// Uso interno do `index.rs`: pular vazios.
pub const FTS5INDEX_QUERY_SKIPEMPTY: i32 = 0x0010;
/// Uso interno do `index.rs`: sem saída.
pub const FTS5INDEX_QUERY_NOOUTPUT: i32 = 0x0020;
/// Uso interno do `index.rs`: pular o hash.
pub const FTS5INDEX_QUERY_SKIPHASH: i32 = 0x0040;
/// Uso interno do `index.rs`: sem tokendata.
pub const FTS5INDEX_QUERY_NOTOKENDATA: i32 = 0x0080;
/// Uso interno do `index.rs`: varrer um só termo.
pub const FTS5INDEX_QUERY_SCANONETERM: i32 = 0x0100;

/// `FTS5_STMT_SCAN_ASC`: `SELECT rowid, * FROM ... ORDER BY 1 ASC`.
pub const FTS5_STMT_SCAN_ASC: i32 = 0;
/// `FTS5_STMT_SCAN_DESC`: `SELECT rowid, * FROM ... ORDER BY 1 DESC`.
pub const FTS5_STMT_SCAN_DESC: i32 = 1;
/// `FTS5_STMT_LOOKUP`: `SELECT rowid, * FROM ... WHERE rowid=?`.
pub const FTS5_STMT_LOOKUP: i32 = 2;

/// Flag de `xTokenize`: tokenização de consulta (MATCH).
pub const FTS5_TOKENIZE_QUERY: i32 = 0x0001;
/// Flag de `xTokenize`: o termo da consulta é seguido de `*` (prefixo).
pub const FTS5_TOKENIZE_PREFIX: i32 = 0x0002;
/// Flag de `xTokenize`: tokenização de documento (inserção ou remoção).
pub const FTS5_TOKENIZE_DOCUMENT: i32 = 0x0004;
/// Flag de `xTokenize`: pedido de uma função auxiliar (ou `xColumnSize` em `columnsize=0`).
pub const FTS5_TOKENIZE_AUX: i32 = 0x0008;
/// Flag de `tflags` do callback: mesma posição do token anterior (sinônimo).
pub const FTS5_TOKEN_COLOCATED: i32 = 0x0001;

// ---------------------------------------------------------------------------------------------
// Fts5Colset
// ---------------------------------------------------------------------------------------------

/// `Fts5Colset`: o conjunto de colunas em que um NEAR ou uma frase pode casar (`nCol` é
/// `ai_col.len()`). Usado pelo `expr.rs` e pelo `index.rs`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fts5Colset {
    /// As colunas que podem casar.
    pub ai_col: Vec<i32>,
}

// ---------------------------------------------------------------------------------------------
// Fts5Config
// ---------------------------------------------------------------------------------------------

/// `Fts5Config`: tudo o que se extrai do CREATE VIRTUAL TABLE e da tabela `%_config`.
///
/// Posse: a `Fts5Table` (em `main.rs`) é a dona da configuração. `Fts5Index` e `Fts5Storage` NÃO
/// guardam ponteiro para ela (o `pConfig` do C): toda função delas recebe `&Fts5Config` (ou
/// `&mut`) explicitamente. O `sqlite3 *db` também não é campo: as funções que o usam recebem
/// `&mut Connection`.
///
/// Sem `SQLITE_DEBUG` não existe `bPrefixIndex`.
#[derive(Default)]
pub struct Fts5Config {
    /// Banco que guarda o índice FTS (por exemplo `main`).
    pub z_db: Vec<u8>,
    /// Nome do índice FTS.
    pub z_name: Vec<u8>,
    /// Nomes das colunas (`nCol` é `az_col.len()`).
    pub az_col: Vec<Vec<u8>>,
    /// Verdadeiro (1) para colunas `UNINDEXED`; mesmo tamanho de `az_col`.
    pub ab_unindexed: Vec<u8>,
    /// Tamanhos em bytes dos índices de prefixo (`nPrefix` é `a_prefix.len()`).
    pub a_prefix: Vec<i32>,
    /// Um valor `FTS5_CONTENT_*`.
    pub e_content: i32,
    /// Opção `contentless_delete=` (padrão 0).
    pub b_contentless_delete: i32,
    /// Tabela de conteúdo (já com aspas: `"main"."tbl"`); `None` em `content=''` sem docsize.
    pub z_content: Option<Vec<u8>>,
    /// Valor de `content_rowid=`, ou `rowid`. Sem aspas.
    pub z_content_rowid: Option<Vec<u8>>,
    /// Opção `columnsize=` (padrão 1): a tabela `%_docsize` existe.
    pub b_columnsize: i32,
    /// Opção `tokendata=` (padrão 0).
    pub b_tokendata: i32,
    /// Um valor `FTS5_DETAIL_*`.
    pub e_detail: i32,
    /// Lista de expressões de seleção do conteúdo (`T.rowid, T.c0, ...`).
    pub z_content_exprlist: Vec<u8>,
    /// O tokenizador (o par `pTok`/`pTokApi` do C). Compartilhado e imutável: quem precisa
    /// tokenizar enquanto muta a configuração clona o `Rc` antes.
    pub p_tok: Option<Rc<dyn Fts5Tokenizer>>,
    /// Verdadeiro enquanto a tabela prepara um comando.
    pub b_lock: i32,
    /// Um valor `FTS5_PATTERN_*`.
    pub e_pattern: i32,

    /* Valores carregados da tabela %_config */
    /// Versão do formato do arquivo.
    pub i_version: i32,
    /// Incrementado quando `%_config` muda.
    pub i_cookie: i32,
    /// Tamanho aproximado de página usado em `%_data`.
    pub pgsz: i32,
    /// Ajuste `automerge`.
    pub n_automerge: i32,
    /// Máximo de segmentos permitidos por nível.
    pub n_crisis_merge: i32,
    /// Ajuste `usermerge`.
    pub n_usermerge: i32,
    /// Bytes de memória do hash em memória.
    pub n_hash_size: i32,
    /// Nome da função de rank.
    pub z_rank: Option<Vec<u8>>,
    /// Argumentos da função de rank.
    pub z_rank_args: Option<Vec<u8>>,
    /// Ajuste `secure-delete`.
    pub b_secure_delete: i32,
    /// Ajuste `deletemerge`.
    pub n_delete_merge: i32,

    /// O `char **pzErrmsg` do C (aponta para `sqlite3_vtab.base.zErrmsg`, quase sempre nulo).
    /// `errmsg_target` diz se o ponteiro está armado; `main.rs` arma antes da chamada e, depois,
    /// move `errmsg` para o `zErrmsg` da tabela virtual e desarma. O `index.rs` e `config.rs`
    /// gravam a mensagem só se `errmsg_target` é verdadeiro (e `errmsg` ainda `None`).
    pub errmsg_target: bool,
    /// A mensagem de erro pendente (veja `errmsg_target`).
    pub errmsg: Option<Vec<u8>>,
}

impl Fts5Config {
    /// `nCol` do C.
    #[inline]
    pub fn n_col(&self) -> i32 {
        self.az_col.len() as i32
    }

    /// `nPrefix` do C.
    #[inline]
    pub fn n_prefix(&self) -> i32 {
        self.a_prefix.len() as i32
    }
}

// ---------------------------------------------------------------------------------------------
// fts5.h: tokenizadores
// ---------------------------------------------------------------------------------------------

/// O `xToken` que um tokenizador chama para cada token: `(tflags, token, iStart, iEnd)`, com
/// `tflags` uma máscara de `FTS5_TOKEN_*`, e os deslocamentos de byte do token no texto de
/// entrada. O `pCtx` do C é o que o fecho captura. Devolve `SQLITE_OK` para continuar; qualquer
/// outro valor abandona a tokenização.
pub type Fts5TokenFn<'a> = &'a mut dyn FnMut(i32, &[u8], i32, i32) -> i32;

/// Uma instância de tokenizador (`Fts5Tokenizer` + `fts5_tokenizer.xTokenize/xDelete`). O
/// `xDelete` é o `Drop`.
pub trait Fts5Tokenizer {
    /// `xTokenize`: tokeniza `text` (`flags` é uma máscara de `FTS5_TOKENIZE_*`), chamando
    /// `x_token` por token na ordem do texto. Devolve `SQLITE_OK` ao esgotar a entrada (um
    /// `SQLITE_DONE` do callback também vira `SQLITE_OK`) ou o primeiro código de erro.
    fn tokenize(&self, flags: i32, text: &[u8], x_token: Fts5TokenFn<'_>) -> i32;

    /// O estilo de casamento de padrões (`sqlite3Fts5TokenizerPattern`): `FTS5_PATTERN_LIKE`,
    /// `FTS5_PATTERN_GLOB` ou `FTS5_PATTERN_NONE`. Só o tokenizador `trigram` sobrescreve.
    fn pattern(&self) -> i32 {
        FTS5_PATTERN_NONE
    }
}

/// O construtor de um tipo de tokenizador registrado (`fts5_tokenizer.xCreate` com o seu
/// `pUserData`: o estado do construtor vive nos campos de quem implementa; o `xDestroy` é o
/// `Drop`). `api` é o `fts5_api` do FTS5 (o `porter` o usa para achar o tokenizador base).
pub trait Fts5TokenizerFactory {
    /// `xCreate`: cria uma instância com os argumentos `args` (os textos que seguem o nome do
    /// tokenizador no CREATE VIRTUAL TABLE). Devolve o código de erro em falha.
    fn create(&self, api: &dyn Fts5Api, args: &[Vec<u8>]) -> Result<Rc<dyn Fts5Tokenizer>, i32>;
}

// ---------------------------------------------------------------------------------------------
// fts5.h: funções auxiliares
// ---------------------------------------------------------------------------------------------

/// `Fts5PhraseIter`: o iterador de `xPhraseFirst`/`xPhraseNext` (e das variantes de coluna). No
/// C guarda dois ponteiros para a poslist; aqui guarda uma cópia dela e dois deslocamentos.
#[derive(Debug, Clone, Default)]
pub struct Fts5PhraseIter {
    /// A poslist (ou lista de colunas) da frase na linha corrente.
    pub data: Vec<u8>,
    /// Deslocamento corrente em `data` (o `a` do C).
    pub a: usize,
    /// Fim da lista em `data` (o `b` do C).
    pub b: usize,
}

/// O callback de `x_tokenize` das funções auxiliares: como [`Fts5TokenFn`], mais o objeto da API
/// de extensão como primeiro argumento (no C o contexto do callback e o `pFts` coexistem; em Rust
/// o cursor não pode estar emprestado duas vezes, então o FTS5 o devolve ao callback).
pub type Fts5ApiTokenFn<'a> =
    &'a mut dyn FnMut(&mut dyn Fts5ExtensionApi, i32, &[u8], i32, i32) -> i32;

/// `Fts5ExtensionApi`: a API oferecida às funções auxiliares. Quem implementa é o cursor do
/// `main.rs` (o `Fts5Context*` do C é o próprio `self`). Códigos de retorno como no C
/// (`SQLITE_RANGE` para argumento fora de faixa). `iVersion` é sempre 3.
pub trait Fts5ExtensionApi {
    /// `xUserData`: o dado do usuário registrado com a função.
    fn x_user_data(&self) -> Option<Rc<dyn Any>>;
    /// `xColumnCount`.
    fn x_column_count(&mut self) -> i32;
    /// `xRowCount`: número de linhas da tabela.
    fn x_row_count(&mut self, pn_row: &mut i64) -> i32;
    /// `xColumnTotalSize`: total de tokens da coluna (`i_col < 0`: da tabela inteira).
    fn x_column_total_size(&mut self, i_col: i32, pn_token: &mut i64) -> i32;
    /// `xTokenize`: tokeniza `text` com o tokenizador da tabela (`FTS5_TOKENIZE_AUX`).
    fn x_tokenize(&mut self, text: &[u8], x_token: Fts5ApiTokenFn<'_>) -> i32;
    /// `xPhraseCount`.
    fn x_phrase_count(&mut self) -> i32;
    /// `xPhraseSize`: tokens da frase `i_phrase` (0 se fora de faixa).
    fn x_phrase_size(&mut self, i_phrase: i32) -> i32;
    /// `xInstCount`.
    fn x_inst_count(&mut self, pn_inst: &mut i32) -> i32;
    /// `xInst`: a ocorrência `i_idx`: frase, coluna e deslocamento.
    fn x_inst(&mut self, i_idx: i32, pi_phrase: &mut i32, pi_col: &mut i32, pi_off: &mut i32) -> i32;
    /// `xRowid`.
    fn x_rowid(&mut self) -> i64;
    /// `xColumnText`: o texto UTF-8 da coluna da linha corrente (`Ok(None)` se NULL ou sem
    /// conteúdo); `Err(SQLITE_RANGE)` se a coluna está fora de faixa.
    fn x_column_text(&mut self, i_col: i32) -> Result<Option<Vec<u8>>, i32>;
    /// `xColumnSize`: tokens da coluna da linha corrente (`i_col < 0`: da linha toda).
    fn x_column_size(&mut self, i_col: i32, pn_token: &mut i32) -> i32;
    /// `xQueryPhrase`: roda a frase `i_phrase` como consulta e chama `x_callback` por linha, com
    /// o objeto da API posicionado na linha. Devolve `SQLITE_OK` mesmo se o callback devolve
    /// `SQLITE_DONE`; outro valor do callback aborta e é devolvido.
    fn x_query_phrase(
        &mut self,
        i_phrase: i32,
        x_callback: &mut dyn FnMut(&mut dyn Fts5ExtensionApi) -> i32,
    ) -> i32;
    /// `xSetAuxdata`: guarda o dado auxiliar (substitui e descarta o anterior; o `xDelete` do C
    /// é o `Drop`).
    fn x_set_auxdata(&mut self, p_aux: Option<Rc<dyn Any>>) -> i32;
    /// `xGetAuxdata`: o dado auxiliar corrente; com `b_clear` ele sai do lugar (sem descarte,
    /// quem recebe passa a ser dono).
    fn x_get_auxdata(&mut self, b_clear: bool) -> Option<Rc<dyn Any>>;
    /// `xPhraseFirst`.
    fn x_phrase_first(
        &mut self,
        i_phrase: i32,
        p_iter: &mut Fts5PhraseIter,
        pi_col: &mut i32,
        pi_off: &mut i32,
    ) -> i32;
    /// `xPhraseNext`.
    fn x_phrase_next(&mut self, p_iter: &mut Fts5PhraseIter, pi_col: &mut i32, pi_off: &mut i32);
    /// `xPhraseFirstColumn`.
    fn x_phrase_first_column(
        &mut self,
        i_phrase: i32,
        p_iter: &mut Fts5PhraseIter,
        pi_col: &mut i32,
    ) -> i32;
    /// `xPhraseNextColumn`.
    fn x_phrase_next_column(&mut self, p_iter: &mut Fts5PhraseIter, pi_col: &mut i32);
    /// `xQueryToken`: o token `i_token` da frase `i_phrase` da consulta; `Err(SQLITE_RANGE)` com
    /// índice fora de faixa.
    fn x_query_token(&mut self, i_phrase: i32, i_token: i32) -> Result<Vec<u8>, i32>;
    /// `xInstToken`: o token `i_token` da ocorrência `i_idx` no documento; `Err(SQLITE_RANGE)`
    /// fora de faixa.
    fn x_inst_token(&mut self, i_idx: i32, i_token: i32) -> Result<Vec<u8>, i32>;
}

/// O que uma função auxiliar produz (os `sqlite3_result_*` do C). O `sqlite3_context` do C não
/// pode acompanhar a função: a API de extensão e o contexto precisariam da mesma conexão ao
/// mesmo tempo (dois empréstimos mutáveis). Quem chama a função aplica o resultado ao contexto.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Fts5AuxResult {
    /// `NULL` (nenhum `sqlite3_result_*` chamado).
    #[default]
    Null,
    /// `sqlite3_result_double`.
    Double(f64),
    /// `sqlite3_result_text` (UTF-8).
    Text(Vec<u8>),
    /// `sqlite3_result_error` com a mensagem.
    Error(Vec<u8>),
    /// `sqlite3_result_error_code`.
    ErrorCode(i32),
}

/// `fts5_extension_function`: uma função auxiliar. `api` é o `pApi`+`pFts` do C; `args` são os
/// argumentos depois do primeiro (`apVal`). Devolve o resultado ou erro. Os dados do usuário e o
/// `xDestroy` são os campos e o `Drop` de quem implementa.
pub trait Fts5ExtensionFunction {
    /// Executa a função.
    fn call(&self, api: &mut dyn Fts5ExtensionApi, args: &[Mem]) -> Fts5AuxResult;
}


// ---------------------------------------------------------------------------------------------
// fts5.h: registro (fts5_api) e o global
// ---------------------------------------------------------------------------------------------

/// `fts5_api`: a API de registro de extensões. Quem implementa é o `Fts5Global` do `main.rs`.
/// `iVersion` é sempre 2.
pub trait Fts5Api {
    /// `xCreateTokenizer`: registra um tipo de tokenizador com o nome `z_name`.
    fn create_tokenizer(&mut self, z_name: &[u8], factory: Rc<dyn Fts5TokenizerFactory>) -> i32;
    /// `xFindTokenizer`: acha um tipo registrado; `None` equivale a `SQLITE_ERROR`.
    fn find_tokenizer(&self, z_name: &[u8]) -> Option<Rc<dyn Fts5TokenizerFactory>>;
    /// `xCreateFunction`: registra uma função auxiliar com o nome `z_name`.
    fn create_function(&mut self, z_name: &[u8], x_function: Rc<dyn Fts5ExtensionFunction>) -> i32;
}

/// O que o `config.rs` precisa do `Fts5Global` (que é do `main.rs`): resolver e criar o
/// tokenizador (`sqlite3Fts5GetTokenizer`). O `Fts5Global` do `main.rs` implementa este trait.
pub trait Fts5GlobalApi: Fts5Api {
    /// `sqlite3Fts5GetTokenizer`: localiza o tokenizador `az_arg[0]` (o padrão `unicode61` se
    /// `az_arg` é vazio), cria uma instância com `az_arg[1..]` e grava `config.p_tok` e
    /// `config.e_pattern`. Em falha deixa `p_tok` vazio, devolve o código de erro e, se
    /// `pz_err` existe, grava a mensagem (`no such tokenizer: %s` ou
    /// `error in tokenizer constructor`).
    fn get_tokenizer(
        &self,
        az_arg: &[Vec<u8>],
        config: &mut Fts5Config,
        pz_err: Option<&mut Option<Vec<u8>>>,
    ) -> i32;
}
