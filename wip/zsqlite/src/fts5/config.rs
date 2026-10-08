//! `fts5_config.c`: a análise dos argumentos do CREATE VIRTUAL TABLE, a carga da tabela
//! `%_config` e utilitários de texto.
//!
//! Desvios do C, decorrentes do modelo v2:
//!
//! * As "strings C" de entrada são fatias; o fim da fatia faz o papel do NUL (`util::at`) e os
//!   ponteiros que o C anda sobre elas são índices (`Option<usize>` onde o C devolve NULL).
//! * `char **pzErr` é `&mut Option<Vec<u8>>`; os erros de alocação não existem.
//! * `sqlite3Fts5ConfigParse` devolve `Result<Fts5Config, i32>` (o `*ppOut = 0` do erro é o `Err`)
//!   e não recebe o `sqlite3 *db`, que não é campo da configuração: as funções que usam o banco
//!   ([`Fts5Config::load`], [`Fts5Config::declare_vtab`]) recebem `&mut Connection`.
//! * `sqlite3Fts5ConfigFree` é o `Drop` (o tokenizador cai junto).
//! * `fts5ConfigDefaultTokenizer` é só a chamada `get_tokenizer(&[])` e não existe como função.

use crate::connection::Connection;
use crate::consts::{SQLITE_ERROR, SQLITE_INTEGER, SQLITE_OK, SQLITE_ROW};
use crate::mem::{value_numeric_type, Mem};
use crate::prepare::prepare_v2;
use crate::printf::{mprintf, PrintfArg};
use crate::util::{at, str_icmp, stricmp, strnicmp};
use crate::vdbeapi::{column_text, column_value, finalize, step, value_int, value_text};
use crate::vtab::declare_vtab;

use super::buffer::{fts5_is_bareword, fts5_mprintf, Fts5Buffer};
use super::int::{
    Fts5Config, Fts5GlobalApi, Fts5TokenFn, FTS5_CONTENT_EXTERNAL, FTS5_CONTENT_NONE,
    FTS5_CONTENT_NORMAL, FTS5_CURRENT_VERSION, FTS5_CURRENT_VERSION_SECUREDELETE,
    FTS5_DETAIL_COLUMNS, FTS5_DETAIL_FULL, FTS5_DETAIL_NONE, FTS5_MAX_PREFIX_INDEXES,
    FTS5_MAX_SEGMENT, FTS5_RANK_NAME, FTS5_ROWID_NAME,
};

/// Tamanho de página padrão do `%_data`.
pub const FTS5_DEFAULT_PAGE_SIZE: i32 = 4050;
/// `automerge` padrão.
pub const FTS5_DEFAULT_AUTOMERGE: i32 = 4;
/// `usermerge` padrão.
pub const FTS5_DEFAULT_USERMERGE: i32 = 4;
/// `crisismerge` padrão.
pub const FTS5_DEFAULT_CRISISMERGE: i32 = 16;
/// Tamanho padrão do hash em memória.
pub const FTS5_DEFAULT_HASHSIZE: i32 = 1024 * 1024;
/// `deletemerge` padrão (10%).
pub const FTS5_DEFAULT_DELETE_AUTOMERGE: i32 = 10;
/// Tamanho máximo de página permitido.
pub const FTS5_MAX_PAGE_SIZE: i32 = 64 * 1024;

fn fts5_iswhitespace(x: u8) -> bool {
    x == b' '
}

fn fts5_isopenquote(x: u8) -> bool {
    x == b'"' || x == b'\'' || x == b'[' || x == b'`'
}

fn fts5_isdigit(a: u8) -> bool {
    a.is_ascii_digit()
}

/// `fts5ConfigSkipWhitespace`: o primeiro caractere a partir de `p` que não é espaço.
fn skip_whitespace(z: &[u8], p: Option<usize>) -> Option<usize> {
    let mut p = p?;
    while fts5_iswhitespace(at(z, p)) {
        p += 1;
    }
    Some(p)
}

/// `fts5ConfigSkipBareword`: o primeiro caractere a partir de `p_in` que não é de palavra nua;
/// `None` se nenhum caractere foi consumido.
fn skip_bareword(z: &[u8], p_in: usize) -> Option<usize> {
    let mut p = p_in;
    while fts5_is_bareword(at(z, p)) {
        p += 1;
    }
    if p == p_in {
        None
    } else {
        Some(p)
    }
}

