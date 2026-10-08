//! `json.c` (parte 1): JSONB, o analisador de JSON e JSON5 em texto, a tradução de JSONB para
//! texto, a busca por caminho com edição no lugar, e a ponte entre valores SQL e JSONB. As
//! funções SQL, os agregados, as tabelas virtuais `json_each` e `json_tree` e o registro estão em
//! [`crate::json2`], reexportado aqui.
//!
//! Desvios do C, todos decorrentes do modelo v2:
//!
//! * `sqlite3_value **argv` com `argc` vira `&[Mem]`. O texto de um argumento é emprestado
//!   quando já é UTF-8 e convertido numa cópia quando não é ([`text_of`]); a cadeia C vira fatia
//!   e o fim da fatia faz o papel do NUL (`at()` lê zero depois do fim).
//! * `JsonString` não guarda o `sqlite3_context`: as rotinas que precisam dele recebem o
//!   `Context` como parâmetro (o `pCtx` do C é sempre o contexto da chamada em curso). O buffer
//!   estático de 100 bytes e o `RCStr` somem: o buffer é um `Vec<u8>`.
//! * `JsonParse` guarda o JSONB num `Rc<Vec<u8>>` e o texto num `Rc<Vec<u8>>`. Uma entrada do
//!   cache é uma cópia barata do `JsonParse` (os dois `Rc` são compartilhados); qualquer edição
//!   passa por `Rc::make_mut`, que copia o JSONB quando ele ainda é compartilhado, o que é
//!   exatamente o "tornar editável" (`jsonBlobMakeEditable`) do C. Por isso somem `nJPRef`,
//!   `nBlobAlloc` (resta a marca `b_blob_owned`, o `nBlobAlloc>0`), `bReadOnly` (só servia a
//!   asserts e à escolha entre `SQLITE_DYNAMIC` e `SQLITE_TRANSIENT`) e `nBlob` (é o
//!   comprimento do `Vec`). A falta de memória (`oom`) nunca acontece com `Vec`.
//! * O cache de análises só é alimentado por uma cadeia de saída que passou do buffer estático
//!   do C (100 bytes); o cache é transparente (o resultado de uma busca é o mesmo com ou sem
//!   ele), então o limite é só uma aproximação do `bStatic`.
//! * Sem `SQLITE_DEBUG` (`json_parse`, `jsonDebugPrintBlob` e `jsonShowParse` não existem) e sem
//!   `SQLITE_LEGACY_JSON_VALID`.

use std::any::Any;
use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use crate::connection::{Context, UserData};
use crate::consts::{
    MEM_BLOB, MEM_NULL, MEM_STR, MEM_ZERO, SQLITE_BLOB, SQLITE_FLOAT, SQLITE_INTEGER, SQLITE_NULL,
    SQLITE_TEXT, SQLITE_UTF8,
};
use crate::ctype::{is_alnum, is_digit, is_xdigit, CTYPE_MAP};
use crate::mem::{value_type, Mem, StrDtor, ENC_UTF8, USE_LONG_DOUBLE};
use crate::printf::{mprintf, snprintf, PrintfArg};
use crate::utf::utf8_read_limited;
use crate::util::{at, atof, dec_or_hex_to_i64, hex_to_int, is_nan, strlen30, strnicmp};
use crate::vdbeapi::text_of;
use crate::vdbeapi::{
    get_auxdata, result_blob, result_error, result_error_nomem, result_int, result_int64,
    result_null, result_subtype, result_text, result_text64, set_auxdata, value_blob,
    value_double, value_subtype, value_text,
};

pub use crate::json2::*;

// ---------------------------------------------------------------------------------------------
// Constantes
// ---------------------------------------------------------------------------------------------

/// Tipos de elemento do JSONB.
pub(crate) const JSONB_NULL: u8 = 0;
pub(crate) const JSONB_TRUE: u8 = 1;
pub(crate) const JSONB_FALSE: u8 = 2;
pub(crate) const JSONB_INT: u8 = 3;
pub(crate) const JSONB_INT5: u8 = 4;
pub(crate) const JSONB_FLOAT: u8 = 5;
pub(crate) const JSONB_FLOAT5: u8 = 6;
pub(crate) const JSONB_TEXT: u8 = 7;
pub(crate) const JSONB_TEXTJ: u8 = 8;
pub(crate) const JSONB_TEXT5: u8 = 9;
pub(crate) const JSONB_TEXTRAW: u8 = 10;
pub(crate) const JSONB_ARRAY: u8 = 11;
pub(crate) const JSONB_OBJECT: u8 = 12;

/// `jsonbType[]`: os nomes legíveis dos tipos do JSONB, indexados pelo tipo.
pub(crate) const JSONB_TYPE: [&str; 17] = [
    "null", "true", "false", "integer", "integer", "real", "real", "text", "text", "text", "text",
    "array", "object", "", "", "", "",
];

/// O identificador do cache de análises em `sqlite3_get_auxdata()`.
pub(crate) const JSON_CACHE_ID: i32 = -429938;
/// Número máximo de entradas do cache.
pub(crate) const JSON_CACHE_SIZE: usize = 4;

/// `jsonUnescapeOneChar()` devolve este ponto de código inválido num erro de sintaxe.
pub(crate) const JSON_INVALID_CHAR: u32 = 0x99999;

/// Valores de `JsonString.eErr`.
pub(crate) const JSTRING_OOM: u8 = 0x01;
pub(crate) const JSTRING_MALFORMED: u8 = 0x02;
pub(crate) const JSTRING_ERR: u8 = 0x04;

/// O subtipo "J" dos valores de texto JSON.
pub(crate) const JSON_SUBTYPE: u32 = 74;

/// Bits de `sqlite3_user_data()` das funções.
pub(crate) const JSON_JSON: i32 = 0x01;
pub(crate) const JSON_SQL: i32 = 0x02;
pub(crate) const JSON_ABPATH: i32 = 0x03;
pub(crate) const JSON_ISSET: i32 = 0x04;
pub(crate) const JSON_BLOB: i32 = 0x08;

/// Valores de `JsonParse.eEdit`.
pub(crate) const JEDIT_DEL: u8 = 1;
pub(crate) const JEDIT_REPL: u8 = 2;
pub(crate) const JEDIT_INS: u8 = 3;
pub(crate) const JEDIT_SET: u8 = 4;

/// Profundidade máxima de aninhamento.
pub(crate) const JSON_MAX_DEPTH: u32 = 1000;

/// Valores do argumento `flgs` de [`json_parse_func_arg`].
pub(crate) const JSON_EDITABLE: u32 = 0x01;
pub(crate) const JSON_KEEPERROR: u32 = 0x02;

/// Erros de [`json_lookup_step`].
pub(crate) const JSON_LOOKUP_ERROR: u32 = 0xffffffff;
pub(crate) const JSON_LOOKUP_NOTFOUND: u32 = 0xfffffffe;
pub(crate) const JSON_LOOKUP_PATHERROR: u32 = 0xfffffffd;

/// `JSON_LOOKUP_ISERROR(x)`.
#[inline]
pub(crate) fn json_lookup_is_error(x: u32) -> bool {
    x >= JSON_LOOKUP_PATHERROR
}

/// O tamanho do buffer estático do `JsonString` do C (aproxima o `bStatic`).
const JSON_STRING_SPACE: usize = 100;

/// `jsonIsSpace[]`: os quatro espaços do JSON canônico.
#[inline]
pub(crate) fn json_is_space(c: u8) -> bool {
    matches!(c, 0x09 | 0x0a | 0x0d | 0x20)
}

/// `strspn(z, jsonSpaces)`.
fn json_strspn_spaces(z: &[u8]) -> u32 {
    let mut n = 0usize;
    while json_is_space(at(z, n)) {
        n += 1;
    }
    n as u32
}

/// `jsonIsOk[]`: verdadeiro para os bytes que passam sem escape numa cadeia JSON (tudo menos os
/// controles, `"`, `\` e `'`).
#[inline]
pub(crate) fn json_is_ok(c: u8) -> bool {
    c >= 0x20 && c != b'"' && c != b'\\' && c != b'\''
}

/// `sqlite3JsonId1(x)`: pode começar um identificador do JSON5.
#[inline]
fn json_id1(c: u8) -> bool {
    CTYPE_MAP[c as usize] & 0x42 != 0
}

/// `sqlite3JsonId2(x)`: pode continuar um identificador do JSON5.
#[inline]
fn json_id2(c: u8) -> bool {
    CTYPE_MAP[c as usize] & 0x46 != 0
}

// ---------------------------------------------------------------------------------------------
// Auxiliares do modelo v2
// ---------------------------------------------------------------------------------------------


/// `sqlite3_value_blob` de um valor BLOB (um blob vazio vira a fatia vazia).
pub(crate) fn blob_of(p: &Mem) -> Cow<'_, [u8]> {
    if p.flags & MEM_BLOB != 0 && p.flags & MEM_ZERO == 0 {
        return Cow::Borrowed(p.bytes());
    }
    let mut c = p.clone();
    Cow::Owned(value_blob(&mut c).map(<[u8]>::to_vec).unwrap_or_default())
}

/// O texto até o primeiro NUL (a cadeia C).
pub(crate) fn cstr(z: &[u8]) -> &[u8] {
    let n = z.iter().position(|&c| c == 0).unwrap_or(z.len());
    &z[..n]
}

/// `z + n` do C: a fatia a partir de `n`, vazia se `n` passa do fim.
pub(crate) fn off(z: &[u8], n: usize) -> &[u8] {
    &z[n.min(z.len())..]
}

/// `&b[start]` com `len` bytes, cortado no fim do buffer.
pub(crate) fn sub(b: &[u8], start: u32, len: u32) -> &[u8] {
    let s = (start as usize).min(b.len());
    let e = (start as usize + len as usize).min(b.len());
    &b[s..e]
}