/// `fts5ConfigSkipLiteral`: pula um literal SQL (NULL, blob, string ou número) que começa em
/// `p_in`. `None` se não é um literal válido.
fn skip_literal(z: &[u8], p_in: usize) -> Option<usize> {
    let mut p = p_in;
    match at(z, p) {
        b'n' | b'N' => {
            if strnicmp(Some(b"null"), Some(z.get(p..).unwrap_or(&[])), 4) == 0 {
                Some(p + 4)
            } else {
                None
            }
        }
        b'x' | b'X' => {
            p += 1;
            if at(z, p) == b'\'' {
                p += 1;
                while at(z, p).is_ascii_hexdigit() {
                    p += 1;
                }
                if at(z, p) == b'\'' && 0 == ((p - p_in) % 2) {
                    Some(p + 1)
                } else {
                    None
                }
            } else {
                None
            }
        }
        b'\'' => {
            p += 1;
            loop {
                if at(z, p) == b'\'' {
                    p += 1;
                    if at(z, p) != b'\'' {
                        break;
                    }
                }
                p += 1;
                if at(z, p) == 0 {
                    return None;
                }
            }
            Some(p)
        }
        _ => {
            /* talvez um número */
            if at(z, p) == b'+' || at(z, p) == b'-' {
                p += 1;
            }
            while fts5_isdigit(at(z, p)) {
                p += 1;
            }

            /* Neste ponto, se o literal era inteiro, a análise acabou. Se é ponto flutuante,
            ** pode continuar com um ponto decimal ou com o caractere 'E'. */
            if at(z, p) == b'.' && fts5_isdigit(at(z, p + 1)) {
                p += 2;
                while fts5_isdigit(at(z, p)) {
                    p += 1;
                }
            }
            if p == p_in {
                None
            } else {
                Some(p)
            }
        }
    }
}

/// `fts5Dequote`: `z[0]` é um caractere de abrir aspas. Procura o fecha-aspas, remove as aspas
/// no próprio buffer (que fica truncado no fim do texto sem aspas) e devolve o deslocamento do
/// caractere que segue o fecha-aspas (se não achou, o fim do texto).
fn fts5_dequote_in_place(z: &mut Vec<u8>) -> usize {
    let mut i_in = 1usize;
    let mut i_out = 0usize;
    let mut q = at(z, 0);

    /* q é o caractere de fechar aspas */
    debug_assert!(q == b'[' || q == b'\'' || q == b'"' || q == b'`');
    if q == b'[' {
        q = b']';
    }

    while at(z, i_in) != 0 {
        if at(z, i_in) == q {
            if at(z, i_in + 1) != q {
                /* O caractere i_in era o fecha-aspas. */
                i_in += 1;
                break;
            } else {
                /* i_in e i_in+1 formam uma aspa escapada: pula os dois e copia uma só. */
                i_in += 2;
                z[i_out] = q;
                i_out += 1;
            }
        } else {
            z[i_out] = z[i_in];
            i_out += 1;
            i_in += 1;
        }
    }
    z.truncate(i_out);
    i_in
}

/// `sqlite3Fts5Dequote`: converte, no próprio buffer, uma string SQL entre aspas em string
/// normal. Se a entrada não começa com aspas, nada acontece. `"abc"`, `'xyz'`, `[pqr]` e `` `mno` ``
/// viram `abc`, `xyz`, `pqr` e `mno`.
pub fn fts5_dequote(z: &mut Vec<u8>) {
    debug_assert!(!fts5_iswhitespace(at(z, 0)));
    if fts5_isopenquote(at(z, 0)) {
        fts5_dequote_in_place(z);
    }
}

/// `fts5ConfigSetEnum`: acha `z_enum` (que pode ser abreviação) em `a_enum`. Ambíguo ou ausente é
/// `SQLITE_ERROR`. `*pe_val` recebe o valor achado (ou -1).
fn set_enum(a_enum: &[(&[u8], i32)], z_enum: &[u8], pe_val: &mut i32) -> i32 {
    let n_enum = z_enum.len() as i32;
    let mut i_val: i32 = -1;
    for (z_name, e_val) in a_enum {
        if strnicmp(Some(*z_name), Some(z_enum), n_enum) == 0 {
            if i_val >= 0 {
                return SQLITE_ERROR;
            }
            i_val = *e_val;
        }
    }
    *pe_val = i_val;
    if i_val < 0 {
        SQLITE_ERROR
    } else {
        SQLITE_OK
    }
}

/// `fts5ConfigParseSpecial`: interpreta uma diretiva especial do CREATE VIRTUAL TABLE e atualiza
/// `config`. Em erro devolve o código e pode deixar a mensagem em `pz_err`.
fn parse_special(
    global: &dyn Fts5GlobalApi,
    config: &mut Fts5Config,
    z_cmd: &[u8],
    z_arg: &[u8],
    pz_err: &mut Option<Vec<u8>>,
) -> i32 {
    let mut rc = SQLITE_OK;
    let n_cmd = z_cmd.len() as i32;
    /* Os nomes das diretivas valem por abreviação: `strnicmp(nome, zCmd, strlen(zCmd))`. */
    let is_cmd = |name: &[u8]| strnicmp(Some(name), Some(z_cmd), n_cmd) == 0;

    if is_cmd(b"prefix") {
        let mut b_first = true;
        let mut p = 0usize;
        loop {
            let mut n_pre: i32 = 0;
            while at(z_arg, p) == b' ' {
                p += 1;
            }
            if !b_first && at(z_arg, p) == b',' {
                p += 1;
                while at(z_arg, p) == b' ' {
                    p += 1;
                }
            } else if at(z_arg, p) == 0 {
                break;
            }
            if !at(z_arg, p).is_ascii_digit() {
                *pz_err = mprintf(b"malformed prefix=... directive", &[]);
                rc = SQLITE_ERROR;
                break;
            }

            if config.a_prefix.len() == FTS5_MAX_PREFIX_INDEXES {
                *pz_err = mprintf(
                    b"too many prefix indexes (max %d)",
                    &[PrintfArg::Int(FTS5_MAX_PREFIX_INDEXES as i64)],
                );
                rc = SQLITE_ERROR;
                break;
            }

            while at(z_arg, p).is_ascii_digit() && n_pre < 1000 {
                n_pre = n_pre * 10 + (at(z_arg, p) - b'0') as i32;
                p += 1;
            }

            if n_pre <= 0 || n_pre >= 1000 {
                *pz_err = mprintf(b"prefix length out of range (max 999)", &[]);
                rc = SQLITE_ERROR;
                break;
            }

            config.a_prefix.push(n_pre);
            b_first = false;
        }
        debug_assert!(config.a_prefix.len() <= FTS5_MAX_PREFIX_INDEXES);
        return rc;
    }

    if is_cmd(b"tokenize") {
        if config.p_tok.is_some() {
            *pz_err = mprintf(b"multiple tokenize=... directives", &[]);
            rc = SQLITE_ERROR;
        } else {
            let mut az_arg: Vec<Vec<u8>> = Vec::new();
            let mut p: Option<usize> = Some(0);
            while let Some(pi) = p {
                if at(z_arg, pi) == 0 {
                    break;
                }
                let p2 = skip_whitespace(z_arg, Some(pi));
                let p2i = p2.unwrap_or(pi);
                p = if at(z_arg, p2i) == b'\'' {
                    skip_literal(z_arg, p2i)
                } else {
                    skip_bareword(z_arg, p2i)
                };
                if let Some(pe) = p {
                    let mut one = z_arg[p2i..pe].to_vec();
                    fts5_dequote(&mut one);
                    az_arg.push(one);
                    p = skip_whitespace(z_arg, Some(pe));
                }
            }
            if p.is_none() {
                *pz_err = mprintf(b"parse error in tokenize directive", &[]);
                rc = SQLITE_ERROR;
            } else {
                rc = global.get_tokenizer(&az_arg, config, Some(pz_err));
            }
        }
        return rc;
    }

    if is_cmd(b"content") {
        if config.e_content != FTS5_CONTENT_NORMAL {
            *pz_err = mprintf(b"multiple content=... directives", &[]);
            rc = SQLITE_ERROR;
        } else if at(z_arg, 0) != 0 {
            config.e_content = FTS5_CONTENT_EXTERNAL;
            config.z_content = fts5_mprintf(
                &mut rc,
                b"%Q.%Q",
                &[PrintfArg::Text(Some(config.z_db.clone())), PrintfArg::Text(Some(z_arg.to_vec()))],
            );
        } else {
            config.e_content = FTS5_CONTENT_NONE;
        }
        return rc;
    }

    if is_cmd(b"contentless_delete") {
        if (at(z_arg, 0) != b'0' && at(z_arg, 0) != b'1') || at(z_arg, 1) != 0 {
            *pz_err = mprintf(b"malformed contentless_delete=... directive", &[]);
            rc = SQLITE_ERROR;
        } else {
            config.b_contentless_delete = (at(z_arg, 0) == b'1') as i32;
        }
        return rc;
    }

    if is_cmd(b"content_rowid") {
        if config.z_content_rowid.is_some() {
            *pz_err = mprintf(b"multiple content_rowid=... directives", &[]);
            rc = SQLITE_ERROR;
        } else {
            config.z_content_rowid = Some(z_arg.to_vec());
        }
        return rc;
    }

    if is_cmd(b"columnsize") {
        if (at(z_arg, 0) != b'0' && at(z_arg, 0) != b'1') || at(z_arg, 1) != 0 {
            *pz_err = mprintf(b"malformed columnsize=... directive", &[]);
            rc = SQLITE_ERROR;
        } else {
            config.b_columnsize = (at(z_arg, 0) == b'1') as i32;
        }
        return rc;
    }

    if is_cmd(b"detail") {
        let a_detail: [(&[u8], i32); 3] = [
            (b"none", FTS5_DETAIL_NONE),
            (b"full", FTS5_DETAIL_FULL),
            (b"columns", FTS5_DETAIL_COLUMNS),
        ];
        rc = set_enum(&a_detail, z_arg, &mut config.e_detail);
        if rc != SQLITE_OK {
            *pz_err = mprintf(b"malformed detail=... directive", &[]);
        }
        return rc;
    }

    if is_cmd(b"tokendata") {
        if (at(z_arg, 0) != b'0' && at(z_arg, 0) != b'1') || at(z_arg, 1) != 0 {
            *pz_err = mprintf(b"malformed tokendata=... directive", &[]);
            rc = SQLITE_ERROR;
        } else {
            config.b_tokendata = (at(z_arg, 0) == b'1') as i32;
        }
        return rc;
    }

    *pz_err = mprintf(
        b"unrecognized option: \"%.*s\"",
        &[PrintfArg::Int(n_cmd as i64), PrintfArg::Text(Some(z_cmd.to_vec()))],
    );
    SQLITE_ERROR
}