/// `SQLITE_PTR_TO_INT(sqlite3_user_data(ctx))`: os bits de opção da função.
pub(crate) fn user_flags(ctx: &Context<'_>) -> i32 {
    match &ctx.arg_func.p_user_data {
        UserData::Int(v) => *v as i32,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------------------------
// JsonString: o acumulador de texto
// ---------------------------------------------------------------------------------------------

/// `struct JsonString`: uma cadeia em construção (e um acumulador genérico).
pub(crate) struct JsonString {
    /// O conteúdo (`zBuf`, `nUsed`).
    pub buf: Vec<u8>,
    /// Combinação de `JSTRING_*`.
    pub e_err: u8,
}

impl JsonString {
    /// `jsonStringInit`.
    pub(crate) fn new() -> JsonString {
        JsonString { buf: Vec::new(), e_err: 0 }
    }

    /// `jsonStringReset`: devolve o acumulador ao estado inicial (o `eErr` fica).
    pub(crate) fn reset(&mut self) {
        self.buf.clear();
    }

    /// `jsonAppendRaw` e `jsonAppendRawNZ`.
    pub(crate) fn append_raw(&mut self, z: &[u8]) {
        self.buf.extend_from_slice(z);
    }

    /// `jsonAppendChar`.
    pub(crate) fn append_char(&mut self, c: u8) {
        self.buf.push(c);
    }

    /// `jsonStringTrimOneChar`: tira o último caractere.
    pub(crate) fn trim_one_char(&mut self) {
        if self.e_err == 0 {
            self.buf.pop();
        }
    }

    /// `jsonAppendSeparator`: uma vírgula, se o último caractere não é `[` nem `{`.
    pub(crate) fn append_separator(&mut self) {
        let Some(&c) = self.buf.last() else {
            return;
        };
        if c == b'[' || c == b'{' {
            return;
        }
        self.append_char(b',');
    }

    /// `jsonAppendControlChar`: a representação canônica de um caractere de controle.
    pub(crate) fn append_control_char(&mut self, c: u8) {
        const SPECIAL: [u8; 32] = [
            0, 0, 0, 0, 0, 0, 0, 0, b'b', b't', b'n', 0, b'f', b'r', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let s = SPECIAL[(c & 0x1f) as usize];
        if s != 0 {
            self.buf.push(b'\\');
            self.buf.push(s);
        } else {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            self.buf.extend_from_slice(b"\\u00");
            self.buf.push(HEX[(c >> 4) as usize & 0xf]);
            self.buf.push(HEX[(c & 0xf) as usize]);
        }
    }

    /// `jsonAppendString`: a cadeia entre aspas, com `"` e `\` escapados e os controles na forma
    /// canônica.
    pub(crate) fn append_string(&mut self, z: &[u8]) {
        self.buf.push(b'"');
        for &c in z {
            if json_is_ok(c) {
                self.buf.push(c);
            } else if c == b'"' || c == b'\\' {
                self.buf.push(b'\\');
                self.buf.push(c);
            } else if c == b'\'' {
                self.buf.push(c);
            } else {
                self.append_control_char(c);
            }
        }
        self.buf.push(b'"');
    }
}

/// `jsonAppendSqlValue`: acrescenta um valor SQL (um argumento) ao JSON em construção.
pub(crate) fn json_append_sql_value(ctx: &mut Context<'_>, p: &mut JsonString, v: &Mem) {
    match value_type(v) {
        SQLITE_NULL => p.append_raw(b"null"),
        SQLITE_FLOAT => {
            let s = snprintf(100, b"%!0.15g", &[PrintfArg::Double(value_double(v))]);
            p.append_raw(&s);
        }
        SQLITE_INTEGER => {
            if let Some(z) = text_of(v) {
                p.append_raw(&z);
            }
        }
        SQLITE_TEXT => {
            if let Some(z) = text_of(v) {
                if value_subtype(v) == JSON_SUBTYPE {
                    p.append_raw(&z);
                } else {
                    p.append_string(&z);
                }
            }
        }
        _ => {
            if json_func_arg_might_be_binary(v) {
                let px = JsonParse::from_blob(blob_of(v).into_owned());
                json_translate_blob_to_text(&px, 0, p);
            } else if p.e_err == 0 {
                result_error(ctx, b"JSON cannot hold BLOB values", -1);
                p.e_err = JSTRING_ERR;
                p.reset();
            }
        }
    }
}

/// `jsonReturnString`: faz do texto de `p` o resultado da função SQL e zera `p`. Se `parse` é
/// dado, o texto vira o `zJson` dele e a análise entra no cache.
pub(crate) fn json_return_string(
    ctx: &mut Context<'_>,
    p: &mut JsonString,
    parse: Option<&mut JsonParse>,
) {
    if p.e_err == 0 {
        let flags = user_flags(ctx);
        if flags & JSON_BLOB != 0 {
            json_return_string_as_blob(ctx, p);
        } else {
            if p.buf.len() >= JSON_STRING_SPACE {
                if let Some(pp) = parse {
                    if !pp.b_json_is_rc_str && pp.b_blob_owned {
                        pp.z_json = Some(Rc::new(p.buf.clone()));
                        pp.b_json_is_rc_str = true;
                        json_cache_insert(ctx, pp);
                    }
                }
            }
            result_text64(
                ctx,
                Some(&p.buf),
                p.buf.len() as u64,
                StrDtor::Transient,
                SQLITE_UTF8 as u8,
            );
        }
    } else if p.e_err & JSTRING_OOM != 0 {
        result_error_nomem(ctx);
    } else if p.e_err & JSTRING_MALFORMED != 0 {
        result_error(ctx, b"malformed JSON", -1);
    }
    p.reset();
}

/// `jsonReturnStringAsBlob`: o texto JSON bem formado de `p` vira JSONB e é o resultado.
pub(crate) fn json_return_string_as_blob(ctx: &mut Context<'_>, p: &mut JsonString) {
    if p.e_err != 0 {
        result_error_nomem(ctx);
        return;
    }
    let mut px = JsonParse::default();
    px.z_json = Some(Rc::new(p.buf.clone()));
    let _ = json_translate_text_to_blob(&mut px, 0);
    if px.oom {
        result_error_nomem(ctx);
    } else {
        result_blob(ctx, Some(px.blob()), px.n_blob() as i32, StrDtor::Transient);
    }
}

// ---------------------------------------------------------------------------------------------
// JsonParse e o cache de análises
// ---------------------------------------------------------------------------------------------

/// `struct JsonParse`: um valor JSON analisado (ver o cabeçalho do módulo para os desvios).
#[derive(Clone, Default)]
pub(crate) struct JsonParse {
    /// A representação em JSONB (`aBlob`, `nBlob`).
    pub a_blob: Rc<Vec<u8>>,
    /// O texto de que o JSONB veio (`zJson`, `nJson`); `None` é o ponteiro nulo.
    pub z_json: Option<Rc<Vec<u8>>>,
    /// Posição do erro em `z_json`.
    pub i_err: u32,
    /// Profundidade de aninhamento.
    pub i_depth: u16,
    /// Número de erros vistos.
    pub n_err: u8,
    /// Falta de memória.
    pub oom: bool,
    /// `zJson` é um `RCStr`.
    pub b_json_is_rc_str: bool,
    /// A entrada usa recursos fora do padrão, como JSON5.
    pub has_nonstd: bool,
    /// O JSONB é dono do próprio buffer (`nBlobAlloc>0`).
    pub b_blob_owned: bool,
    /// A operação de edição a aplicar.
    pub e_edit: u8,
    /// A mudança de tamanho causada pela edição.
    pub delta: i32,
    /// O conteúdo a inserir (`aIns`, `nIns`).
    pub a_ins: Rc<Vec<u8>>,
    /// Posição do rótulo quando a busca parou no valor de um objeto.
    pub i_label: u32,
}

impl JsonParse {
    /// Uma análise cujo JSONB é `blob` (um JSONB de fora, não editável sem copiar).
    pub(crate) fn from_blob(blob: Vec<u8>) -> JsonParse {
        JsonParse { a_blob: Rc::new(blob), ..JsonParse::default() }
    }

    /// Os bytes do JSONB.
    #[inline]
    pub(crate) fn blob(&self) -> &[u8] {
        &self.a_blob
    }

    /// `nBlob`.
    #[inline]
    pub(crate) fn n_blob(&self) -> u32 {
        self.a_blob.len() as u32
    }

    /// `aBlob[i]`, ou zero além do fim.
    #[inline]
    pub(crate) fn blob_at(&self, i: u32) -> u8 {
        at(&self.a_blob, i as usize)
    }

    /// O JSONB para escrita: copia quando ainda é compartilhado (cache, valor SQL).
    pub(crate) fn blob_mut(&mut self) -> &mut Vec<u8> {
        self.b_blob_owned = true;
        Rc::make_mut(&mut self.a_blob)
    }
}

/// `jsonParseReset`: devolve a memória que a análise guarda.
pub(crate) fn json_parse_reset(p: &mut JsonParse) {
    if p.b_json_is_rc_str {
        p.z_json = None;
        p.b_json_is_rc_str = false;
    }
    if p.b_blob_owned {
        p.a_blob = Rc::new(Vec::new());
        p.b_blob_owned = false;
    }
}

/// `struct JsonCache`: as últimas análises de texto, as mais antigas primeiro.
#[derive(Default)]
pub(crate) struct JsonCache {
    /// `a[0..nUsed]`.
    a: Vec<JsonParse>,
}

/// O cache do comando (o auxdata de `JSON_CACHE_ID`), se já existe.
fn json_cache_get(ctx: &Context<'_>) -> Option<Rc<RefCell<JsonCache>>> {
    get_auxdata(ctx, JSON_CACHE_ID).and_then(|a| a.downcast::<RefCell<JsonCache>>().ok())
}

/// `jsonCacheInsert`: põe uma análise no cache, expulsando a mais antiga se ele está cheio.
pub(crate) fn json_cache_insert(ctx: &mut Context<'_>, p: &mut JsonParse) -> i32 {
    let cache = match json_cache_get(ctx) {
        Some(c) => c,
        None => {
            let c = Rc::new(RefCell::new(JsonCache::default()));
            let any: Rc<dyn Any> = c.clone();
            set_auxdata(ctx, JSON_CACHE_ID, Some(any), None);
            c
        }
    };
    let mut c = cache.borrow_mut();
    if c.a.len() >= JSON_CACHE_SIZE {
        c.a.remove(0);
    }
    p.e_edit = 0;
    c.a.push(p.clone());
    0
}

/// `jsonCacheSearch`: procura no cache a tradução do texto JSON de `arg`. A entrada achada vira
/// a mais recente.
fn json_cache_search(ctx: &Context<'_>, arg: &Mem) -> Option<JsonParse> {
    if value_type(arg) != SQLITE_TEXT {
        return None;
    }
    let z = text_of(arg)?;
    let cache = json_cache_get(ctx)?;
    let mut c = cache.borrow_mut();
    let idx = c
        .a
        .iter()
        .position(|e| e.z_json.as_ref().is_some_and(|j| j.as_slice() == &z[..]))?;
    let e = c.a.remove(idx);
    c.a.push(e.clone());
    Some(e)
}

// ---------------------------------------------------------------------------------------------
// Utilitários do analisador de texto
// ---------------------------------------------------------------------------------------------

/// `jsonHexToInt4`: quatro dígitos hexadecimais.
fn json_hex_to_int4(z: &[u8]) -> u32 {
    ((hex_to_int(at(z, 0) as i32) as u32) << 12)
        + ((hex_to_int(at(z, 1) as i32) as u32) << 8)
        + ((hex_to_int(at(z, 2) as i32) as u32) << 4)
        + hex_to_int(at(z, 3) as i32) as u32
}

/// `jsonIs2Hex`.
fn json_is_2hex(z: &[u8]) -> bool {
    is_xdigit(at(z, 0)) && is_xdigit(at(z, 1))
}

/// `jsonIs4Hex`.
pub(crate) fn json_is_4hex(z: &[u8]) -> bool {
    json_is_2hex(z) && json_is_2hex(off(z, 2))
}

/// `jsonIs4HexB`: se `z` é `u` seguido de quatro dígitos hexadecimais, `*op` vira `JSONB_TEXTJ`.
fn json_is_4hex_b(z: &[u8], op: &mut u8) -> bool {
    if at(z, 0) != b'u' {
        return false;
    }
    if !json_is_4hex(off(z, 1)) {
        return false;
    }
    *op = JSONB_TEXTJ;
    true
}

/// `json5Whitespace`: quantos bytes de espaço do JSON5 (e comentários) começam `z`.
pub(crate) fn json5_whitespace(z: &[u8]) -> usize {
    let mut n = 0usize;
    loop {
        match at(z, n) {
            0x09 | 0x0a | 0x0b | 0x0c | 0x0d | 0x20 => {
                n += 1;
            }
            b'/' => {
                if at(z, n + 1) == b'*' && at(z, n + 2) != 0 {
                    let mut j = n + 3;
                    while at(z, j) != b'/' || at(z, j - 1) != b'*' {
                        if at(z, j) == 0 {
                            return n;
                        }
                        j += 1;
                    }
                    n = j + 1;
                } else if at(z, n + 1) == b'/' {
                    let mut j = n + 2;
                    loop {
                        let c = at(z, j);
                        if c == 0 {
                            break;
                        }
                        if c == b'\n' || c == b'\r' {
                            break;
                        }
                        if 0xe2 == c && 0x80 == at(z, j + 1) && (0xa8 == at(z, j + 2) || 0xa9 == at(z, j + 2))
                        {
                            j += 2;
                            break;
                        }
                        j += 1;
                    }
                    n = j;
                    if at(z, n) != 0 {
                        n += 1;
                    }
                } else {
                    return n;
                }
            }
            0xc2 => {
                if at(z, n + 1) == 0xa0 {
                    n += 2;
                } else {
                    return n;
                }
            }
            0xe1 => {
                if at(z, n + 1) == 0x9a && at(z, n + 2) == 0x80 {
                    n += 3;
                } else {
                    return n;
                }
            }
            0xe2 => {
                if at(z, n + 1) == 0x80 {
                    let c = at(z, n + 2);
                    if c < 0x80 {
                        return n;
                    }
                    if c <= 0x8a || c == 0xa8 || c == 0xa9 || c == 0xaf {
                        n += 3;
                        continue;
                    }
                } else if at(z, n + 1) == 0x81 && at(z, n + 2) == 0x9f {
                    n += 3;
                    continue;
                }
                return n;
            }
            0xe3 => {
                if at(z, n + 1) == 0x80 && at(z, n + 2) == 0x80 {
                    n += 3;
                } else {
                    return n;
                }
            }
            0xef => {
                if at(z, n + 1) == 0xbb && at(z, n + 2) == 0xbf {
                    n += 3;
                } else {
                    return n;
                }
            }
            _ => return n,
        }
    }
}

/// `aNanInfName[]`: os literais de ponto flutuante extras: (c1, c2, n, vira float, texto).
const NAN_INF_NAME: [(u8, u8, usize, bool, &[u8]); 5] = [
    (b'i', b'I', 3, true, b"inf"),
    (b'i', b'I', 8, true, b"infinity"),
    (b'n', b'N', 3, false, b"NaN"),
    (b'q', b'Q', 4, false, b"QNaN"),
    (b's', b'S', 4, false, b"SNaN"),
];

// ---------------------------------------------------------------------------------------------
// Utilitários do JSONB
// ---------------------------------------------------------------------------------------------

/// `jsonBlobMakeEditable`: o JSONB passa a ser do `JsonParse` (a cópia, se preciso, é feita pelo
/// `Rc::make_mut` na primeira escrita).
pub(crate) fn json_blob_make_editable(p: &mut JsonParse, _n_extra: usize) -> bool {
    if p.oom {
        return false;
    }
    p.b_blob_owned = true;
    true
}

/// `jsonBlobAppendOneByte`.
pub(crate) fn json_blob_append_one_byte(p: &mut JsonParse, c: u8) {
    p.blob_mut().push(c);
}

/// `jsonBlobAppendNode`: acrescenta o byte de tipo com o tamanho do payload e, se `payload` é
/// dado, o payload. Sem payload só o cabeçalho é escrito (o tamanho se corrige depois).
pub(crate) fn json_blob_append_node(
    p: &mut JsonParse,
    e_type: u8,
    sz_payload: u32,
    payload: Option<&[u8]>,
) {
    let v = p.blob_mut();
    if sz_payload <= 11 {
        v.push(e_type | ((sz_payload as u8) << 4));
    } else if sz_payload <= 0xff {
        v.push(e_type | 0xc0);
        v.push((sz_payload & 0xff) as u8);
    } else if sz_payload <= 0xffff {
        v.push(e_type | 0xd0);
        v.push(((sz_payload >> 8) & 0xff) as u8);
        v.push((sz_payload & 0xff) as u8);
    } else {
        v.push(e_type | 0xe0);
        v.push(((sz_payload >> 24) & 0xff) as u8);
        v.push(((sz_payload >> 16) & 0xff) as u8);
        v.push(((sz_payload >> 8) & 0xff) as u8);
        v.push((sz_payload & 0xff) as u8);
    }
    if let Some(pl) = payload {
        v.extend_from_slice(sub(pl, 0, sz_payload));
    }
}

/// `jsonBlobChangePayloadSize`: muda o tamanho do payload do nó em `i`; devolve a variação do
/// tamanho do cabeçalho.
pub(crate) fn json_blob_change_payload_size(p: &mut JsonParse, i: u32, sz_payload: u32) -> i32 {
    if p.oom {
        return 0;
    }
    let sz_type = p.blob_at(i) >> 4;
    let n_extra: i32 = if sz_type <= 11 {
        0
    } else if sz_type == 12 {
        1
    } else if sz_type == 13 {
        2
    } else {
        4
    };
    let n_needed: i32 = if sz_payload <= 11 {
        0
    } else if sz_payload <= 0xff {
        1
    } else if sz_payload <= 0xffff {
        2
    } else {
        4
    };
    let delta = n_needed - n_extra;
    let iu = i as usize;
    let v = p.blob_mut();
    if delta > 0 {
        let at_i = (iu + 1).min(v.len());
        v.splice(at_i..at_i, std::iter::repeat(0u8).take(delta as usize));
    } else if delta < 0 {
        let s = (iu + 1).min(v.len());
        let e = (iu + 1 + (-delta) as usize).min(v.len());
        v.drain(s..e);
    }
    if iu >= v.len() {
        return delta;
    }
    let keep = v[iu] & 0x0f;
    if n_needed == 0 {
        v[iu] = keep | ((sz_payload as u8) << 4);
    } else if n_needed == 1 {
        v[iu] = keep | 0xc0;
        v[iu + 1] = (sz_payload & 0xff) as u8;
    } else if n_needed == 2 {
        v[iu] = keep | 0xd0;
        v[iu + 1] = ((sz_payload >> 8) & 0xff) as u8;
        v[iu + 2] = (sz_payload & 0xff) as u8;
    } else {
        v[iu] = keep | 0xe0;
        v[iu + 1] = ((sz_payload >> 24) & 0xff) as u8;
        v[iu + 2] = ((sz_payload >> 16) & 0xff) as u8;
        v[iu + 3] = ((sz_payload >> 8) & 0xff) as u8;
        v[iu + 4] = (sz_payload & 0xff) as u8;
    }
    delta
}

/// `jsonbPayloadSize` sobre um JSONB cru: o byte em `i` é um código de tipo; devolve
/// `(deslocamento até o payload, tamanho do payload)`, ou `(0, 0)` se há erro. `delta` é a
/// mudança de tamanho pendente de uma edição.
pub(crate) fn json_payload_size_raw(blob: &[u8], delta: i32, i: u32) -> (u32, u32) {
    let n_blob = blob.len() as u32;
    if i > n_blob {
        return (0, 0);
    }
    let b = |k: u32| -> u32 { at(blob, (i + k) as usize) as u32 };
    let x = b(0) >> 4;
    let sz: u32;
    let n: u32;
    if x <= 11 {
        sz = x;
        n = 1;
    } else if x == 12 {
        if i.wrapping_add(1) >= n_blob {
            return (0, 0);
        }
        sz = b(1);
        n = 2;
    } else if x == 13 {
        if i.wrapping_add(2) >= n_blob {
            return (0, 0);
        }
        sz = (b(1) << 8) + b(2);
        n = 3;
    } else if x == 14 {
        if i.wrapping_add(4) >= n_blob {
            return (0, 0);
        }
        sz = (b(1) << 24).wrapping_add(b(2) << 16).wrapping_add(b(3) << 8).wrapping_add(b(4));
        n = 5;
    } else {
        if i.wrapping_add(8) >= n_blob || b(1) != 0 || b(2) != 0 || b(3) != 0 || b(4) != 0 {
            return (0, 0);
        }
        sz = (b(5) << 24).wrapping_add(b(6) << 16).wrapping_add(b(7) << 8).wrapping_add(b(8));
        n = 9;
    }
    let end = i as i64 + sz as i64 + n as i64;
    if end > n_blob as i64 && end > n_blob.wrapping_sub(delta as u32) as i64 {
        return (0, 0);
    }
    (n, sz)
}

/// `jsonbPayloadSize` sobre uma análise.
#[inline]
pub(crate) fn json_payload_size(p: &JsonParse, i: u32) -> (u32, u32) {
    json_payload_size_raw(&p.a_blob, p.delta, i)
}

/// `jsonbValidityCheck`: confere um elemento do JSONB (de `i` até `i_end`). Devolve zero se está
/// certo, ou o deslocamento (a partir de 1) do erro.
pub(crate) fn json_validity_check(blob: &[u8], i: u32, i_end: u32, i_depth: u32) -> u32 {
    if i_depth > JSON_MAX_DEPTH {
        return i + 1;
    }
    let (n, sz) = json_payload_size_raw(blob, 0, i);
    if n == 0 {
        return i + 1;
    }
    if i + n + sz != i_end {
        return i + 1;
    }
    let zb = |k: u32| -> u8 { at(blob, k as usize) };
    let x = zb(i) & 0x0f;
    match x {
        JSONB_NULL | JSONB_TRUE | JSONB_FALSE => {
            if n + sz == 1 {
                0
            } else {
                i + 1
            }
        }
        JSONB_INT => {
            if sz < 1 {
                return i + 1;
            }
            let mut j = i + n;
            if zb(j) == b'-' {
                j += 1;
                if sz < 2 {
                    return i + 1;
                }
            }
            let k = i + n + sz;
            while j < k {
                if is_digit(zb(j)) {
                    j += 1;
                } else {
                    return j + 1;
                }
            }
            0
        }
        JSONB_INT5 => {
            if sz < 3 {
                return i + 1;
            }
            let mut j = i + n;
            if zb(j) == b'-' {
                if sz < 4 {
                    return i + 1;
                }
                j += 1;
            }
            if zb(j) != b'0' {
                return i + 1;
            }
            if zb(j + 1) != b'x' && zb(j + 1) != b'X' {
                return j + 2;
            }
            j += 2;
            let k = i + n + sz;
            while j < k {
                if is_xdigit(zb(j)) {
                    j += 1;
                } else {
                    return j + 1;
                }
            }
            0
        }
        JSONB_FLOAT | JSONB_FLOAT5 => {
            let mut seen: u8 = 0; // 0: inicial. 1: '.' visto. 2: 'e' visto
            if sz < 2 {
                return i + 1;
            }
            let mut j = i + n;
            let k = j + sz;
            if zb(j) == b'-' {
                j += 1;
                if sz < 3 {
                    return i + 1;
                }
            }
            if zb(j) == b'.' {
                if x == JSONB_FLOAT {
                    return j + 1;
                }
                if !is_digit(zb(j + 1)) {
                    return j + 1;
                }
                j += 2;
                seen = 1;
            } else if zb(j) == b'0' && x == JSONB_FLOAT {
                if j + 3 > k {
                    return j + 1;
                }
                if zb(j + 1) != b'.' && zb(j + 1) != b'e' && zb(j + 1) != b'E' {
                    return j + 1;
                }
                j += 1;
            }
            while j < k {
                let c = zb(j);
                if is_digit(c) {
                    j += 1;
                    continue;
                }
                if c == b'.' {
                    if seen > 0 {
                        return j + 1;
                    }
                    if x == JSONB_FLOAT && (j == k - 1 || !is_digit(zb(j + 1))) {
                        return j + 1;
                    }
                    seen = 1;
                    j += 1;
                    continue;
                }
                if c == b'e' || c == b'E' {
                    if seen == 2 {
                        return j + 1;
                    }
                    if j == k - 1 {
                        return j + 1;
                    }
                    if zb(j + 1) == b'+' || zb(j + 1) == b'-' {
                        j += 1;
                        if j == k - 1 {
                            return j + 1;
                        }
                    }
                    seen = 2;
                    j += 1;
                    continue;
                }
                return j + 1;
            }
            if seen == 0 {
                return i + 1;
            }
            0
        }
        JSONB_TEXT => {
            let mut j = i + n;
            let k = j + sz;
            while j < k {
                if !json_is_ok(zb(j)) && zb(j) != b'\'' {
                    return j + 1;
                }
                j += 1;
            }
            0
        }
        JSONB_TEXTJ | JSONB_TEXT5 => {
            let mut j = i + n;
            let k = j + sz;
            while j < k {
                let c = zb(j);
                if !json_is_ok(c) && c != b'\'' {
                    if c == b'"' {
                        if x == JSONB_TEXTJ {
                            return j + 1;
                        }
                    } else if c <= 0x1f {
                        // Caracteres de controle em literais JSON5 são aceitos.
                        if x == JSONB_TEXTJ {
                            return j + 1;
                        }
                    } else if c != b'\\' || j + 1 >= k {
                        return j + 1;
                    } else if matches!(zb(j + 1), b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' | 0)
                    {
                        // `strchr("\"\\/bfnrt", c)` também casa o NUL final.
                        j += 1;
                    } else if zb(j + 1) == b'u' {
                        if j + 5 >= k {
                            return j + 1;
                        }
                        if !json_is_4hex(sub(blob, j + 2, 4)) {
                            return j + 1;
                        }
                        j += 1;
                    } else if x != JSONB_TEXT5 {
                        return j + 1;
                    } else {
                        let (sz_c, c_val) = json_unescape_one_char(sub(blob, j, k - j));
                        if c_val == JSON_INVALID_CHAR {
                            return j + 1;
                        }
                        j += sz_c - 1;
                    }
                }
                j += 1;
            }
            0
        }
        JSONB_TEXTRAW => 0,
        JSONB_ARRAY => {
            let mut j = i + n;
            let k = j + sz;
            while j < k {
                let (n2, sz2) = json_payload_size_raw(blob, 0, j);
                if n2 == 0 {
                    return j + 1;
                }
                if j + n2 + sz2 > k {
                    return j + 1;
                }
                let sub_err = json_validity_check(blob, j, j + n2 + sz2, i_depth + 1);
                if sub_err != 0 {
                    return sub_err;
                }
                j += n2 + sz2;
            }
            0
        }
        JSONB_OBJECT => {
            let mut cnt = 0u32;
            let mut j = i + n;
            let k = j + sz;
            while j < k {
                let (n2, sz2) = json_payload_size_raw(blob, 0, j);
                if n2 == 0 {
                    return j + 1;
                }
                if j + n2 + sz2 > k {
                    return j + 1;
                }
                if (cnt & 1) == 0 {
                    let xl = zb(j) & 0x0f;
                    if xl < JSONB_TEXT || xl > JSONB_TEXTRAW {
                        return j + 1;
                    }
                }
                let sub_err = json_validity_check(blob, j, j + n2 + sz2, i_depth + 1);
                if sub_err != 0 {
                    return sub_err;
                }
                cnt += 1;
                j += n2 + sz2;
            }
            if (cnt & 1) != 0 {
                return j + 1;
            }
            0
        }
        _ => i + 1,
    }
}

// ---------------------------------------------------------------------------------------------
// O analisador de texto
// ---------------------------------------------------------------------------------------------

/// `jsonTranslateTextToBlob`: traduz um elemento do JSON de `p.z_json[i]` para JSONB e o
/// acrescenta ao fim de `p`. Devolve o índice do primeiro caractere depois do elemento, ou:
/// 0 no fim da entrada, -1 num erro de sintaxe, -2 ao ver `}`, -3 ao ver `]`, -4 ao ver `,`,
/// -5 ao ver `:` (nos quatro últimos `p.i_err` é o índice do caractere visto).
pub(crate) fn json_translate_text_to_blob(p: &mut JsonParse, i0: u32) -> i32 {
    let zj: Rc<Vec<u8>> = p.z_json.clone().unwrap_or_default();
    let z: &[u8] = &zj;
    let n_json = z.len() as u32;
    let zc = |k: u32| -> u8 { at(z, k as usize) };
    let mut i = i0;
    loop {
        // json_parse_restart
        match zc(i) {
            b'{' => {
                // Analisa um objeto.
                let i_this = p.n_blob();
                json_blob_append_node(p, JSONB_OBJECT, n_json - i, None);
                p.i_depth += 1;
                if p.i_depth as u32 > JSON_MAX_DEPTH {
                    p.i_err = i;
                    return -1;
                }
                let i_start = p.n_blob();
                let mut j = i + 1;
                loop {
                    let i_blob = p.n_blob();
                    let mut x = json_translate_text_to_blob(p, j);
                    if x <= 0 {
                        if x == -2 {
                            j = p.i_err;
                            if p.n_blob() != i_start {
                                p.has_nonstd = true;
                            }
                            break;
                        }
                        j += json5_whitespace(off(z, j as usize)) as u32;
                        let mut op = JSONB_TEXT;
                        if json_id1(zc(j))
                            || (zc(j) == b'\\' && json_is_4hex_b(off(z, j as usize + 1), &mut op))
                        {
                            let mut k = j + 1;
                            while (json_id2(zc(k)) && json5_whitespace(off(z, k as usize)) == 0)
                                || (zc(k) == b'\\' && json_is_4hex_b(off(z, k as usize + 1), &mut op))
                            {
                                k += 1;
                            }
                            json_blob_append_node(p, op, k - j, Some(sub(z, j, k - j)));
                            p.has_nonstd = true;
                            x = k as i32;
                        } else {
                            if x != -1 {
                                p.i_err = j;
                            }
                            return -1;
                        }
                    }
                    if p.oom {
                        return -1;
                    }
                    let t = p.blob_at(i_blob) & 0x0f;
                    if t < JSONB_TEXT || t > JSONB_TEXTRAW {
                        p.i_err = j;
                        return -1;
                    }
                    j = x as u32;
                    let mut object_value = false;
                    if zc(j) == b':' {
                        j += 1;
                    } else {
                        if json_is_space(zc(j)) {
                            // strspn() não ajuda aqui.
                            loop {
                                j += 1;
                                if !json_is_space(zc(j)) {
                                    break;
                                }
                            }
                            if zc(j) == b':' {
                                j += 1;
                                object_value = true;
                            }
                        }
                        if !object_value {
                            x = json_translate_text_to_blob(p, j);
                            if x != -5 {
                                if x != -1 {
                                    p.i_err = j;
                                }
                                return -1;
                            }
                            j = p.i_err + 1;
                        }
                    }
                    // parse_object_value:
                    x = json_translate_text_to_blob(p, j);
                    if x <= 0 {
                        if x != -1 {
                            p.i_err = j;
                        }
                        return -1;
                    }
                    j = x as u32;
                    if zc(j) == b',' {
                        j += 1;
                        continue;
                    } else if zc(j) == b'}' {
                        break;
                    } else {
                        if json_is_space(zc(j)) {
                            j += 1 + json_strspn_spaces(off(z, j as usize + 1));
                            if zc(j) == b',' {
                                j += 1;
                                continue;
                            } else if zc(j) == b'}' {
                                break;
                            }
                        }
                        x = json_translate_text_to_blob(p, j);
                        if x == -4 {
                            j = p.i_err + 1;
                            continue;
                        }
                        if x == -2 {
                            j = p.i_err;
                            break;
                        }
                    }
                    p.i_err = j;
                    return -1;
                }
                let n_new = p.n_blob() - i_start;
                json_blob_change_payload_size(p, i_this, n_new);
                p.i_depth -= 1;
                return (j + 1) as i32;
            }
            b'[' => {
                // Analisa um vetor.
                let i_this = p.n_blob();
                json_blob_append_node(p, JSONB_ARRAY, n_json - i, None);
                let i_start = p.n_blob();
                if p.oom {
                    return -1;
                }
                p.i_depth += 1;
                if p.i_depth as u32 > JSON_MAX_DEPTH {
                    p.i_err = i;
                    return -1;
                }
                let mut j = i + 1;
                loop {
                    let mut x = json_translate_text_to_blob(p, j);
                    if x <= 0 {
                        if x == -3 {
                            j = p.i_err;
                            if p.n_blob() != i_start {
                                p.has_nonstd = true;
                            }
                            break;
                        }
                        if x != -1 {
                            p.i_err = j;
                        }
                        return -1;
                    }
                    j = x as u32;
                    if zc(j) == b',' {
                        j += 1;
                        continue;
                    } else if zc(j) == b']' {
                        break;
                    } else {
                        if json_is_space(zc(j)) {
                            j += 1 + json_strspn_spaces(off(z, j as usize + 1));
                            if zc(j) == b',' {
                                j += 1;
                                continue;
                            } else if zc(j) == b']' {
                                break;
                            }
                        }
                        x = json_translate_text_to_blob(p, j);
                        if x == -4 {
                            j = p.i_err + 1;
                            continue;
                        }
                        if x == -3 {
                            j = p.i_err;
                            break;
                        }
                    }
                    p.i_err = j;
                    return -1;
                }
                let n_new = p.n_blob() - i_start;
                json_blob_change_payload_size(p, i_this, n_new);
                p.i_depth -= 1;
                return (j + 1) as i32;
            }
            b'\'' | b'"' => {
                // Analisa uma cadeia.
                let mut opcode = JSONB_TEXT;
                if zc(i) == b'\'' {
                    p.has_nonstd = true;
                }
                let c_delim = zc(i);
                let mut j = i + 1;
                loop {
                    if json_is_ok(zc(j)) {
                        if !json_is_ok(zc(j + 1)) {
                            j += 1;
                        } else if !json_is_ok(zc(j + 2)) {
                            j += 2;
                        } else {
                            j += 3;
                            continue;
                        }
                    }
                    let mut c = zc(j);
                    if c == c_delim {
                        break;
                    } else if c == b'\\' {
                        j += 1;
                        c = zc(j);
                        if matches!(c, b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't')
                            || (c == b'u' && json_is_4hex(off(z, j as usize + 1)))
                        {
                            if opcode == JSONB_TEXT {
                                opcode = JSONB_TEXTJ;
                            }
                        } else if c == b'\''
                            || c == b'0'
                            || c == b'v'
                            || c == b'\n'
                            || (0xe2 == c
                                && 0x80 == zc(j + 1)
                                && (0xa8 == zc(j + 2) || 0xa9 == zc(j + 2)))
                            || (c == b'x' && json_is_2hex(off(z, j as usize + 1)))
                        {
                            opcode = JSONB_TEXT5;
                            p.has_nonstd = true;
                        } else if c == b'\r' {
                            if zc(j + 1) == b'\n' {
                                j += 1;
                            }
                            opcode = JSONB_TEXT5;
                            p.has_nonstd = true;
                        } else {
                            p.i_err = j;
                            return -1;
                        }
                    } else if c <= 0x1f {
                        if c == 0 {
                            p.i_err = j;
                            return -1;
                        }
                        // Caracteres de controle não são aceitos em literais do JSON canônico,
                        // mas são aceitos nos do JSON5.
                        opcode = JSONB_TEXT5;
                        p.has_nonstd = true;
                    } else if c == b'"' {
                        opcode = JSONB_TEXT5;
                    }
                    j += 1;
                }
                json_blob_append_node(p, opcode, j - 1 - i, Some(sub(z, i + 1, j - 1 - i)));
                return (j + 1) as i32;
            }
            b't' => {
                if z[(i as usize).min(z.len())..].starts_with(b"true") && !is_alnum(zc(i + 4)) {
                    json_blob_append_one_byte(p, JSONB_TRUE);
                    return (i + 4) as i32;
                }
                p.i_err = i;
                return -1;
            }
            b'f' => {
                if z[(i as usize).min(z.len())..].starts_with(b"false") && !is_alnum(zc(i + 5)) {
                    json_blob_append_one_byte(p, JSONB_FALSE);
                    return (i + 5) as i32;
                }
                p.i_err = i;
                return -1;
            }
            b'+' | b'.' | b'-' | b'0'..=b'9' => {
                return json_translate_number(p, z, i);
            }
            b'}' => {
                p.i_err = i;
                return -2; // Fim de {...}
            }
            b']' => {
                p.i_err = i;
                return -3; // Fim de [...]
            }
            b',' => {
                p.i_err = i;
                return -4; // Separador de lista
            }
            b':' => {
                p.i_err = i;
                return -5; // Separador de rótulo e valor
            }
            0 => return 0, // Fim da entrada
            0x09 | 0x0a | 0x0d | 0x20 => {
                i += 1 + json_strspn_spaces(off(z, i as usize + 1));
                continue;
            }
            0x0b | 0x0c | b'/' | 0xc2 | 0xe1 | 0xe2 | 0xe3 | 0xef => {
                let j = json5_whitespace(off(z, i as usize)) as u32;
                if j > 0 {
                    i += j;
                    p.has_nonstd = true;
                    continue;
                }
                p.i_err = i;
                return -1;
            }
            b'n' => {
                if z[(i as usize).min(z.len())..].starts_with(b"null") && !is_alnum(zc(i + 4)) {
                    json_blob_append_one_byte(p, JSONB_NULL);
                    return (i + 4) as i32;
                }
                // Cai no caso padrão, que procura NaN.
                return json_translate_nan_inf(p, z, i);
            }
            _ => return json_translate_nan_inf(p, z, i),
        }
    }
}

/// O caso padrão de `jsonTranslateTextToBlob`: `Infinity`, `NaN` e afins.
fn json_translate_nan_inf(p: &mut JsonParse, z: &[u8], i: u32) -> i32 {
    let c = at(z, i as usize);
    for (c1, c2, nn, is_float, z_match) in NAN_INF_NAME.iter() {
        if c != *c1 && c != *c2 {
            continue;
        }
        if strnicmp(Some(off(z, i as usize)), Some(z_match), *nn as i32) != 0 {
            continue;
        }
        if is_alnum(at(z, i as usize + nn)) {
            continue;
        }
        if *is_float {
            json_blob_append_node(p, JSONB_FLOAT, 5, Some(b"9e999"));
        } else {
            json_blob_append_one_byte(p, JSONB_NULL);
        }
        p.has_nonstd = true;
        return (i as usize + nn) as i32;
    }
    p.i_err = i;
    -1 // Erro de sintaxe
}

/// Os casos de número de `jsonTranslateTextToBlob` (`+`, `.`, `-` e dígitos).
fn json_translate_number(p: &mut JsonParse, z: &[u8], i0: u32) -> i32 {
    let zc = |k: u32| -> u8 { at(z, k as usize) };
    let mut i = i0;
    let mut t: u8 = 0; // Bit 0x01: JSON5. Bit 0x02: FLOAT
    let mut seen_e = false;
    let mut j: u32 = 0;
    let mut finish = false;
    let mut number_2 = false;
    let c0 = zc(i);
    if c0 == b'+' {
        p.has_nonstd = true;
    } else if c0 == b'.' {
        if is_digit(zc(i + 1)) {
            p.has_nonstd = true;
            t = 0x03;
            number_2 = true;
        } else {
            p.i_err = i;
            return -1;
        }
    }
    if !number_2 {
        // parse_number:
        let c = zc(i);
        if c <= b'0' {
            if c == b'0' {
                if (zc(i + 1) == b'x' || zc(i + 1) == b'X') && is_xdigit(zc(i + 2)) {
                    p.has_nonstd = true;
                    t = 0x01;
                    j = i + 3;
                    while is_xdigit(zc(j)) {
                        j += 1;
                    }
                    finish = true;
                } else if is_digit(zc(i + 1)) {
                    p.i_err = i + 1;
                    return -1;
                }
            } else if !is_digit(zc(i + 1)) {
                // O JSON5 aceita "+Infinity" e "-Infinity" assim mesmo. O SQLite aceita também
                // em qualquer caixa e aceita "+inf" e "-inf".
                if (zc(i + 1) == b'I' || zc(i + 1) == b'i')
                    && strnicmp(Some(off(z, i as usize + 1)), Some(b"inf"), 3) == 0
                {
                    p.has_nonstd = true;
                    if zc(i) == b'-' {
                        json_blob_append_node(p, JSONB_FLOAT, 6, Some(b"-9e999"));
                    } else {
                        json_blob_append_node(p, JSONB_FLOAT, 5, Some(b"9e999"));
                    }
                    let more = if strnicmp(Some(off(z, i as usize + 4)), Some(b"inity"), 5) == 0 {
                        9
                    } else {
                        4
                    };
                    return (i + more) as i32;
                }
                if zc(i + 1) == b'.' {
                    p.has_nonstd = true;
                    t |= 0x01;
                } else {
                    p.i_err = i;
                    return -1;
                }
            } else if zc(i + 1) == b'0' {
                if is_digit(zc(i + 2)) {
                    p.i_err = i + 1;
                    return -1;
                } else if (zc(i + 2) == b'x' || zc(i + 2) == b'X') && is_xdigit(zc(i + 3)) {
                    p.has_nonstd = true;
                    t |= 0x01;
                    j = i + 4;
                    while is_xdigit(zc(j)) {
                        j += 1;
                    }
                    finish = true;
                }
            }
        }
    }
    if !finish {
        // parse_number_2:
        j = i + 1;
        loop {
            let mut c = zc(j);
            if is_digit(c) {
                j += 1;
                continue;
            }
            if c == b'.' {
                if (t & 0x02) != 0 {
                    p.i_err = j;
                    return -1;
                }
                t |= 0x02;
                j += 1;
                continue;
            }
            if c == b'e' || c == b'E' {
                if zc(j - 1) < b'0' {
                    if zc(j - 1) == b'.' && j >= i + 2 && is_digit(zc(j - 2)) {
                        p.has_nonstd = true;
                        t |= 0x01;
                    } else {
                        p.i_err = j;
                        return -1;
                    }
                }
                if seen_e {
                    p.i_err = j;
                    return -1;
                }
                t |= 0x02;
                seen_e = true;
                c = zc(j + 1);
                if c == b'+' || c == b'-' {
                    j += 1;
                    c = zc(j + 1);
                }
                if c < b'0' || c > b'9' {
                    p.i_err = j;
                    return -1;
                }
                j += 1;
                continue;
            }
            break;
        }
        if zc(j - 1) < b'0' {
            if zc(j - 1) == b'.' && j >= i + 2 && is_digit(zc(j - 2)) {
                p.has_nonstd = true;
                t |= 0x01;
            } else {
                p.i_err = j;
                return -1;
            }
        }
    }
    // parse_number_finish:
    if zc(i) == b'+' {
        i += 1;
    }
    json_blob_append_node(p, JSONB_INT + t, j - i, Some(sub(z, i, j - i)));
    j as i32
}

/// `jsonConvertTextToBlob`: analisa um texto JSON completo. Devolve zero se deu certo e 1 se há
/// erro; num erro a memória da análise é devolvida.
pub(crate) fn json_convert_text_to_blob(p: &mut JsonParse, ctx: Option<&mut Context<'_>>) -> i32 {
    let zj: Rc<Vec<u8>> = p.z_json.clone().unwrap_or_default();
    let z: &[u8] = &zj;
    let mut i = json_translate_text_to_blob(p, 0);
    if p.oom {
        i = -1;
    }
    let mut ctx = ctx;
    if i > 0 {
        let mut iu = i as usize;
        while json_is_space(at(z, iu)) {
            iu += 1;
        }
        if at(z, iu) != 0 {
            iu += json5_whitespace(off(z, iu));
            if at(z, iu) != 0 {
                if let Some(c) = ctx.as_deref_mut() {
                    result_error(c, b"malformed JSON", -1);
                }
                json_parse_reset(p);
                return 1;
            }
            p.has_nonstd = true;
        }
    }
    if i <= 0 {
        if let Some(c) = ctx.as_deref_mut() {
            if p.oom {
                result_error_nomem(c);
            } else {
                result_error(c, b"malformed JSON", -1);
            }
        }
        json_parse_reset(p);
        return 1;
    }
    0
}

// ---------------------------------------------------------------------------------------------
// De JSONB para texto
// ---------------------------------------------------------------------------------------------

/// `jsonTranslateBlobToText`: traduz o JSONB a partir de `i` em texto JSON e o acrescenta a
/// `out`. Devolve o índice do primeiro byte depois do elemento traduzido. Um JSONB mal formado
/// pode ligar `JSTRING_MALFORMED`.
pub(crate) fn json_translate_blob_to_text(p: &JsonParse, i: u32, out: &mut JsonString) -> u32 {
    let (n, sz) = json_payload_size(p, i);
    if n == 0 {
        out.e_err |= JSTRING_MALFORMED;
        return p.n_blob() + 1;
    }
    let blob = p.blob();
    let mut malformed = false;
    match p.blob_at(i) & 0x0f {
        JSONB_NULL => {
            out.append_raw(b"null");
            return i + 1;
        }
        JSONB_TRUE => {
            out.append_raw(b"true");
            return i + 1;
        }
        JSONB_FALSE => {
            out.append_raw(b"false");
            return i + 1;
        }
        JSONB_INT | JSONB_FLOAT => {
            if sz == 0 {
                malformed = true;
            } else {
                out.append_raw(sub(blob, i + n, sz));
            }
        }
        JSONB_INT5 => {
            // Literal inteiro em notação hexadecimal.
            let zin = sub(blob, i + n, sz);
            let mut k: u32 = 2;
            let mut u: u64 = 0;
            let mut b_overflow = false;
            if sz == 0 {
                malformed = true;
            } else {
                if at(zin, 0) == b'-' {
                    out.append_char(b'-');
                    k += 1;
                } else if at(zin, 0) == b'+' {
                    k += 1;
                }
                while k < sz {
                    let c = at(zin, k as usize);
                    if !is_xdigit(c) {
                        out.e_err |= JSTRING_MALFORMED;
                        break;
                    } else if (u >> 60) != 0 {
                        b_overflow = true;
                    } else {
                        u = u * 16 + hex_to_int(c as i32) as u64;
                    }
                    k += 1;
                }
                if b_overflow {
                    out.append_raw(b"9.0e999");
                } else {
                    out.append_raw(u.to_string().as_bytes());
                }
            }
        }
        JSONB_FLOAT5 => {
            // Literal de ponto flutuante sem dígitos ao lado do ".".
            let zin = sub(blob, i + n, sz);
            let mut k: u32 = 0;
            if sz == 0 {
                malformed = true;
            } else {
                if at(zin, 0) == b'-' {
                    out.append_char(b'-');
                    k += 1;
                }
                if at(zin, k as usize) == b'.' {
                    out.append_char(b'0');
                }
                while k < sz {
                    let c = at(zin, k as usize);
                    out.append_char(c);
                    if c == b'.' && (k + 1 == sz || !is_digit(at(zin, k as usize + 1))) {
                        out.append_char(b'0');
                    }
                    k += 1;
                }
            }
        }
        JSONB_TEXT | JSONB_TEXTJ => {
            out.append_char(b'"');
            out.append_raw(sub(blob, i + n, sz));
            out.append_char(b'"');
        }
        JSONB_TEXT5 => {
            let zin = sub(blob, i + n, sz);
            let mut pos = 0usize;
            let mut sz2 = zin.len();
            out.append_char(b'"');
            while sz2 > 0 {
                let mut k = 0usize;
                while k < sz2 && (json_is_ok(at(zin, pos + k)) || at(zin, pos + k) == b'\'') {
                    k += 1;
                }
                if k > 0 {
                    out.append_raw(&zin[pos..pos + k]);
                    if k >= sz2 {
                        break;
                    }
                    pos += k;
                    sz2 -= k;
                }
                if at(zin, pos) == b'"' {
                    out.append_raw(b"\\\"");
                    pos += 1;
                    sz2 -= 1;
                    continue;
                }
                if at(zin, pos) <= 0x1f {
                    out.append_control_char(at(zin, pos));
                    pos += 1;
                    sz2 -= 1;
                    continue;
                }
                // zin[pos] é '\\'
                if sz2 < 2 {
                    out.e_err |= JSTRING_MALFORMED;
                    break;
                }
                match at(zin, pos + 1) {
                    b'\'' => out.append_char(b'\''),
                    b'v' => out.append_raw(b"\\u0009"),
                    b'x' => {
                        if sz2 < 4 {
                            out.e_err |= JSTRING_MALFORMED;
                            sz2 = 2;
                        } else {
                            out.append_raw(b"\\u00");
                            out.append_raw(sub(zin, (pos + 2) as u32, 2));
                            pos += 2;
                            sz2 -= 2;
                        }
                    }
                    b'0' => out.append_raw(b"\\u0000"),
                    b'\r' => {
                        if sz2 > 2 && at(zin, pos + 2) == b'\n' {
                            pos += 1;
                            sz2 -= 1;
                        }
                    }
                    b'\n' => {}
                    0xe2 => {
                        // '\' seguido de U+2028 ou U+2029 é espaço e se ignora. Em UTF-8, U+2028
                        // é 0xe2 0x80 0xa8 e U+2029 só difere no último byte.
                        if sz2 < 4
                            || 0x80 != at(zin, pos + 2)
                            || (0xa8 != at(zin, pos + 3) && 0xa9 != at(zin, pos + 3))
                        {
                            out.e_err |= JSTRING_MALFORMED;
                            sz2 = 2;
                        } else {
                            pos += 2;
                            sz2 -= 2;
                        }
                    }
                    _ => out.append_raw(sub(zin, pos as u32, 2)),
                }
                pos += 2;
                sz2 -= 2;
            }
            out.append_char(b'"');
        }
        JSONB_TEXTRAW => {
            out.append_string(sub(blob, i + n, sz));
        }
        JSONB_ARRAY => {
            out.append_char(b'[');
            let mut j = i + n;
            let i_end = j + sz;
            while j < i_end && out.e_err == 0 {
                j = json_translate_blob_to_text(p, j, out);
                out.append_char(b',');
            }
            if j > i_end {
                out.e_err |= JSTRING_MALFORMED;
            }
            if sz > 0 {
                out.trim_one_char();
            }
            out.append_char(b']');
        }
        JSONB_OBJECT => {
            let mut x = 0u32;
            out.append_char(b'{');
            let mut j = i + n;
            let i_end = j + sz;
            while j < i_end && out.e_err == 0 {
                j = json_translate_blob_to_text(p, j, out);
                out.append_char(if (x & 1) != 0 { b',' } else { b':' });
                x += 1;
            }
            if (x & 1) != 0 || j > i_end {
                out.e_err |= JSTRING_MALFORMED;
            }
            if sz > 0 {
                out.trim_one_char();
            }
            out.append_char(b'}');
        }
        _ => malformed = true,
    }
    if malformed {
        out.e_err |= JSTRING_MALFORMED;
    }
    i + n + sz
}

/// `struct JsonPretty`: o contexto da recursão de `json_pretty()`.
pub(crate) struct JsonPretty<'a> {
    /// O JSONB a mostrar.
    pub p_parse: &'a JsonParse,
    /// Onde gerar a saída.
    pub p_out: &'a mut JsonString,
    /// O texto usado na indentação.
    pub z_indent: Vec<u8>,
    /// O nível de indentação atual.
    pub n_indent: u32,
}

impl JsonPretty<'_> {
    /// `jsonPrettyIndent`.
    fn indent(&mut self) {
        for _ in 0..self.n_indent {
            self.p_out.append_raw(&self.z_indent);
        }
    }
}

/// `jsonTranslateBlobToPrettyText`: como [`json_translate_blob_to_text`] mas com espaços extras
/// que deixam o JSON mais fácil de ler.
pub(crate) fn json_translate_blob_to_pretty_text(pretty: &mut JsonPretty<'_>, i: u32) -> u32 {
    let p = pretty.p_parse;
    let (n, sz) = json_payload_size(p, i);
    if n == 0 {
        pretty.p_out.e_err |= JSTRING_MALFORMED;
        return p.n_blob() + 1;
    }
    let mut i = i;
    match p.blob_at(i) & 0x0f {
        JSONB_ARRAY => {
            let mut j = i + n;
            let i_end = j + sz;
            pretty.p_out.append_char(b'[');
            if j < i_end {
                pretty.p_out.append_char(b'\n');
                pretty.n_indent += 1;
                while pretty.p_out.e_err == 0 {
                    pretty.indent();
                    j = json_translate_blob_to_pretty_text(pretty, j);
                    if j >= i_end {
                        break;
                    }
                    pretty.p_out.append_raw(b",\n");
                }
                pretty.p_out.append_char(b'\n');
                pretty.n_indent -= 1;
                pretty.indent();
            }
            pretty.p_out.append_char(b']');
            i = i_end;
        }
        JSONB_OBJECT => {
            let mut j = i + n;
            let i_end = j + sz;
            pretty.p_out.append_char(b'{');
            if j < i_end {
                pretty.p_out.append_char(b'\n');
                pretty.n_indent += 1;
                while pretty.p_out.e_err == 0 {
                    pretty.indent();
                    j = json_translate_blob_to_text(p, j, pretty.p_out);
                    if j > i_end {
                        pretty.p_out.e_err |= JSTRING_MALFORMED;
                        break;
                    }
                    pretty.p_out.append_raw(b": ");
                    j = json_translate_blob_to_pretty_text(pretty, j);
                    if j >= i_end {
                        break;
                    }
                    pretty.p_out.append_raw(b",\n");
                }
                pretty.p_out.append_char(b'\n');
                pretty.n_indent -= 1;
                pretty.indent();
            }
            pretty.p_out.append_char(b'}');
            i = i_end;
        }
        _ => {
            i = json_translate_blob_to_text(p, i, pretty.p_out);
        }
    }
    i
}

/// `jsonFuncArgMightBeBinary`: verdadeiro se o argumento é um BLOB que talvez seja JSONB. Não
/// confere o conteúdo em detalhe: há falsos positivos, nunca falsos negativos.
pub(crate) fn json_func_arg_might_be_binary(p_json: &Mem) -> bool {
    if value_type(p_json) != SQLITE_BLOB {
        return false;
    }
    let blob = blob_of(p_json);
    if blob.is_empty() {
        return false;
    }
    if (blob[0] & 0x0f) > JSONB_OBJECT {
        return false;
    }
    let (n, sz) = json_payload_size_raw(&blob, 0, 0);
    if n == 0 {
        return false;
    }
    if sz + n != blob.len() as u32 {
        return false;
    }
    if (blob[0] & 0x0f) <= JSONB_FALSE && sz > 0 {
        return false;
    }
    sz + n == blob.len() as u32
}

/// `jsonbArrayCount`: o número de elementos do vetor que começa em `i_root`.
pub(crate) fn json_array_count(p: &JsonParse, i_root: u32) -> u32 {
    let (mut n, mut sz) = json_payload_size(p, i_root);
    let i_end = i_root + n + sz;
    let mut k = 0u32;
    let mut i = i_root + n;
    while n > 0 && i < i_end {
        let (n2, sz2) = json_payload_size(p, i);
        n = n2;
        sz = sz2;
        i = i.wrapping_add(sz + n);
        k += 1;
    }
    k
}

/// `jsonAfterEditSizeAdjust`: corrige o tamanho do payload do elemento em `i_root` pela variação
/// `p.delta`.
pub(crate) fn json_after_edit_size_adjust(p: &mut JsonParse, i_root: u32) {
    let (_, sz) = json_payload_size(p, i_root);
    let sz = sz.wrapping_add(p.delta as u32);
    let d = json_blob_change_payload_size(p, i_root, sz);
    p.delta += d;
}

/// `jsonBlobEdit`: tira `n_del` bytes a partir de `i_del` e põe no lugar os `n_ins` bytes de
/// `a_ins` (sem `a_ins`, o espaço novo fica com zeros).
pub(crate) fn json_blob_edit(
    p: &mut JsonParse,
    i_del: u32,
    n_del: u32,
    a_ins: Option<&[u8]>,
    n_ins: u32,
) {
    let d = n_ins as i64 - n_del as i64;
    if d != 0 {
        let v = p.blob_mut();
        let old = v.len();
        let a = (i_del as usize).min(old);
        let b = (i_del as usize + n_del as usize).min(old);
        if d > 0 {
            v.resize(old + d as usize, 0);
            v.copy_within(b..old, a + n_ins as usize);
        } else {
            v.copy_within(b..old, a + n_ins as usize);
            v.truncate(old - (-d) as usize);
        }
        p.delta += d as i32;
    }
    if n_ins > 0 {
        if let Some(ins) = a_ins {
            let v = p.blob_mut();
            let a = i_del as usize;
            let src = sub(ins, 0, n_ins);
            if a + src.len() <= v.len() {
                v[a..a + src.len()].copy_from_slice(src);
            }
        }
    }
}

/// `jsonBytesToBypass`: o número de bytes de quebras de linha escapadas a ignorar.
fn json_bytes_to_bypass(z: &[u8], n: u32) -> u32 {
    let mut i = 0u32;
    while i + 1 < n {
        if at(z, i as usize) != b'\\' {
            return i;
        }
        if at(z, i as usize + 1) == b'\n' {
            i += 2;
            continue;
        }
        if at(z, i as usize + 1) == b'\r' {
            if i + 2 < n && at(z, i as usize + 2) == b'\n' {
                i += 3;
            } else {
                i += 2;
            }
            continue;
        }
        if 0xe2 == at(z, i as usize + 1)
            && i + 3 < n
            && 0x80 == at(z, i as usize + 2)
            && (0xa8 == at(z, i as usize + 3) || 0xa9 == at(z, i as usize + 3))
        {
            i += 4;
            continue;
        }
        break;
    }
    i
}

/// `jsonUnescapeOneChar`: `z` é a sequência de escape, com a `\` inicial. Devolve
/// `(bytes da sequência, caractere)`; num erro de sintaxe o caractere é `JSON_INVALID_CHAR`.
pub(crate) fn json_unescape_one_char(z: &[u8]) -> (u32, u32) {
    let n = z.len() as u32;
    if n < 2 {
        return (n, JSON_INVALID_CHAR);
    }
    match at(z, 1) {
        b'u' => {
            if n < 6 {
                return (n, JSON_INVALID_CHAR);
            }
            let v = json_hex_to_int4(off(z, 2));
            let vlo = json_hex_to_int4(off(z, 8));
            if (v & 0xfc00) == 0xd800
                && n >= 12
                && at(z, 6) == b'\\'
                && at(z, 7) == b'u'
                && (vlo & 0xfc00) == 0xdc00
            {
                (12, ((v & 0x3ff) << 10) + (vlo & 0x3ff) + 0x10000)
            } else {
                (6, v)
            }
        }
        b'b' => (2, 0x08),
        b'f' => (2, 0x0c),
        b'n' => (2, b'\n' as u32),
        b'r' => (2, b'\r' as u32),
        b't' => (2, b'\t' as u32),
        b'v' => (2, 0x0b),
        b'0' => (2, 0),
        b'\'' | b'"' | b'/' | b'\\' => (2, at(z, 1) as u32),
        b'x' => {
            if n < 4 {
                return (n, JSON_INVALID_CHAR);
            }
            (
                4,
                ((hex_to_int(at(z, 2) as i32) as u32) << 4) | hex_to_int(at(z, 3) as i32) as u32,
            )
        }
        0xe2 | b'\r' | b'\n' => {
            let n_skip = json_bytes_to_bypass(z, n);
            if n_skip == 0 {
                (n, JSON_INVALID_CHAR)
            } else if n_skip == n {
                (n, 0)
            } else if at(z, n_skip as usize) == b'\\' {
                let (k, v) = json_unescape_one_char(off(z, n_skip as usize));
                (n_skip + k, v)
            } else {
                let (v, sz) = utf8_read_limited(off(z, n_skip as usize), (n - n_skip) as i32);
                (n_skip + sz as u32, v)
            }
        }
        _ => (2, JSON_INVALID_CHAR),
    }
}

/// `jsonLabelCompareEscaped`: compara dois rótulos de objeto, um deles ou os dois com sequências
/// de escape. Devolve verdadeiro se são iguais.
fn json_label_compare_escaped(zl: &[u8], raw_left: bool, zr: &[u8], raw_right: bool) -> bool {
    let mut l = zl;
    let mut r = zr;
    loop {
        let c_left: u32;
        if l.is_empty() {
            c_left = 0;
        } else if raw_left || l[0] != b'\\' {
            let c = l[0] as u32;
            if c >= 0xc0 {
                let (v, sz) = utf8_read_limited(l, l.len() as i32);
                c_left = v;
                l = off(l, sz as usize);
            } else {
                c_left = c;
                l = off(l, 1);
            }
        } else {
            let (k, v) = json_unescape_one_char(l);
            c_left = v;
            l = off(l, k as usize);
        }
        let c_right: u32;
        if r.is_empty() {
            c_right = 0;
        } else if raw_right || r[0] != b'\\' {
            let c = r[0] as u32;
            if c >= 0xc0 {
                let (v, sz) = utf8_read_limited(r, r.len() as i32);
                c_right = v;
                r = off(r, sz as usize);
            } else {
                c_right = c;
                r = off(r, 1);
            }
        } else {
            let (k, v) = json_unescape_one_char(r);
            c_right = v;
            r = off(r, k as usize);
        }
        if c_left != c_right {
            return false;
        }
        if c_left == 0 {
            return true;
        }
    }
}

/// `jsonLabelCompare`: compara dois rótulos de objeto; verdadeiro se são iguais.
pub(crate) fn json_label_compare(zl: &[u8], raw_left: bool, zr: &[u8], raw_right: bool) -> bool {
    if raw_left && raw_right {
        // O caso mais simples: nenhum tem escapes e um memcmp basta.
        zl == zr
    } else {
        json_label_compare_escaped(zl, raw_left, zr, raw_right)
    }
}

// ---------------------------------------------------------------------------------------------
// A busca por caminho (e a edição)
// ---------------------------------------------------------------------------------------------

/// `jsonCreateEditSubstructure`: monta em `p_ins` o JSONB a inserir. No caso comum é só o
/// conteúdo de `p.a_ins`; se o caminho continua além do ponto de inserção, cria a subestrutura
/// que falta (`json_insert('{}', '$.a.b.c', 123)` insere `{"b":{"c":123}}` em `$.a`).
fn json_create_edit_substructure(p: &mut JsonParse, p_ins: &mut JsonParse, z_tail: &[u8]) -> u32 {
    *p_ins = JsonParse::default();
    if at(z_tail, 0) == 0 {
        // Sem subestrutura: insere o que está em `p.a_ins`.
        p_ins.a_blob = p.a_ins.clone();
        0
    } else {
        // Constrói a subestrutura binária.
        let first = if at(z_tail, 0) == b'.' { JSONB_OBJECT } else { JSONB_ARRAY };
        p_ins.a_blob = Rc::new(vec![first]);
        p_ins.e_edit = p.e_edit;
        p_ins.a_ins = p.a_ins.clone();
        let rc = json_lookup_step(p_ins, 0, z_tail, 0);
        p.oom |= p_ins.oom;
        rc
    }
}

/// `jsonLookupStep`: procura em `z_path` o elemento do JSON e devolve o índice do seu valor em
/// `p.a_blob`. Se o valor achado é a metade valor de um par rótulo e valor, `p.i_label` recebe o
/// início do rótulo. Devolve um dos `JSON_LOOKUP_*` de erro nos problemas. Se `p.e_edit` é uma
/// operação de edição, o JSONB também é modificado, e então o valor devolvido só serve para
/// detectar erros.
pub(crate) fn json_lookup_step(
    p: &mut JsonParse,
    i_root: u32,
    z_path: &[u8],
    i_label: u32,
) -> u32 {
    let zp = z_path;
    if at(zp, 0) == 0 {
        let mut i_root = i_root;
        let n_extra = p.a_ins.len();
        if p.e_edit != 0 && json_blob_make_editable(p, n_extra) {
            let (n, mut sz) = json_payload_size(p, i_root);
            sz += n;
            if p.e_edit == JEDIT_DEL {
                if i_label > 0 {
                    sz += i_root - i_label;
                    i_root = i_label;
                }
                json_blob_edit(p, i_root, sz, None, 0);
            } else if p.e_edit == JEDIT_INS {
                // Já existe, então json_insert() não faz nada.
            } else {
                // json_set() ou json_replace().
                let ins = p.a_ins.clone();
                json_blob_edit(p, i_root, sz, Some(&ins), ins.len() as u32);
            }
        }
        p.i_label = i_label;
        return i_root;
    }
    if at(zp, 0) == b'.' {
        let mut raw_key = true;
        let x = p.blob_at(i_root);
        let zp = off(zp, 1);
        let key_start: usize;
        let n_key: usize;
        let mut i: usize;
        if at(zp, 0) == b'"' {
            key_start = 1;
            i = 1;
            while at(zp, i) != 0 && at(zp, i) != b'"' {
                i += 1;
            }
            n_key = i - 1;
            if at(zp, i) != 0 {
                i += 1;
            } else {
                return JSON_LOOKUP_PATHERROR;
            }
            raw_key = !zp[key_start..key_start + n_key].contains(&b'\\');
        } else {
            key_start = 0;
            i = 0;
            while at(zp, i) != 0 && at(zp, i) != b'.' && at(zp, i) != b'[' {
                i += 1;
            }
            n_key = i;
            if n_key == 0 {
                return JSON_LOOKUP_PATHERROR;
            }
        }
        let z_key = &zp[key_start..key_start + n_key];
        if (x & 0x0f) != JSONB_OBJECT {
            return JSON_LOOKUP_NOTFOUND;
        }
        let (n, sz) = json_payload_size(p, i_root);
        let mut j = i_root + n; // j é o índice de um rótulo
        let i_end = j + sz;
        while j < i_end {
            let x = p.blob_at(j) & 0x0f;
            if x < JSONB_TEXT || x > JSONB_TEXTRAW {
                return JSON_LOOKUP_ERROR;
            }
            let (n, sz) = json_payload_size(p, j);
            if n == 0 {
                return JSON_LOOKUP_ERROR;
            }
            let k = j + n; // k é o índice do texto do rótulo
            if k + sz >= i_end {
                return JSON_LOOKUP_ERROR;
            }
            let raw_label = x == JSONB_TEXT || x == JSONB_TEXTRAW;
            let matched = json_label_compare(z_key, raw_key, sub(p.blob(), k, sz), raw_label);
            if matched {
                let v = k + sz; // v é o índice do valor
                if (p.blob_at(v) & 0x0f) > JSONB_OBJECT {
                    return JSON_LOOKUP_ERROR;
                }
                let (n, sz) = json_payload_size(p, v);
                if n == 0 || v + n + sz > i_end {
                    return JSON_LOOKUP_ERROR;
                }
                let rc = json_lookup_step(p, v, off(zp, i), j);
                if p.delta != 0 {
                    json_after_edit_size_adjust(p, i_root);
                }
                return rc;
            }
            j = k + sz;
            if (p.blob_at(j) & 0x0f) > JSONB_OBJECT {
                return JSON_LOOKUP_ERROR;
            }
            let (n, sz) = json_payload_size(p, j);
            if n == 0 {
                return JSON_LOOKUP_ERROR;
            }
            j += n + sz;
        }
        if j > i_end {
            return JSON_LOOKUP_ERROR;
        }
        if p.e_edit >= JEDIT_INS {
            // Total de bytes a inserir (rótulo e valor).
            let mut ix = JsonParse::default(); // O cabeçalho do rótulo a inserir
            json_blob_append_node(
                &mut ix,
                if raw_key { JSONB_TEXTRAW } else { JSONB_TEXT5 },
                n_key as u32,
                None,
            );
            let mut v = JsonParse::default(); // O JSONB do valor a inserir
            let rc = json_create_edit_substructure(p, &mut v, off(zp, i));
            if !json_lookup_is_error(rc) && json_blob_make_editable(p, ix.a_blob.len() + n_key + v.a_blob.len())
            {
                let n_ins = ix.n_blob() + n_key as u32 + v.n_blob();
                json_blob_edit(p, j, 0, None, n_ins);
                if !p.oom {
                    let buf = p.blob_mut();
                    let a = j as usize;
                    if a + n_ins as usize <= buf.len() {
                        buf[a..a + ix.a_blob.len()].copy_from_slice(&ix.a_blob);
                        let k = a + ix.a_blob.len();
                        buf[k..k + n_key].copy_from_slice(z_key);
                        let k = k + n_key;
                        buf[k..k + v.a_blob.len()].copy_from_slice(&v.a_blob);
                    }
                    if p.delta != 0 {
                        json_after_edit_size_adjust(p, i_root);
                    }
                }
            }
            return rc;
        }
    } else if at(zp, 0) == b'[' {
        let x = p.blob_at(i_root) & 0x0f;
        if x != JSONB_ARRAY {
            return JSON_LOOKUP_NOTFOUND;
        }
        let (n, sz) = json_payload_size(p, i_root);
        let mut k: u32 = 0;
        let mut i: usize = 1;
        while is_digit(at(zp, i)) {
            k = k.wrapping_mul(10).wrapping_add((at(zp, i) - b'0') as u32);
            i += 1;
        }
        if i < 2 || at(zp, i) != b']' {
            if at(zp, 1) == b'#' {
                k = json_array_count(p, i_root);
                i = 2;
                if at(zp, 2) == b'-' && is_digit(at(zp, 3)) {
                    let mut nn: u32 = 0;
                    i = 3;
                    loop {
                        nn = nn.wrapping_mul(10).wrapping_add((at(zp, i) - b'0') as u32);
                        i += 1;
                        if !is_digit(at(zp, i)) {
                            break;
                        }
                    }
                    if nn > k {
                        return JSON_LOOKUP_NOTFOUND;
                    }
                    k -= nn;
                }
                if at(zp, i) != b']' {
                    return JSON_LOOKUP_PATHERROR;
                }
            } else {
                return JSON_LOOKUP_PATHERROR;
            }
        }
        let mut j = i_root + n;
        let i_end = j + sz;
        while j < i_end {
            if k == 0 {
                let rc = json_lookup_step(p, j, off(zp, i + 1), 0);
                if p.delta != 0 {
                    json_after_edit_size_adjust(p, i_root);
                }
                return rc;
            }
            k -= 1;
            let (n, sz) = json_payload_size(p, j);
            if n == 0 {
                return JSON_LOOKUP_ERROR;
            }
            j += n + sz;
        }
        if j > i_end {
            return JSON_LOOKUP_ERROR;
        }
        if k > 0 {
            return JSON_LOOKUP_NOTFOUND;
        }
        if p.e_edit >= JEDIT_INS {
            let mut v = JsonParse::default();
            let rc = json_create_edit_substructure(p, &mut v, off(zp, i + 1));
            if !json_lookup_is_error(rc) && json_blob_make_editable(p, v.a_blob.len()) {
                json_blob_edit(p, j, 0, Some(&v.a_blob), v.n_blob());
            }
            if p.delta != 0 {
                json_after_edit_size_adjust(p, i_root);
            }
            return rc;
        }
    } else {
        return JSON_LOOKUP_PATHERROR;
    }
    JSON_LOOKUP_NOTFOUND
}

// ---------------------------------------------------------------------------------------------
// De JSONB para valor SQL
// ---------------------------------------------------------------------------------------------

/// `jsonReturnTextJsonFromBlob`: converte um JSONB em texto e o devolve como resultado.
pub(crate) fn json_return_text_json_from_blob(ctx: &mut Context<'_>, blob: &[u8]) {
    let x = JsonParse::from_blob(blob.to_vec());
    let mut s = JsonString::new();
    json_translate_blob_to_text(&x, 0, &mut s);
    json_return_string(ctx, &mut s, None);
}

/// Parte final dos casos de número de `jsonReturnFromBlob` (o `to_double`).
fn json_return_double(ctx: &mut Context<'_>, blob: &[u8], start: u32, sz: u32) {
    let z = sub(blob, start, sz).to_vec();
    let (rc, r) = atof(&z, strlen30(&z), SQLITE_UTF8 as u8, USE_LONG_DOUBLE);
    if rc <= 0 {
        result_error(ctx, b"malformed JSON", -1);
    } else {
        crate::vdbeapi::result_double(ctx, r);
    }
}

/// `jsonReturnFromBlob`: o valor do nó em `i`. Primitivos viram valores SQL; vetores e objetos
/// viram texto JSON ou JSONB, conforme `JSON_BLOB` (a menos que `text_only`).
pub(crate) fn json_return_from_blob(
    ctx: &mut Context<'_>,
    p: &JsonParse,
    i: u32,
    text_only: bool,
) {
    let (mut n, mut sz) = json_payload_size(p, i);
    if n == 0 {
        result_error(ctx, b"malformed JSON", -1);
        return;
    }
    let blob = p.blob();
    let malformed = |ctx: &mut Context<'_>| result_error(ctx, b"malformed JSON", -1);
    match p.blob_at(i) & 0x0f {
        JSONB_NULL => {
            if sz != 0 {
                malformed(ctx);
                return;
            }
            result_null(ctx);
        }
        JSONB_TRUE => {
            if sz != 0 {
                malformed(ctx);
                return;
            }
            result_int(ctx, 1);
        }
        JSONB_FALSE => {
            if sz != 0 {
                malformed(ctx);
                return;
            }
            result_int(ctx, 0);
        }
        JSONB_INT5 | JSONB_INT => {
            let mut i_res: i64 = 0;
            let mut b_neg = false;
            if sz == 0 {
                malformed(ctx);
                return;
            }
            let x = p.blob_at(i + n);
            if x == b'-' {
                if sz < 2 {
                    malformed(ctx);
                    return;
                }
                n += 1;
                sz -= 1;
                b_neg = true;
            }
            let z = sub(blob, i + n, sz).to_vec();
            let rc = dec_or_hex_to_i64(&z, &mut i_res);
            if rc == 0 {
                result_int64(ctx, if b_neg { i_res.wrapping_neg() } else { i_res });
            } else if rc == 3 && b_neg {
                result_int64(ctx, i64::MIN);
            } else if rc == 1 {
                malformed(ctx);
                return;
            } else {
                if b_neg {
                    n -= 1;
                    sz += 1;
                }
                json_return_double(ctx, blob, i + n, sz);
            }
        }
        JSONB_FLOAT5 | JSONB_FLOAT => {
            if sz == 0 {
                malformed(ctx);
                return;
            }
            json_return_double(ctx, blob, i + n, sz);
        }
        JSONB_TEXTRAW | JSONB_TEXT => {
            result_text(ctx, Some(sub(blob, i + n, sz)), sz as i32, StrDtor::Transient);
        }
        JSONB_TEXT5 | JSONB_TEXTJ => {
            // Traduz a cadeia formatada em JSON para texto cru.
            let z = sub(blob, i + n, sz);
            let mut out: Vec<u8> = Vec::with_capacity(z.len());
            let mut i_in = 0usize;
            while i_in < z.len() {
                let c = z[i_in];
                if c == b'\\' {
                    let (sz_escape, v) = json_unescape_one_char(&z[i_in..]);
                    if v <= 0x7f {
                        out.push(v as u8);
                    } else if v <= 0x7ff {
                        out.push((0xc0 | (v >> 6)) as u8);
                        out.push((0x80 | (v & 0x3f)) as u8);
                    } else if v < 0x10000 {
                        out.push((0xe0 | (v >> 12)) as u8);
                        out.push((0x80 | ((v >> 6) & 0x3f)) as u8);
                        out.push((0x80 | (v & 0x3f)) as u8);
                    } else if v == JSON_INVALID_CHAR {
                        // Ignora em silêncio o unicode ilegal.
                    } else {
                        out.push((0xf0 | (v >> 18)) as u8);
                        out.push((0x80 | ((v >> 12) & 0x3f)) as u8);
                        out.push((0x80 | ((v >> 6) & 0x3f)) as u8);
                        out.push((0x80 | (v & 0x3f)) as u8);
                    }
                    i_in += (sz_escape as usize).max(1) - 1;
                } else {
                    out.push(c);
                }
                i_in += 1;
            }
            result_text(ctx, Some(&out), out.len() as i32, StrDtor::Transient);
        }
        JSONB_ARRAY | JSONB_OBJECT => {
            let flags = if text_only { 0 } else { user_flags(ctx) };
            if flags & JSON_BLOB != 0 {
                let part = sub(blob, i, sz + n);
                result_blob(ctx, Some(part), part.len() as i32, StrDtor::Transient);
            } else {
                json_return_text_json_from_blob(ctx, sub(blob, i, sz + n));
            }
        }
        _ => malformed(ctx),
    }
}

// ---------------------------------------------------------------------------------------------
// Argumentos de função como JSONB
// ---------------------------------------------------------------------------------------------

/// `jsonFunctionArgToBlob`: o argumento (valor SQL ou JSON) como JSONB. Devolve `None` e já
/// deixa o erro no contexto quando o argumento é um BLOB que claramente não é JSONB.
pub(crate) fn json_function_arg_to_blob(ctx: &mut Context<'_>, arg: &Mem) -> Option<JsonParse> {
    let mut p = JsonParse::default();
    match value_type(arg) {
        SQLITE_BLOB => {
            if json_func_arg_might_be_binary(arg) {
                p.a_blob = Rc::new(blob_of(arg).into_owned());
            } else {
                result_error(ctx, b"JSON cannot hold BLOB values", -1);
                return None;
            }
        }
        SQLITE_TEXT => {
            let z = text_of(arg)?;
            if value_subtype(arg) == JSON_SUBTYPE {
                p.z_json = Some(Rc::new(z.to_vec()));
                if json_convert_text_to_blob(&mut p, Some(&mut *ctx)) != 0 {
                    result_error(ctx, b"malformed JSON", -1);
                    return None;
                }
            } else {
                json_blob_append_node(&mut p, JSONB_TEXTRAW, z.len() as u32, Some(&z));
            }
        }
        SQLITE_FLOAT => {
            let r = value_double(arg);
            if is_nan(r) {
                json_blob_append_node(&mut p, JSONB_NULL, 0, None);
            } else {
                let z = text_of(arg)?;
                if at(&z, 0) == b'I' {
                    json_blob_append_node(&mut p, JSONB_FLOAT, 5, Some(b"9e999"));
                } else if at(&z, 0) == b'-' && at(&z, 1) == b'I' {
                    json_blob_append_node(&mut p, JSONB_FLOAT, 6, Some(b"-9e999"));
                } else {
                    json_blob_append_node(&mut p, JSONB_FLOAT, z.len() as u32, Some(&z));
                }
            }
        }
        SQLITE_INTEGER => {
            let z = text_of(arg)?;
            json_blob_append_node(&mut p, JSONB_INT, z.len() as u32, Some(&z));
        }
        _ => {
            p.a_blob = Rc::new(vec![0x00]);
        }
    }
    Some(p)
}

/// `jsonBadPathError`: a mensagem de caminho inválido. Com `ctx` ela vira o erro da função e se
/// devolve `None`; sem `ctx` se devolve o texto.
pub(crate) fn json_bad_path_error(ctx: Option<&mut Context<'_>>, z_path: &[u8]) -> Option<Vec<u8>> {
    let msg = mprintf(b"bad JSON path: %Q", &[PrintfArg::Text(Some(z_path.to_vec()))]);
    match ctx {
        None => msg,
        Some(c) => {
            match msg {
                Some(m) => result_error(c, &m, -1),
                None => result_error_nomem(c),
            }
            None
        }
    }
}

/// `jsonArgIsJsonb`: se `arg` (um BLOB) parece JSONB, põe o JSONB em `p` e devolve verdadeiro.
fn json_arg_is_jsonb(arg: &Mem, p: &mut JsonParse) -> bool {
    let blob = blob_of(arg);
    if blob.is_empty() {
        return false;
    }
    let (n, sz) = json_payload_size_raw(&blob, 0, 0);
    if (blob[0] & 0x0f) <= JSONB_OBJECT
        && n > 0
        && sz + n == blob.len() as u32
        && ((blob[0] & 0x0f) > JSONB_FALSE || sz == 0)
    {
        p.a_blob = Rc::new(blob.into_owned());
        return true;
    }
    false
}

/// A cópia editável de uma entrada do cache (`rebuild_from_cache`).
fn json_rebuild_from_cache(from_cache: &JsonParse) -> JsonParse {
    let mut p = JsonParse::default();
    p.a_blob = from_cache.a_blob.clone();
    p.b_blob_owned = true;
    p.has_nonstd = from_cache.has_nonstd;
    p
}

/// `jsonParseFuncArg`: a análise do argumento de uma função (JSON em texto, JSONB ou valor SQL),
/// com o JSONB em `a_blob`. Devolve `None` com o erro já no contexto, e também para NULL, sem
/// erro, de modo que a função devolva NULL. Com `JSON_KEEPERROR`, um erro devolve a análise com
/// `n_err` ligado.
pub(crate) fn json_parse_func_arg(
    ctx: &mut Context<'_>,
    arg: &Mem,
    flgs: u32,
) -> Option<JsonParse> {
    let e_type = value_type(arg);
    if e_type == SQLITE_NULL {
        return None;
    }
    if let Some(from_cache) = json_cache_search(ctx, arg) {
        if (flgs & JSON_EDITABLE) == 0 {
            return Some(from_cache);
        }
        return Some(json_rebuild_from_cache(&from_cache));
    }
    let mut p = JsonParse::default();
    if e_type == SQLITE_BLOB && json_arg_is_jsonb(arg, &mut p) {
        if (flgs & JSON_EDITABLE) != 0 && !json_blob_make_editable(&mut p, 0) {
            result_error_nomem(ctx);
            return None;
        }
        return Some(p);
    }
    // Se o blob não é JSONB válido, cai na tentativa de lê-lo como texto JSON. Isso contraria a
    // documentação histórica (o blob era reservado e devia dar erro), mas muitas aplicações
    // dependem do comportamento, sobretudo com readfile() do CLI (tag-20240123-a no C).
    let Some(text) = text_of(arg) else {
        result_error_nomem(ctx);
        return None;
    };
    if text.is_empty() {
        if flgs & JSON_KEEPERROR != 0 {
            p.n_err = 1;
            return Some(p);
        }
        result_error(ctx, b"malformed JSON", -1);
        return None;
    }
    p.z_json = Some(Rc::new(text.into_owned()));
    let keep = flgs & JSON_KEEPERROR != 0;
    let failed = json_convert_text_to_blob(&mut p, if keep { None } else { Some(&mut *ctx) }) != 0;
    if failed {
        if keep {
            p.n_err = 1;
            return Some(p);
        }
        return None;
    }
    p.b_json_is_rc_str = true;
    json_cache_insert(ctx, &mut p);
    if flgs & JSON_EDITABLE != 0 {
        return Some(json_rebuild_from_cache(&p));
    }
    Some(p)
}

/// `jsonReturnParse`: o resultado da função é o JSONB de `p` ou o seu texto JSON, conforme a
/// opção `JSON_BLOB` da função.
pub(crate) fn json_return_parse(ctx: &mut Context<'_>, p: &mut JsonParse) {
    if p.oom {
        result_error_nomem(ctx);
        return;
    }
    let flgs = user_flags(ctx);
    if flgs & JSON_BLOB != 0 {
        result_blob(ctx, Some(p.blob()), p.n_blob() as i32, StrDtor::Transient);
    } else {
        let mut s = JsonString::new();
        p.delta = 0;
        json_translate_blob_to_text(p, 0, &mut s);
        json_return_string(ctx, &mut s, Some(p));
        result_subtype(ctx, JSON_SUBTYPE);
    }
}