/// `fts5ConfigGobbleWord`: engole a primeira palavra nua ou entre aspas de `z_in`. Devolve
/// `(posição logo depois da palavra, cópia sem aspas da palavra, se tinha aspas)`. A posição e a
/// cópia são `None` se a palavra não pôde ser engolida.
fn gobble_word(z_in: &[u8]) -> (Option<usize>, Option<Vec<u8>>, bool) {
    let mut z_out = z_in.to_vec();
    if fts5_isopenquote(at(&z_out, 0)) {
        let ii = fts5_dequote_in_place(&mut z_out);
        (Some(ii), Some(z_out), true)
    } else {
        match skip_bareword(z_in, 0) {
            Some(z_ret) => {
                z_out.truncate(z_ret);
                (Some(z_ret), Some(z_out), false)
            }
            None => (None, None, false),
        }
    }
}

/// `fts5ConfigParseColumn`: acrescenta a coluna `z_col` (com a opção `z_arg`, só `unindexed`).
fn parse_column(
    p: &mut Fts5Config,
    z_col: Vec<u8>,
    z_arg: Option<Vec<u8>>,
    pz_err: &mut Option<Vec<u8>>,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut unindexed = 0u8;
    if 0 == str_icmp(&z_col, FTS5_RANK_NAME) || 0 == str_icmp(&z_col, FTS5_ROWID_NAME) {
        *pz_err = mprintf(b"reserved fts5 column name: %s", &[PrintfArg::Text(Some(z_col.clone()))]);
        rc = SQLITE_ERROR;
    } else if let Some(z_arg) = z_arg {
        if 0 == str_icmp(&z_arg, b"unindexed") {
            unindexed = 1;
        } else {
            *pz_err = mprintf(b"unrecognized column option: %s", &[PrintfArg::Text(Some(z_arg))]);
            rc = SQLITE_ERROR;
        }
    }
    p.ab_unindexed.push(unindexed);
    p.az_col.push(z_col);
    rc
}

/// `fts5ConfigMakeExprlist`: monta `Fts5Config.zContentExprlist`.
fn make_exprlist(p: &mut Fts5Config) -> i32 {
    let mut rc = SQLITE_OK;
    let mut buf = Fts5Buffer::new();

    buf.append_printf(
        &mut rc,
        b"T.%Q",
        &[PrintfArg::Text(p.z_content_rowid.clone())],
    );
    if p.e_content != FTS5_CONTENT_NONE {
        for (i, col) in p.az_col.iter().enumerate() {
            if p.e_content == FTS5_CONTENT_EXTERNAL {
                buf.append_printf(&mut rc, b", T.%Q", &[PrintfArg::Text(Some(col.clone()))]);
            } else {
                buf.append_printf(&mut rc, b", T.c%d", &[PrintfArg::Int(i as i64)]);
            }
        }
    }

    p.z_content_exprlist = buf.p;
    rc
}

/// `sqlite3Fts5ConfigParse`: `az_arg` são os argumentos de xCreate/xConnect do módulo
/// (`azArg[1]` o banco, `azArg[2]` o nome da tabela, `azArg[3..]` as colunas e opções). Devolve a
/// configuração ou o código de erro (a mensagem, se houver, fica em `pz_err`).
pub fn fts5_config_parse(
    global: &dyn Fts5GlobalApi,
    az_arg: &[Vec<u8>],
    pz_err: &mut Option<Vec<u8>>,
) -> Result<Fts5Config, i32> {
    let mut rc = SQLITE_OK;
    let mut p_ret = Fts5Config::default();
    let n_arg = az_arg.len();

    p_ret.i_cookie = -1;
    p_ret.z_db = az_arg[1].clone();
    p_ret.z_name = az_arg[2].clone();
    p_ret.b_columnsize = 1;
    p_ret.e_detail = FTS5_DETAIL_FULL;
    if str_icmp(&p_ret.z_name, FTS5_RANK_NAME) == 0 {
        *pz_err = mprintf(
            b"reserved fts5 table name: %s",
            &[PrintfArg::Text(Some(p_ret.z_name.clone()))],
        );
        rc = SQLITE_ERROR;
    }

    let mut i = 3;
    while rc == SQLITE_OK && i < n_arg {
        let z_orig: &[u8] = &az_arg[i];
        let mut b_option = false;

        let (z, z_one, b_must_be_col) = gobble_word(z_orig);
        let mut z = skip_whitespace(z_orig, z);
        if let Some(zi) = z {
            if at(z_orig, zi) == b'=' {
                b_option = true;
                debug_assert!(z_one.is_some());
                z = Some(zi + 1);
                if b_must_be_col {
                    z = None;
                }
            }
        }
        z = skip_whitespace(z_orig, z);
        let mut z_two: Option<Vec<u8>> = None;
        if let Some(zi) = z {
            if at(z_orig, zi) != 0 {
                let (z2, two, _b_dummy) = gobble_word(&z_orig[zi..]);
                z = z2.map(|k| zi + k);
                z_two = two;
                if let Some(k) = z {
                    if at(z_orig, k) != 0 {
                        z = None;
                    }
                }
            }
        }
        if z.is_none() {
            *pz_err = mprintf(b"parse error in \"%s\"", &[PrintfArg::Text(Some(z_orig.to_vec()))]);
            rc = SQLITE_ERROR;
        } else if b_option {
            rc = parse_special(
                global,
                &mut p_ret,
                z_one.as_deref().unwrap_or(b""),
                z_two.as_deref().unwrap_or(b""),
                pz_err,
            );
        } else {
            rc = parse_column(&mut p_ret, z_one.unwrap_or_default(), z_two, pz_err);
        }
        i += 1;
    }

    /* Só se permite contentless_delete=1 se a tabela é de fato sem conteúdo. */
    if rc == SQLITE_OK && p_ret.b_contentless_delete != 0 && p_ret.e_content != FTS5_CONTENT_NONE {
        *pz_err = mprintf(b"contentless_delete=1 requires a contentless table", &[]);
        rc = SQLITE_ERROR;
    }

    /* Só se permite contentless_delete=1 se columnsize=0 não está presente. Essa restrição pode
    ** ser removida algum dia. */
    if rc == SQLITE_OK && p_ret.b_contentless_delete != 0 && p_ret.b_columnsize == 0 {
        *pz_err = mprintf(b"contentless_delete=1 is incompatible with columnsize=0", &[]);
        rc = SQLITE_ERROR;
    }

    /* Se uma opção tokenizer= foi lida com sucesso, o tokenizador já foi criado. Senão cria uma
    ** instância do padrão (unicode61) agora. */
    if rc == SQLITE_OK && p_ret.p_tok.is_none() {
        debug_assert!(p_ret.e_pattern == 0);
        rc = global.get_tokenizer(&[], &mut p_ret, None);
    }

    /* Se nenhuma opção zContent foi dada, preenche os valores padrão. */
    if rc == SQLITE_OK && p_ret.z_content.is_none() {
        debug_assert!(
            p_ret.e_content == FTS5_CONTENT_NORMAL || p_ret.e_content == FTS5_CONTENT_NONE
        );
        let z_tail: Option<&[u8]> = if p_ret.e_content == FTS5_CONTENT_NORMAL {
            Some(b"content")
        } else if p_ret.b_columnsize != 0 {
            Some(b"docsize")
        } else {
            None
        };
        if let Some(z_tail) = z_tail {
            p_ret.z_content = fts5_mprintf(
                &mut rc,
                b"%Q.'%q_%s'",
                &[
                    PrintfArg::Text(Some(p_ret.z_db.clone())),
                    PrintfArg::Text(Some(p_ret.z_name.clone())),
                    PrintfArg::Text(Some(z_tail.to_vec())),
                ],
            );
        }
    }

    if rc == SQLITE_OK && p_ret.z_content_rowid.is_none() {
        p_ret.z_content_rowid = Some(b"rowid".to_vec());
    }

    /* Monta o texto de zContentExprlist */
    if rc == SQLITE_OK {
        rc = make_exprlist(&mut p_ret);
    }

    if rc != SQLITE_OK {
        Err(rc)
    } else {
        Ok(p_ret)
    }
}

/// `fts5ConfigSkipArgs`: `z[p_in..]` deveria ser uma lista de literais SQL separados por vírgula
/// seguida de `)`. Devolve a posição do `)` ou `None` se há erro.
fn skip_args(z: &[u8], p_in: usize) -> Option<usize> {
    let mut p = Some(p_in);
    loop {
        p = skip_whitespace(z, p);
        p = skip_literal(z, p?);
        p = skip_whitespace(z, p);
        match p {
            None => break,
            Some(pi) if at(z, pi) == b')' => break,
            Some(pi) if at(z, pi) != b',' => {
                p = None;
                break;
            }
            Some(pi) => p = Some(pi + 1),
        }
    }
    p
}

/// `sqlite3Fts5ConfigParseRank`: `z_in` é uma especificação de função de rank: uma palavra nua
/// (o nome), `(`, zero ou mais literais SQL separados por vírgula e `)`. Devolve o nome e os
/// argumentos (ausentes se vazios) ou `SQLITE_ERROR`.
pub fn fts5_config_parse_rank(z_in: Option<&[u8]>) -> Result<(Vec<u8>, Option<Vec<u8>>), i32> {
    let z = z_in.ok_or(SQLITE_ERROR)?;
    let p = skip_whitespace(z, Some(0));
    let p_rank = p.unwrap_or(0);
    let p = skip_bareword(z, p_rank).ok_or(SQLITE_ERROR)?;
    let z_rank = z[p_rank..p].to_vec();

    let p = skip_whitespace(z, Some(p)).unwrap_or(p);
    if at(z, p) != b'(' {
        return Err(SQLITE_ERROR);
    }
    let p = p + 1;

    let p = skip_whitespace(z, Some(p)).unwrap_or(p);
    let p_args = p;
    let mut z_rank_args = None;
    if at(z, p) != b')' {
        let p = skip_args(z, p).ok_or(SQLITE_ERROR)?;
        z_rank_args = Some(z[p_args..p].to_vec());
    }
    Ok((z_rank, z_rank_args))
}

impl Fts5Config {
    /// `sqlite3Fts5ConfigDeclareVtab`: chama `sqlite3_declare_vtab()` conforme a configuração.
    pub fn declare_vtab(&self, db: &mut Connection) -> i32 {
        let mut z_sql = b"CREATE TABLE x(".to_vec();
        for (i, col) in self.az_col.iter().enumerate() {
            let z_sep: &[u8] = if i == 0 { b"" } else { b", " };
            if let Some(piece) = mprintf(
                b"%s%Q",
                &[PrintfArg::Text(Some(z_sep.to_vec())), PrintfArg::Text(Some(col.clone()))],
            ) {
                z_sql.extend_from_slice(&piece);
            }
        }
        if let Some(tail) = mprintf(
            b", %Q HIDDEN, %s HIDDEN)",
            &[
                PrintfArg::Text(Some(self.z_name.clone())),
                PrintfArg::Text(Some(FTS5_RANK_NAME.to_vec())),
            ],
        ) {
            z_sql.extend_from_slice(&tail);
        }
        declare_vtab(db, &z_sql)
    }

    /// `sqlite3Fts5Tokenize`: tokeniza `p_text` com o tokenizador da tabela, chamando `x_token`
    /// por token (os argumentos do callback são os de [`Fts5TokenFn`]; a posição do token na
    /// entrada fica por conta do chamador, que a conta no fecho). `None` (texto NULL) não faz
    /// nada. Quem precisa mutar a configuração dentro do callback clona `p_tok` antes e chama
    /// `tokenize` do clone.
    pub fn tokenize(&self, flags: i32, p_text: Option<&[u8]>, x_token: Fts5TokenFn<'_>) -> i32 {
        let Some(text) = p_text else {
            return SQLITE_OK;
        };
        debug_assert!(self.p_tok.is_some());
        match &self.p_tok {
            Some(tok) => tok.tokenize(flags, text, x_token),
            None => SQLITE_OK,
        }
    }

    /// `sqlite3Fts5ConfigSetValue`: grava um único atributo de configuração. `*pb_badkey` vira 1
    /// se a chave ou o valor são inválidos.
    pub fn set_value(&mut self, z_key: &[u8], p_val: &mut Mem, pb_badkey: &mut i32) -> i32 {
        let mut rc = SQLITE_OK;

        if 0 == str_icmp(z_key, b"pgsz") {
            let mut pgsz = 0;
            if SQLITE_INTEGER == value_numeric_type(p_val) {
                pgsz = value_int(p_val);
            }
            if !(32..=FTS5_MAX_PAGE_SIZE).contains(&pgsz) {
                *pb_badkey = 1;
            } else {
                self.pgsz = pgsz;
            }
        } else if 0 == str_icmp(z_key, b"hashsize") {
            let mut n_hash_size = -1;
            if SQLITE_INTEGER == value_numeric_type(p_val) {
                n_hash_size = value_int(p_val);
            }
            if n_hash_size <= 0 {
                *pb_badkey = 1;
            } else {
                self.n_hash_size = n_hash_size;
            }
        } else if 0 == str_icmp(z_key, b"automerge") {
            let mut n_automerge = -1;
            if SQLITE_INTEGER == value_numeric_type(p_val) {
                n_automerge = value_int(p_val);
            }
            if !(0..=64).contains(&n_automerge) {
                *pb_badkey = 1;
            } else {
                if n_automerge == 1 {
                    n_automerge = FTS5_DEFAULT_AUTOMERGE;
                }
                self.n_automerge = n_automerge;
            }
        } else if 0 == str_icmp(z_key, b"usermerge") {
            let mut n_usermerge = -1;
            if SQLITE_INTEGER == value_numeric_type(p_val) {
                n_usermerge = value_int(p_val);
            }
            if !(2..=16).contains(&n_usermerge) {
                *pb_badkey = 1;
            } else {
                self.n_usermerge = n_usermerge;
            }
        } else if 0 == str_icmp(z_key, b"crisismerge") {
            let mut n_crisis_merge = -1;
            if SQLITE_INTEGER == value_numeric_type(p_val) {
                n_crisis_merge = value_int(p_val);
            }
            if n_crisis_merge < 0 {
                *pb_badkey = 1;
            } else {
                if n_crisis_merge <= 1 {
                    n_crisis_merge = FTS5_DEFAULT_CRISISMERGE;
                }
                if n_crisis_merge >= FTS5_MAX_SEGMENT {
                    n_crisis_merge = FTS5_MAX_SEGMENT - 1;
                }
                self.n_crisis_merge = n_crisis_merge;
            }
        } else if 0 == str_icmp(z_key, b"deletemerge") {
            let mut n_val = -1;
            if SQLITE_INTEGER == value_numeric_type(p_val) {
                n_val = value_int(p_val);
            } else {
                *pb_badkey = 1;
            }
            if n_val < 0 {
                n_val = FTS5_DEFAULT_DELETE_AUTOMERGE;
            }
            if n_val > 100 {
                n_val = 0;
            }
            self.n_delete_merge = n_val;
        } else if 0 == str_icmp(z_key, b"rank") {
            let z_in = value_text(p_val).map(|s| s.to_vec());
            match fts5_config_parse_rank(z_in.as_deref()) {
                Ok((z_rank, z_rank_args)) => {
                    self.z_rank = Some(z_rank);
                    self.z_rank_args = z_rank_args;
                }
                Err(e) if e == SQLITE_ERROR => {
                    rc = SQLITE_OK;
                    *pb_badkey = 1;
                }
                Err(e) => rc = e,
            }
        } else if 0 == str_icmp(z_key, b"secure-delete") {
            let mut b_val = -1;
            if SQLITE_INTEGER == value_numeric_type(p_val) {
                b_val = value_int(p_val);
            }
            if b_val < 0 {
                *pb_badkey = 1;
            } else {
                self.b_secure_delete = (b_val != 0) as i32;
            }
        } else {
            *pb_badkey = 1;
        }
        rc
    }

    /// `sqlite3Fts5ConfigLoad`: carrega o conteúdo da tabela `%_config` para a memória.
    pub fn load(&mut self, db: &mut Connection, i_cookie: i32) -> i32 {
        let mut rc = SQLITE_OK;
        let mut i_version = 0;

        /* Valores padrão */
        self.pgsz = FTS5_DEFAULT_PAGE_SIZE;
        self.n_automerge = FTS5_DEFAULT_AUTOMERGE;
        self.n_usermerge = FTS5_DEFAULT_USERMERGE;
        self.n_crisis_merge = FTS5_DEFAULT_CRISISMERGE;
        self.n_hash_size = FTS5_DEFAULT_HASHSIZE;
        self.n_delete_merge = FTS5_DEFAULT_DELETE_AUTOMERGE;

        let z_sql = fts5_mprintf(
            &mut rc,
            b"SELECT k, v FROM %Q.'%q_config'",
            &[
                PrintfArg::Text(Some(self.z_db.clone())),
                PrintfArg::Text(Some(self.z_name.clone())),
            ],
        );
        let mut p = None;
        if let Some(z_sql) = z_sql {
            let (rc2, stmt, _tail) = prepare_v2(db, &z_sql, -1);
            rc = rc2;
            p = stmt;
        }
        debug_assert!(rc == SQLITE_OK || p.is_none());
        if rc == SQLITE_OK {
            if let Some(stmt) = p {
                while SQLITE_ROW == step(db, stmt) {
                    let z_k = column_text(db, stmt, 0).map(|s| s.to_vec());
                    let mut p_val = column_value(db, stmt, 1);
                    if 0 == stricmp(z_k.as_deref(), Some(b"version")) {
                        i_version = value_int(&p_val);
                    } else {
                        let mut b_dummy = 0;
                        self.set_value(z_k.as_deref().unwrap_or(b""), &mut p_val, &mut b_dummy);
                    }
                }
                rc = finalize(db, stmt);
            }
        }

        if rc == SQLITE_OK
            && i_version != FTS5_CURRENT_VERSION
            && i_version != FTS5_CURRENT_VERSION_SECUREDELETE
        {
            rc = SQLITE_ERROR;
            if self.errmsg_target {
                debug_assert!(self.errmsg.is_none());
                self.errmsg = mprintf(
                    b"invalid fts5 file format (found %d, expected %d or %d) - run 'rebuild'",
                    &[
                        PrintfArg::Int(i_version as i64),
                        PrintfArg::Int(FTS5_CURRENT_VERSION as i64),
                        PrintfArg::Int(FTS5_CURRENT_VERSION_SECUREDELETE as i64),
                    ],
                );
            }
        } else {
            self.i_version = i_version;
        }

        if rc == SQLITE_OK {
            self.i_cookie = i_cookie;
        }
        rc
    }
}
