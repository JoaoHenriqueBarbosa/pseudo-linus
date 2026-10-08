// Mesclado das partes traduzidas de json_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Tipos de elemento do JSONB.
pub const JSONB_NULL: u8 = 0; // "null"
pub const JSONB_TRUE: u8 = 1; // "true"
pub const JSONB_FALSE: u8 = 2; // "false"
pub const JSONB_INT: u8 = 3; // inteiro aceito por JSON e SQL
pub const JSONB_INT5: u8 = 4; // inteiro em notação 0x000
pub const JSONB_FLOAT: u8 = 5; // ponto flutuante aceito por JSON e SQL
pub const JSONB_FLOAT5: u8 = 6; // ponto flutuante com extensões do JSON5
pub const JSONB_TEXT: u8 = 7; // texto compatível com JSON e SQL
pub const JSONB_TEXTJ: u8 = 8; // texto com escapes do JSON
pub const JSONB_TEXT5: u8 = 9; // texto com escapes do JSON-5
pub const JSONB_TEXTRAW: u8 = 10; // texto SQL que precisa de escape para JSON
pub const JSONB_ARRAY: u8 = 11; // um array
pub const JSONB_OBJECT: u8 = 12; // um objeto

/// Nomes legíveis dos valores JSONB. O índice de cada texto corresponde ao
/// valor inteiro `JSONB_*` acima.
pub static JSONB_TYPE: [&[u8]; 17] = [
    b"null", b"true", b"false", b"integer", b"integer", b"real", b"real", b"text", b"text",
    b"text", b"text", b"array", b"object", b"", b"", b"", b"",
];

/// Tabela de espaços reconhecidos pelo parser de JSON (mais rápida que o
/// isspace da biblioteca).
pub static JSON_IS_SPACE: [u8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Equivalente do macro `jsonIsspace(x)`.
#[inline]
pub fn json_isspace(x: u8) -> u8 {
    JSON_IS_SPACE[x as usize]
}

/// O conjunto de todos os espaços reconhecidos por `json_isspace()`: tab, LF,
/// CR e espaço. Útil como segundo argumento de strspn.
pub const JSON_SPACES: &[u8] = b"\x09\x0a\x0d\x20";

/// Caracteres que não são especiais para JSON. Os de controle, '"', '\\' e
/// '\'' ficam de fora (o apóstrofo não é especial no JSON canônico, mas é no
/// JSON-5, então entra no conjunto de especiais).
pub static JSON_IS_OK: [u8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
];

/// Número mágico usado para o cache de parse de JSON em `get_auxdata()`.
pub const JSON_CACHE_ID: i32 = -429938; // entrada do cache
pub const JSON_CACHE_SIZE: usize = 4; // máximo de entradas no cache

/// `json_unescape_one_char()` devolve este ponto de código inválido ao
/// encontrar um erro de sintaxe.
pub const JSON_INVALID_CHAR: u32 = 0x99999;

/// Um cache que mapeia texto JSON em blobs JSONB.
///
/// Cada entrada é um objeto `JsonParse` com as restrições: `b_read_only`
/// ligado; `a_blob` pertence ao próprio objeto (`n_blob_alloc` diferente de
/// zero); `e_edit` e `delta` zerados; `z_json` é um RCStr (`b_json_is_rc_str`
/// verdadeiro).
#[derive(Default)]
pub struct JsonCache {
    pub db: Option<Sqlite3Ref>, // conexão com o banco
    pub n_used: i32,            // número de entradas ativas no cache
    pub a: [Option<JsonParseRef>; JSON_CACHE_SIZE], // uma linha por entrada
}

/// Uma string JSON em construção. Na verdade é um acumulador genérico de
/// strings, usado também para gerar textos que não são JSON.
///
/// No C, se o texto não cabe em `z_space[]`, vira uma string RCStr. Aqui
/// `z_buf` é sempre um `Vec<u8>`; `b_static` e `z_space` ficam para manter
/// a mesma contabilidade de `n_alloc`.
pub struct JsonString {
    pub p_ctx: Option<Sqlite3ContextRef>, // contexto da função: mensagens de erro vão aqui
    pub z_buf: Vec<u8>,                   // acrescenta o conteúdo JSON aqui
    pub n_alloc: u64,                     // bytes de armazenamento disponíveis em z_buf[]
    pub n_used: u64,                      // bytes de z_buf[] em uso
    pub b_static: u8,                     // verdadeiro se z_buf é o espaço estático
    pub e_err: u8,                        // verdadeiro se houve erro
    pub z_space: [u8; 100],               // espaço estático inicial
}

// Valores permitidos para JsonString.e_err
pub const JSTRING_OOM: u8 = 0x01; // sem memória
pub const JSTRING_MALFORMED: u8 = 0x02; // JSONB malformado
pub const JSTRING_ERR: u8 = 0x04; // erro já enviado para result

/// O "subtype" dos valores de texto JSON passados por `result_subtype()` e
/// `value_subtype()`.
pub const JSON_SUBTYPE: u32 = 74; // ASCII de "J"

// Bits dos flags passados às funções SQL via `user_data()`.
pub const JSON_JSON: i32 = 0x01; // o resultado é sempre JSON
pub const JSON_SQL: i32 = 0x02; // o resultado é sempre SQL
pub const JSON_ABPATH: i32 = 0x03; // aceita caminhos JSON abreviados
pub const JSON_ISSET: i32 = 0x04; // json_set(), não json_insert()
pub const JSON_BLOB: i32 = 0x08; // usa o formato de saída BLOB

/// Um valor JSON já interpretado. Ciclo de vida: o JSON chega e é convertido
/// em JSONB em `a_blob` (o texto original fica em `z_json`; o passo é
/// ignorado se a entrada já é JSONB); `a_blob[]` é pesquisado pela notação
/// de caminho JSON, se preciso; zero ou mais alterações são feitas em
/// `a_blob[]`; por fim o texto JSON de saída é gerado (ignorado nas funções
/// `jsonb_*`, que devolvem JSONB).
#[derive(Default)]
pub struct JsonParse {
    pub a_blob: Vec<u8>,          // representação JSONB do valor JSON
    pub n_blob: u32,              // bytes de a_blob[] realmente usados
    pub n_blob_alloc: u32,        // bytes alocados em a_blob[]; 0 se a_blob é externo
    pub z_json: Vec<u8>,          // texto JSON usado no parse
    pub db: Option<Sqlite3Ref>,   // conexão a que este objeto pertence
    pub n_json: i32,              // tamanho de z_json em bytes
    pub n_jp_ref: u32,            // número de referências a este objeto
    pub i_err: u32,               // posição do erro em z_json[]
    pub i_depth: u16,             // profundidade de aninhamento
    pub n_err: u8,                // número de erros vistos
    pub oom: u8,                  // verdadeiro se faltou memória
    pub b_json_is_rc_str: u8,     // verdadeiro se z_json é um RCStr
    pub has_nonstd: u8,           // verdadeiro se a entrada usa recursos não padrão como JSON5
    pub b_read_only: u8,          // não modificar
    // Informações de busca e edição. Ver json_lookup_step().
    pub e_edit: u8,               // operação de edição a aplicar
    pub delta: i32,               // mudança de tamanho causada pela edição
    pub n_ins: u32,               // bytes a inserir
    pub i_label: u32,             // posição do rótulo se a busca parou num valor de objeto
    pub a_ins: Vec<u8>,           // conteúdo a inserir
}

/// Referência compartilhada a um `JsonParse` (o C usa contagem em `n_jp_ref`).
pub type JsonParseRef = Rc<RefCell<JsonParse>>;

// Valores permitidos para JsonParse.e_edit
pub const JEDIT_DEL: u8 = 1; // apaga se existe
pub const JEDIT_REPL: u8 = 2; // sobrescreve se existe
pub const JEDIT_INS: u8 = 3; // insere se não existe
pub const JEDIT_SET: u8 = 4; // insere ou sobrescreve

/// Profundidade máxima de aninhamento de JSON nesta implementação. Evita
/// estouro de pilha no parser de descida recursiva.
pub const JSON_MAX_DEPTH: u16 = 1000;

// Valores permitidos para o argumento flgs de json_parse_func_arg().
pub const JSON_EDITABLE: u32 = 0x01; // gera um objeto JsonParse gravável
pub const JSON_KEEPERROR: u32 = 0x02; // devolve não nulo mesmo havendo erro

/// Libera um objeto JsonCache.
pub fn json_cache_delete(p: &mut JsonCache) {
    let mut i: i32 = 0;
    while i < p.n_used {
        json_parse_free(p.a[i as usize].take());
        i += 1;
    }
    // sqlite3DbFree(p->db, p): o dono do cache (o auxdata) libera o objeto.
}


// ---- part_001.rs ----

/// Contexto da função de uma JsonString. No C `pCtx` pode ser NULL; aqui os
/// chamadores que o usam sempre o preencheram antes (`json_string_init`).
pub fn json_string_ctx(p: &JsonString) -> Sqlite3ContextRef {
    p.p_ctx.clone().expect("JsonString sem contexto de função")
}

// Modelo adotado nesta parte (ver CONVENTIONS.md):
// - `JsonString.z_buf` é um `Vec<u8>` cujo comprimento é sempre >= `n_alloc`;
//   o estado "estático" (`b_static`) copia `z_space` para ele.
// - `JsonParse` compartilhado vive atrás de `JsonParseRef = Rc<RefCell<JsonParse>>`;
//   a contagem manual `n_jp_ref` continua existindo, como no C.
// - O cache fica no auxdata do contexto como `Rc<dyn Any>` (o `Rc<RefCell<JsonCache>>`).

/// Recupera o cache de JSON guardado no auxdata do contexto, se existir.
fn json_cache_get_auxdata(ctx: &sqlite3_context) -> Option<Rc<RefCell<JsonCache>>> {
    match api::get_auxdata(ctx, JSON_CACHE_ID) {
        Some(any) => any.downcast::<RefCell<JsonCache>>().ok(),
        None => None,
    }
}

/// Destrutor genérico registrado no auxdata do contexto.
fn json_cache_delete_generic(p: Rc<dyn std::any::Any>) {
    if let Ok(cache) = p.downcast::<RefCell<JsonCache>>() {
        json_cache_delete(&mut cache.borrow_mut());
    }
}

/// Insere uma nova entrada no cache. Se o cache estiver cheio, expulsa a
/// entrada menos recentemente usada. Devolve SQLITE_OK em caso de sucesso ou
/// um código de resultado em caso contrário.
///
/// As entradas do cache ficam em ordem de idade, a mais antiga primeiro.
fn json_cache_insert(ctx: &mut sqlite3_context, p_parse: &JsonParseRef) -> i32 {
    {
        let pp = p_parse.borrow();
        debug_assert!(!pp.z_json.is_empty());
        debug_assert!(pp.b_json_is_rc_str != 0);
        debug_assert!(pp.delta == 0);
    }
    let mut p = json_cache_get_auxdata(ctx);
    if p.is_none() {
        let db = api::context_db_handle(ctx);
        // sqlite3DbMallocZero: a alocação em Rust não falha, então o ramo
        // `p==0 -> SQLITE_NOMEM` desta etapa não existe.
        let novo = Rc::new(RefCell::new(JsonCache {
            db,
            n_used: 0,
            a: [None, None, None, None],
        }));
        api::set_auxdata(ctx, JSON_CACHE_ID, novo, Some(json_cache_delete_generic));
        p = json_cache_get_auxdata(ctx);
        if p.is_none() {
            return SQLITE_NOMEM;
        }
    }
    let p = p.unwrap();
    let mut c = p.borrow_mut();
    if c.n_used >= JSON_CACHE_SIZE as i32 {
        if let Some(velho) = c.a[0].take() {
            json_parse_free(Some(velho));
        }
        // memmove(p->a, &p->a[1], (JSON_CACHE_SIZE-1)*sizeof(p->a[0]))
        for k in 0..(JSON_CACHE_SIZE - 1) {
            c.a[k] = c.a[k + 1].take();
        }
        c.n_used = JSON_CACHE_SIZE as i32 - 1;
    }
    {
        let mut pp = p_parse.borrow_mut();
        debug_assert!(pp.n_blob_alloc > 0);
        pp.e_edit = 0;
        pp.n_jp_ref += 1;
        pp.b_read_only = 1;
    }
    let idx = c.n_used as usize;
    c.a[idx] = Some(p_parse.clone());
    c.n_used += 1;
    SQLITE_OK
}

/// Procura uma tradução em cache do texto JSON fornecido por `p_arg`.
/// Devolve o objeto JsonParse se achar, ou None se não achar.
///
/// Quando acha, a entrada correspondente passa a ser a mais recentemente
/// usada, se ainda não for.
///
/// O JsonParse devolvido ainda pertence ao cache e pode ser apagado a
/// qualquer momento. Quem quiser que ele permaneça precisa incrementar o
/// contador de referências.
fn json_cache_search(ctx: &sqlite3_context, p_arg: &MemRef) -> Option<JsonParseRef> {
    if api::value_type(p_arg) != SQLITE_TEXT {
        return None;
    }
    let z_json = match api::value_text(p_arg) {
        Some(z) => z,
        None => return None,
    };
    let n_json = api::value_bytes(p_arg);

    let p = match json_cache_get_auxdata(ctx) {
        Some(p) => p,
        None => return None,
    };
    let mut c = p.borrow_mut();
    let n_used = c.n_used as usize;
    let nj = n_json as usize;
    let mut i = 0usize;
    // Primeiro a comparação de identidade do ponteiro, depois a de conteúdo.
    while i < n_used {
        let ent = c.a[i].as_ref().unwrap().borrow();
        if std::ptr::eq(ent.z_json.as_ptr(), z_json.as_ptr()) {
            break;
        }
        i += 1;
    }
    if i >= n_used {
        i = 0;
        while i < n_used {
            let ent = c.a[i].as_ref().unwrap().borrow();
            if ent.n_json == n_json && ent.z_json[..nj] == z_json[..nj] {
                break;
            }
            i += 1;
        }
    }
    if i < n_used {
        if i < n_used - 1 {
            // Faz da entrada encontrada a mais recentemente usada
            c.a[i..n_used].rotate_left(1);
            i = n_used - 1;
        }
        let r = c.a[i].clone();
        debug_assert!(r.as_ref().unwrap().borrow().delta == 0);
        r
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Rotinas utilitárias para objetos JsonString
// ---------------------------------------------------------------------------

/// Transforma memória bruta não inicializada em um JsonString válido que
/// guarda uma string de comprimento zero.
fn json_string_zero(p: &mut JsonString) {
    p.z_buf = p.z_space.to_vec();
    p.n_alloc = p.z_space.len() as u64;
    p.n_used = 0;
    p.b_static = 1;
}

/// Inicializa o objeto JsonString.
fn json_string_init(p: &mut JsonString, p_ctx: Option<Sqlite3ContextRef>) {
    p.p_ctx = p_ctx;
    p.e_err = 0;
    json_string_zero(p);
}

/// Libera toda a memória alocada e devolve o objeto JsonString ao estado
/// inicial.
fn json_string_reset(p: &mut JsonString) {
    if p.b_static == 0 {
        rc_str_unref(&p.z_buf);
    }
    json_string_zero(p);
}

/// Informa uma condição de falta de memória (OOM).
fn json_string_oom(p: &mut JsonString) {
    p.e_err |= JSTRING_OOM;
    if let Some(ctx) = &p.p_ctx {
        api::result_error_nomem(&mut ctx.borrow_mut());
    }
    json_string_reset(p);
}

/// Aumenta `p.z_buf` para que caiba pelo menos N bytes a mais. Devolve zero
/// em caso de sucesso e diferente de zero em caso de OOM.
fn json_string_grow(p: &mut JsonString, n: u32) -> i32 {
    let n_total: u64 = if (n as u64) < p.n_alloc {
        p.n_alloc.wrapping_mul(2)
    } else {
        p.n_alloc.wrapping_add(n as u64).wrapping_add(10)
    };
    if p.b_static != 0 {
        if p.e_err != 0 {
            return 1;
        }
        let mut z_new = match rc_str_new(n_total) {
            Some(z) => z,
            None => {
                json_string_oom(p);
                return SQLITE_NOMEM;
            }
        };
        // O comprimento do Vec precisa ser sempre >= n_alloc.
        z_new.resize(n_total as usize, 0);
        let nu = p.n_used as usize;
        z_new[..nu].copy_from_slice(&p.z_buf[..nu]);
        p.z_buf = z_new;
        p.b_static = 0;
    } else {
        match rc_str_resize(&p.z_buf, n_total) {
            Some(mut z) => {
                z.resize(n_total as usize, 0);
                p.z_buf = z;
            }
            None => {
                p.e_err |= JSTRING_OOM;
                json_string_zero(p);
                return SQLITE_NOMEM;
            }
        }
    }
    p.n_alloc = n_total;
    SQLITE_OK
}

/// Acrescenta N bytes de `z_in` ao fim da string JsonString.
fn json_string_expand_and_append(p: &mut JsonString, z_in: &[u8], n: u32) {
    debug_assert!(n > 0);
    if json_string_grow(p, n) != 0 {
        return;
    }
    let nu = p.n_used as usize;
    let nn = n as usize;
    p.z_buf[nu..nu + nn].copy_from_slice(&z_in[..nn]);
    p.n_used += n as u64;
}

fn json_append_raw(p: &mut JsonString, z_in: &[u8], n: u32) {
    if n == 0 {
        return;
    }
    if (n as u64) + p.n_used >= p.n_alloc {
        json_string_expand_and_append(p, z_in, n);
    } else {
        let nu = p.n_used as usize;
        let nn = n as usize;
        p.z_buf[nu..nu + nn].copy_from_slice(&z_in[..nn]);
        p.n_used += n as u64;
    }
}

fn json_append_raw_nz(p: &mut JsonString, z_in: &[u8], n: u32) {
    debug_assert!(n > 0);
    if (n as u64) + p.n_used >= p.n_alloc {
        json_string_expand_and_append(p, z_in, n);
    } else {
        let nu = p.n_used as usize;
        let nn = n as usize;
        p.z_buf[nu..nu + nn].copy_from_slice(&z_in[..nn]);
        p.n_used += n as u64;
    }
}

/// Acrescenta texto formatado (sem passar de N bytes) ao JsonString.
fn json_printf(n: i32, p: &mut JsonString, z_format: &[u8], ap: &[Value]) {
    if (p.n_used + n as u64 >= p.n_alloc) && json_string_grow(p, n as u32) != 0 {
        return;
    }
    let nu = p.n_used as usize;
    api::vsnprintf(n, &mut p.z_buf[nu..], z_format, ap);
    // p->nUsed += (int)strlen(p->zBuf+p->nUsed)
    let len = p.z_buf[nu..]
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(p.z_buf.len() - nu);
    p.n_used += len as i32 as u64;
}

/// Acrescenta um único caractere.
fn json_append_char_expand(p: &mut JsonString, c: u8) {
    if json_string_grow(p, 1) != 0 {
        return;
    }
    let nu = p.n_used as usize;
    p.z_buf[nu] = c;
    p.n_used += 1;
}

fn json_append_char(p: &mut JsonString, c: u8) {
    if p.n_used >= p.n_alloc {
        json_append_char_expand(p, c);
    } else {
        let nu = p.n_used as usize;
        p.z_buf[nu] = c;
        p.n_used += 1;
    }
}

/// Remove um único caractere do fim da string.
fn json_string_trim_one_char(p: &mut JsonString) {
    if p.e_err == 0 {
        debug_assert!(p.n_used > 0);
        p.n_used -= 1;
    }
}

/// Garante que haja um terminador zero em `p.z_buf[]`.
///
/// Devolve verdadeiro em caso de sucesso e falso se um OOM impedir isso.
fn json_string_terminate(p: &mut JsonString) -> i32 {
    json_append_char(p, 0);
    json_string_trim_one_char(p);
    (p.e_err == 0) as i32
}

/// Acrescenta uma vírgula separadora ao buffer de saída, se o caractere
/// anterior não for '[' nem '{'.
fn json_append_separator(p: &mut JsonString) {
    if p.n_used == 0 {
        return;
    }
    let c = p.z_buf[(p.n_used - 1) as usize];
    if c == b'[' || c == b'{' {
        return;
    }
    json_append_char(p, b',');
}

/// `c` é um caractere de controle. Acrescenta a representação JSON canônica
/// desse caractere a `p`.
///
/// Esta rotina presume que o buffer de saída já foi aumentado o bastante
/// para caber a pior codificação mais o terminador nulo.
fn json_append_control_char(p: &mut JsonString, c: u8) {
    const A_SPECIAL: [u8; 32] = [
        0, 0, 0, 0, 0, 0, 0, 0, b'b', b't', b'n', 0, b'f', b'r', 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    debug_assert!(A_SPECIAL[8] == b'b');
    debug_assert!(A_SPECIAL[12] == b'f');
    debug_assert!(A_SPECIAL[10] == b'n');
    debug_assert!(A_SPECIAL[13] == b'r');
    debug_assert!(A_SPECIAL[9] == b't');
    debug_assert!((c as usize) < A_SPECIAL.len());
    debug_assert!(p.n_used + 7 <= p.n_alloc);
    let nu = p.n_used as usize;
    if A_SPECIAL[c as usize] != 0 {
        p.z_buf[nu] = b'\\';
        p.z_buf[nu + 1] = A_SPECIAL[c as usize];
        p.n_used += 2;
    } else {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        p.z_buf[nu] = b'\\';
        p.z_buf[nu + 1] = b'u';
        p.z_buf[nu + 2] = b'0';
        p.z_buf[nu + 3] = b'0';
        p.z_buf[nu + 4] = HEX[(c >> 4) as usize];
        p.z_buf[nu + 5] = HEX[(c & 0xf) as usize];
        p.n_used += 6;
    }
}

/// Acrescenta a string de N bytes em `z_in` ao fim da JsonString em
/// construção. Envolve a string em aspas duplas ("...") e escapa aspas
/// duplas e barras invertidas contidas nela.
///
/// Esta rotina é de alta frequência. O desenrolamento do laço de
/// `JSON_IS_OK[]` traz ganho de desempenho mensurável.
fn json_append_string(p: &mut JsonString, z_in: Option<&[u8]>, n: u32) {
    let mut z: &[u8] = match z_in {
        Some(z) => z,
        None => return,
    };
    let mut n = n;
    if ((n as u64) + p.n_used + 2 >= p.n_alloc) && json_string_grow(p, n.wrapping_add(2)) != 0 {
        return;
    }
    let nu = p.n_used as usize;
    p.z_buf[nu] = b'"';
    p.n_used += 1;
    loop {
        let mut k: u32 = 0;
        // O while a seguir é o equivalente desenrolado em 4 vias de
        //
        //     while( k<N && jsonIsOk[z[k]] ){ k++; }
        loop {
            if k.wrapping_add(3) >= n {
                while k < n && JSON_IS_OK[z[k as usize] as usize] != 0 {
                    k += 1;
                }
                break;
            }
            if JSON_IS_OK[z[k as usize] as usize] == 0 {
                break;
            }
            if JSON_IS_OK[z[(k + 1) as usize] as usize] == 0 {
                k += 1;
                break;
            }
            if JSON_IS_OK[z[(k + 2) as usize] as usize] == 0 {
                k += 2;
                break;
            }
            if JSON_IS_OK[z[(k + 3) as usize] as usize] == 0 {
                k += 3;
                break;
            } else {
                k += 4;
            }
        }
        if k >= n {
            if k > 0 {
                let nu = p.n_used as usize;
                p.z_buf[nu..nu + k as usize].copy_from_slice(&z[..k as usize]);
                p.n_used += k as u64;
            }
            break;
        }
        if k > 0 {
            let nu = p.n_used as usize;
            p.z_buf[nu..nu + k as usize].copy_from_slice(&z[..k as usize]);
            p.n_used += k as u64;
            z = &z[k as usize..];
            n -= k;
        }
        let c = z[0];
        if c == b'"' || c == b'\\' {
            if (p.n_used + n as u64 + 3 > p.n_alloc) && json_string_grow(p, n.wrapping_add(3)) != 0 {
                return;
            }
            let nu = p.n_used as usize;
            p.z_buf[nu] = b'\\';
            p.z_buf[nu + 1] = c;
            p.n_used += 2;
        } else if c == b'\'' {
            let nu = p.n_used as usize;
            p.z_buf[nu] = c;
            p.n_used += 1;
        } else {
            if (p.n_used + n as u64 + 7 > p.n_alloc) && json_string_grow(p, n.wrapping_add(7)) != 0 {
                return;
            }
            json_append_control_char(p, c);
        }
        z = &z[1..];
        n -= 1;
    }
    let nu = p.n_used as usize;
    p.z_buf[nu] = b'"';
    p.n_used += 1;
    debug_assert!(p.n_used < p.n_alloc);
}


// ---- part_002.rs ----

/// Lê o byte `i` de `z` como o C lê uma string terminada em NUL: além do fim
/// do slice o resultado é 0.
#[inline]
fn json_byte_at(z: &[u8], i: usize) -> u8 {
    z.get(i).copied().unwrap_or(0)
}

/// Acrescenta um sqlite3_value (por exemplo, um parâmetro de função) à string
/// JSON em construção em `p`.
fn json_append_sql_value(
    p: &mut JsonString,        // Acrescenta a esta string JSON
    p_value: &mut Mem,         // Valor a acrescentar
) {
    match api::value_type(p_value) {
        SQLITE_NULL => {
            json_append_raw_nz(p, b"null", 4);
        }
        SQLITE_FLOAT => {
            let x = api::value_double(p_value);
            json_printf(100, p, b"%!0.15g", &[Value::Double(x)]);
        }
        SQLITE_INTEGER => {
            let z: Vec<u8> = api::value_text(p_value).map(|s| s.to_vec()).unwrap_or_default();
            let n = api::value_bytes(p_value) as u32;
            json_append_raw(p, &z, n);
        }
        SQLITE_TEXT => {
            let z: Vec<u8> = api::value_text(p_value).map(|s| s.to_vec()).unwrap_or_default();
            let n = api::value_bytes(p_value) as u32;
            if api::value_subtype(p_value) == JSON_SUBTYPE as u32 {
                json_append_raw(p, &z, n);
            } else {
                json_append_string(p, Some(&z), n);
            }
        }
        _ => {
            if json_func_arg_might_be_binary(p_value) {
                let mut px = JsonParse::default();
                px.a_blob = api::value_blob(p_value).map(|s| s.to_vec()).unwrap_or_default();
                px.n_blob = api::value_bytes(p_value) as u32;
                json_translate_blob_to_text(&px, 0, p);
            } else if p.e_err == 0 {
                let ctx = json_string_ctx(p);
                api::result_error(&mut ctx.borrow_mut(), b"JSON cannot hold BLOB values", -1);
                p.e_err = JSTRING_ERR;
                json_string_reset(p);
            }
        }
    }
}

/// Faz do texto em `p` (provavelmente uma string JSON gerada) o resultado da
/// função SQL.
///
/// A JsonString é reiniciada.
///
/// Se `p_parse` e `ctx` são ambos presentes, a string SQL de `p` é carregada
/// no campo `z_json` do `p_parse` como uma RCStr e o `p_parse` entra no cache.
fn json_return_string(
    p: &mut JsonString,                              // String a retornar
    p_parse: Option<&JsonParseRef>,                  // Fonte JSONB ou None
    ctx: Option<&Sqlite3ContextRef>,                 // Onde guardar no cache
) {
    debug_assert!(p_parse.is_some() == ctx.is_some());
    if p.e_err == 0 {
        let p_ctx = json_string_ctx(p);
        let flags = ptr_to_int(api::user_data(&p_ctx.borrow())) as i32;
        if flags & (JSON_BLOB as i32) != 0 {
            json_return_string_as_blob(p);
        } else if p.b_static != 0 {
            api::result_text64(
                &mut p_ctx.borrow_mut(),
                &p.z_buf[..p.n_used as usize],
                p.n_used,
                SQLITE_TRANSIENT,
                SQLITE_UTF8,
            );
        } else if json_string_terminate(p) != 0 {
            if let (Some(p_parse), Some(ctx)) = (p_parse, ctx) {
                let go = {
                    let pp = p_parse.borrow();
                    pp.b_json_is_rc_str == 0 && pp.n_blob_alloc > 0
                };
                if go {
                    {
                        let mut pp = p_parse.borrow_mut();
                        pp.z_json = rc_str_ref(&mut p.z_buf).to_vec();
                        pp.n_json = p.n_used as i32;
                        pp.b_json_is_rc_str = 1;
                    }
                    let rc = json_cache_insert(&mut ctx.borrow_mut(), p_parse);
                    if rc == SQLITE_NOMEM {
                        api::result_error_nomem(&mut ctx.borrow_mut());
                        json_string_reset(p);
                        return;
                    }
                }
            }
            let z_ref = rc_str_ref(&mut p.z_buf).to_vec();
            api::result_text64(
                &mut p_ctx.borrow_mut(),
                &z_ref[..p.n_used as usize],
                p.n_used,
                rc_str_unref,
                SQLITE_UTF8,
            );
        } else {
            api::result_error_nomem(&mut p_ctx.borrow_mut());
        }
    } else if p.e_err & JSTRING_OOM != 0 {
        api::result_error_nomem(&mut json_string_ctx(p).borrow_mut());
    } else if p.e_err & JSTRING_MALFORMED != 0 {
        api::result_error(&mut json_string_ctx(p).borrow_mut(), b"malformed JSON", -1);
    }
    json_string_reset(p);
}

// ************************************************************************
// Rotinas utilitárias para objetos JsonParse
// ************************************************************************

/// Recupera toda a memória alocada por um objeto JsonParse, mas não apaga o
/// próprio objeto JsonParse.
fn json_parse_reset(p_parse: &mut JsonParse) {
    debug_assert!(p_parse.n_jp_ref <= 1);
    if p_parse.b_json_is_rc_str != 0 {
        rc_str_unref(&p_parse.z_json);
        p_parse.z_json = Vec::new();
        p_parse.n_json = 0;
        p_parse.b_json_is_rc_str = 0;
    }
    if p_parse.n_blob_alloc != 0 {
        p_parse.a_blob = Vec::new();
        p_parse.n_blob = 0;
        p_parse.n_blob_alloc = 0;
    }
}

/// Decrementa a contagem de referências do objeto JsonParse. Quando a
/// contagem chega a zero, libera o objeto.
fn json_parse_free(p_parse: Option<JsonParseRef>) {
    if let Some(p_parse) = p_parse {
        let mut pp = p_parse.borrow_mut();
        if pp.n_jp_ref > 1 {
            pp.n_jp_ref -= 1;
        } else {
            json_parse_reset(&mut pp);
        }
    }
}

// ************************************************************************
// Rotinas utilitárias para o analisador de texto JSON
// ************************************************************************

/// Traduz um único byte hexadecimal para inteiro.
/// Esta rotina só dá a resposta correta se `h` for de fato um caractere
/// hexadecimal válido: 0..9a..fA..F. Mas, diferente de sqlite3HexToInt(), ela
/// não dispara assert() se o dígito não for hexadecimal.
fn json_hex_to_int(h: i32) -> u8 {
    // SQLITE_ASCII está definido; SQLITE_EBCDIC não.
    let h = h.wrapping_add(9 * (1 & (h >> 6)));
    (h & 0xf) as u8
}

/// Converte uma string hexadecimal de 4 bytes em inteiro.
fn json_hex_to_int4(z: &[u8]) -> u32 {
    ((json_hex_to_int(z[0] as i8 as i32) as u32) << 12)
        .wrapping_add((json_hex_to_int(z[1] as i8 as i32) as u32) << 8)
        .wrapping_add((json_hex_to_int(z[2] as i8 as i32) as u32) << 4)
        .wrapping_add(json_hex_to_int(z[3] as i8 as i32) as u32)
}

/// Retorna verdadeiro se z[] começa com 2 (ou mais) dígitos hexadecimais.
fn json_is2_hex(z: &[u8]) -> i32 {
    (isxdigit(json_byte_at(z, 0)) && isxdigit(json_byte_at(z, 1))) as i32
}

/// Retorna verdadeiro se z[] começa com 4 (ou mais) dígitos hexadecimais.
fn json_is4_hex(z: &[u8]) -> i32 {
    (json_is2_hex(z) != 0 && json_is2_hex(z.get(2..).unwrap_or(&[])) != 0) as i32
}

/// Retorna o número de bytes de espaço em branco JSON5 no início da string de
/// entrada z[].
///
/// O espaço em branco JSON5 consiste em qualquer um dos caracteres a seguir:
///
///    Unicode  UTF-8         Nome
///    U+0009   09            tabulação horizontal
///    U+000a   0a            avanço de linha
///    U+000b   0b            tabulação vertical
///    U+000c   0c            avanço de página
///    U+000d   0d            retorno de carro
///    U+0020   20            espaço
///    U+00a0   c2 a0         espaço sem quebra
///    U+1680   e1 9a 80      marca de espaço ogham
///    U+2000   e2 80 80      en quad
///    U+2001   e2 80 81      em quad
///    U+2002   e2 80 82      en space
///    U+2003   e2 80 83      em space
///    U+2004   e2 80 84      espaço de um terço de em
///    U+2005   e2 80 85      espaço de um quarto de em
///    U+2006   e2 80 86      espaço de um sexto de em
///    U+2007   e2 80 87      espaço de figura
///    U+2008   e2 80 88      espaço de pontuação
///    U+2009   e2 80 89      espaço fino
///    U+200a   e2 80 8a      espaço de fio de cabelo
///    U+2028   e2 80 a8      separador de linha
///    U+2029   e2 80 a9      separador de parágrafo
///    U+202f   e2 80 af      espaço estreito sem quebra (NNBSP)
///    U+205f   e2 81 9f      espaço matemático médio (MMSP)
///    U+3000   e3 80 80      espaço ideográfico
///    U+FEFF   ef bb bf      marca de ordem de bytes
///
/// Além disso, comentários entre '/', '*' e '*', '/' e de '/', '/' até o fim
/// da linha também são considerados espaço em branco.
fn json5_whitespace(z_in: &[u8]) -> i32 {
    let mut n: usize = 0;
    let z = z_in;
    // Sai por "goto whitespace_done".
    'whitespace_done: loop {
        match json_byte_at(z, n) {
            0x09 | 0x0a | 0x0b | 0x0c | 0x0d | 0x20 => {
                n += 1;
            }
            b'/' => {
                if json_byte_at(z, n + 1) == b'*' && json_byte_at(z, n + 2) != 0 {
                    let mut j = n + 3;
                    while json_byte_at(z, j) != b'/' || json_byte_at(z, j - 1) != b'*' {
                        if json_byte_at(z, j) == 0 {
                            break 'whitespace_done;
                        }
                        j += 1;
                    }
                    n = j + 1;
                } else if json_byte_at(z, n + 1) == b'/' {
                    let mut j = n + 2;
                    loop {
                        let c = json_byte_at(z, j);
                        if c == 0 {
                            break;
                        }
                        if c == b'\n' || c == b'\r' {
                            break;
                        }
                        if 0xe2 == c
                            && 0x80 == json_byte_at(z, j + 1)
                            && (0xa8 == json_byte_at(z, j + 2) || 0xa9 == json_byte_at(z, j + 2))
                        {
                            j += 2;
                            break;
                        }
                        j += 1;
                    }
                    n = j;
                    if json_byte_at(z, n) != 0 {
                        n += 1;
                    }
                } else {
                    break 'whitespace_done;
                }
            }
            0xc2 => {
                if json_byte_at(z, n + 1) == 0xa0 {
                    n += 2;
                } else {
                    break 'whitespace_done;
                }
            }
            0xe1 => {
                if json_byte_at(z, n + 1) == 0x9a && json_byte_at(z, n + 2) == 0x80 {
                    n += 3;
                } else {
                    break 'whitespace_done;
                }
            }
            0xe2 => {
                if json_byte_at(z, n + 1) == 0x80 {
                    let c = json_byte_at(z, n + 2);
                    if c < 0x80 {
                        break 'whitespace_done;
                    }
                    if c <= 0x8a || c == 0xa8 || c == 0xa9 || c == 0xaf {
                        n += 3;
                        continue 'whitespace_done;
                    }
                } else if json_byte_at(z, n + 1) == 0x81 && json_byte_at(z, n + 2) == 0x9f {
                    n += 3;
                    continue 'whitespace_done;
                }
                break 'whitespace_done;
            }
            0xe3 => {
                if json_byte_at(z, n + 1) == 0x80 && json_byte_at(z, n + 2) == 0x80 {
                    n += 3;
                } else {
                    break 'whitespace_done;
                }
            }
            0xef => {
                if json_byte_at(z, n + 1) == 0xbb && json_byte_at(z, n + 2) == 0xbf {
                    n += 3;
                } else {
                    break 'whitespace_done;
                }
            }
            _ => {
                break 'whitespace_done;
            }
        }
    }
    n as i32
}

/// Literais de ponto flutuante extras permitidos em JSON.
pub struct NanInfName {
    pub c1: u8,
    pub c2: u8,
    pub n: u8,
    pub e_type: u8,
    pub n_repl: u8,
    pub z_match: &'static [u8],
    pub z_repl: &'static [u8],
}

pub const A_NAN_INF_NAME: [NanInfName; 5] = [
    NanInfName { c1: b'i', c2: b'I', n: 3, e_type: JSONB_FLOAT as u8, n_repl: 7, z_match: b"inf", z_repl: b"9.0e999" },
    NanInfName { c1: b'i', c2: b'I', n: 8, e_type: JSONB_FLOAT as u8, n_repl: 7, z_match: b"infinity", z_repl: b"9.0e999" },
    NanInfName { c1: b'n', c2: b'N', n: 3, e_type: JSONB_NULL as u8, n_repl: 4, z_match: b"NaN", z_repl: b"null" },
    NanInfName { c1: b'q', c2: b'Q', n: 4, e_type: JSONB_NULL as u8, n_repl: 4, z_match: b"QNaN", z_repl: b"null" },
    NanInfName { c1: b's', c2: b'S', n: 4, e_type: JSONB_NULL as u8, n_repl: 4, z_match: b"SNaN", z_repl: b"null" },
];

/// Informa o número errado de argumentos para json_insert(), json_replace() ou
/// json_set().
fn json_wrong_num_args(p_ctx: &mut sqlite3_context, z_func_name: &[u8]) {
    let mut z_msg: Vec<u8> = Vec::new();
    z_msg.extend_from_slice(b"json_");
    z_msg.extend_from_slice(z_func_name);
    z_msg.extend_from_slice(b"() needs an odd number of arguments");
    api::result_error(p_ctx, &z_msg, -1);
}

// **************************************************************************
// Rotinas utilitárias para a representação binária BLOB do JSON
// **************************************************************************

/// Expande `p_parse.a_blob` para que guarde pelo menos N bytes.
///
/// Retorna o número de erros.
fn json_blob_expand(p_parse: &mut JsonParse, n: u32) -> i32 {
    debug_assert!(n > p_parse.n_blob_alloc);
    let mut t: u32 = if p_parse.n_blob_alloc == 0 {
        100
    } else {
        p_parse.n_blob_alloc.wrapping_mul(2)
    };
    if t < n {
        t = n.wrapping_add(100);
    }
    let cur = p_parse.a_blob.len();
    if (t as usize) > cur {
        if p_parse.a_blob.try_reserve_exact(t as usize - cur).is_err() {
            p_parse.oom = 1;
            return 1;
        }
    }
    p_parse.a_blob.resize(t as usize, 0);
    p_parse.n_blob_alloc = t;
    0
}


// ---- part_003.rs ----

/// Se `a_blob` não é editável (vem de `sqlite3_value_blob()`, o que se
/// reconhece por `n_blob_alloc==0` e `n_blob>0`), faz uma cópia própria para
/// poder editá-lo. Devolve verdadeiro (1) se deu certo e falso (0) em OOM.
pub fn json_blob_make_editable(p_parse: &mut JsonParse, n_extra: u32) -> i32 {
    if p_parse.oom != 0 {
        return 0;
    }
    if p_parse.n_blob_alloc > 0 {
        return 1;
    }
    let a_old = std::mem::take(&mut p_parse.a_blob);
    let n_size = p_parse.n_blob.wrapping_add(n_extra);
    if json_blob_expand(p_parse, n_size) != 0 {
        return 0;
    }
    let n = p_parse.n_blob as usize;
    p_parse.a_blob[..n].copy_from_slice(&a_old[..n]);
    1
}

/// Expande `a_blob` e acrescenta um byte.
pub fn json_blob_expand_and_append_one_byte(p_parse: &mut JsonParse, c: u8) {
    json_blob_expand(p_parse, p_parse.n_blob.wrapping_add(1));
    if p_parse.oom == 0 {
        let i = p_parse.n_blob as usize;
        p_parse.a_blob[i] = c;
        p_parse.n_blob += 1;
    }
}

/// Acrescenta um único byte.
pub fn json_blob_append_one_byte(p_parse: &mut JsonParse, c: u8) {
    if p_parse.n_blob >= p_parse.n_blob_alloc {
        json_blob_expand_and_append_one_byte(p_parse, c);
    } else {
        let i = p_parse.n_blob as usize;
        p_parse.a_blob[i] = c;
        p_parse.n_blob += 1;
    }
}

/// Versão lenta de `json_blob_append_node()`, que antes redimensiona
/// `a_blob`.
pub fn json_blob_expand_and_append_node(
    p_parse: &mut JsonParse,
    e_type: u8,
    sz_payload: u32,
    a_payload: Option<&[u8]>,
) {
    let n = p_parse
        .n_blob
        .wrapping_add(sz_payload)
        .wrapping_add(9);
    if json_blob_expand(p_parse, n) != 0 {
        return;
    }
    json_blob_append_node(p_parse, e_type, sz_payload, a_payload);
}

/// Acrescenta o byte de tipo do nó junto com o tamanho do payload e,
/// possivelmente, o próprio payload.
///
/// Se `a_payload` é `Some`, o payload também é acrescentado. Se é `None`,
/// `a_blob` é redimensionado (se preciso) para caber o payload, mas o payload
/// não é acrescentado e `n_blob` fica apontando para onde o primeiro byte do
/// payload vai ficar.
pub fn json_blob_append_node(
    p_parse: &mut JsonParse,
    e_type: u8,
    sz_payload: u32,
    a_payload: Option<&[u8]>,
) {
    if p_parse
        .n_blob
        .wrapping_add(sz_payload)
        .wrapping_add(9)
        > p_parse.n_blob_alloc
    {
        json_blob_expand_and_append_node(p_parse, e_type, sz_payload, a_payload);
        return;
    }
    let base = p_parse.n_blob as usize;
    if sz_payload <= 11 {
        p_parse.a_blob[base] = e_type | ((sz_payload << 4) as u8);
        p_parse.n_blob += 1;
    } else if sz_payload <= 0xff {
        p_parse.a_blob[base] = e_type | 0xc0;
        p_parse.a_blob[base + 1] = (sz_payload & 0xff) as u8;
        p_parse.n_blob += 2;
    } else if sz_payload <= 0xffff {
        p_parse.a_blob[base] = e_type | 0xd0;
        p_parse.a_blob[base + 1] = ((sz_payload >> 8) & 0xff) as u8;
        p_parse.a_blob[base + 2] = (sz_payload & 0xff) as u8;
        p_parse.n_blob += 3;
    } else {
        p_parse.a_blob[base] = e_type | 0xe0;
        p_parse.a_blob[base + 1] = ((sz_payload >> 24) & 0xff) as u8;
        p_parse.a_blob[base + 2] = ((sz_payload >> 16) & 0xff) as u8;
        p_parse.a_blob[base + 3] = ((sz_payload >> 8) & 0xff) as u8;
        p_parse.a_blob[base + 4] = (sz_payload & 0xff) as u8;
        p_parse.n_blob += 5;
    }
    if let Some(payload) = a_payload {
        p_parse.n_blob = p_parse.n_blob.wrapping_add(sz_payload);
        let end = p_parse.n_blob as usize;
        let sz = sz_payload as usize;
        p_parse.a_blob[end - sz..end].copy_from_slice(&payload[..sz]);
    }
}

/// Muda o tamanho do payload do nó no índice `i` para `sz_payload`.
/// Devolve a variação de tamanho (delta) do blob.
pub fn json_blob_change_payload_size(
    p_parse: &mut JsonParse,
    i: u32,
    sz_payload: u32,
) -> i32 {
    if p_parse.oom != 0 {
        return 0;
    }
    let i = i as usize;
    let sz_type: u8 = p_parse.a_blob[i] >> 4;
    let n_extra: u8 = if sz_type <= 11 {
        0
    } else if sz_type == 12 {
        1
    } else if sz_type == 13 {
        2
    } else {
        4
    };
    let n_needed: u8 = if sz_payload <= 11 {
        0
    } else if sz_payload <= 0xff {
        1
    } else if sz_payload <= 0xffff {
        2
    } else {
        4
    };
    let delta: i32 = n_needed as i32 - n_extra as i32;
    if delta != 0 {
        let new_size: u32 = p_parse.n_blob.wrapping_add(delta as u32);
        if delta > 0 {
            if new_size > p_parse.n_blob_alloc && json_blob_expand(p_parse, new_size) != 0 {
                return 0; /* Erro de OOM. O estado fica registrado em p_parse.oom. */
            }
            let len = p_parse.n_blob as usize - (i + 1);
            let dst = i + 1 + delta as usize;
            p_parse.a_blob.copy_within(i + 1..i + 1 + len, dst);
        } else {
            let src = i + 1 + (-delta) as usize;
            let len = p_parse.n_blob as usize - src;
            p_parse.a_blob.copy_within(src..src + len, i + 1);
        }
        p_parse.n_blob = new_size;
    }
    let a = &mut p_parse.a_blob[i..];
    if n_needed == 0 {
        a[0] = (a[0] & 0x0f) | ((sz_payload << 4) as u8);
    } else if n_needed == 1 {
        a[0] = (a[0] & 0x0f) | 0xc0;
        a[1] = (sz_payload & 0xff) as u8;
    } else if n_needed == 2 {
        a[0] = (a[0] & 0x0f) | 0xd0;
        a[1] = ((sz_payload >> 8) & 0xff) as u8;
        a[2] = (sz_payload & 0xff) as u8;
    } else {
        a[0] = (a[0] & 0x0f) | 0xe0;
        a[1] = ((sz_payload >> 24) & 0xff) as u8;
        a[2] = ((sz_payload >> 16) & 0xff) as u8;
        a[3] = ((sz_payload >> 8) & 0xff) as u8;
        a[4] = (sz_payload & 0xff) as u8;
    }
    delta
}

/// Se `z[0]` é 'u' e é seguido por exatamente 4 caracteres hexadecimais,
/// põe `*p_op` em `JSONB_TEXTJ` e devolve verdadeiro. Senão não muda
/// `*p_op` e devolve falso.
pub fn json_is4_hex_b(z: &[u8], p_op: &mut i32) -> i32 {
    if z.first().copied() != Some(b'u') {
        return 0;
    }
    if json_is4_hex(&z[1..]) == 0 {
        return 0;
    }
    *p_op = JSONB_TEXTJ as i32;
    1
}

/// Verifica a validade de um único elemento do JSONB em `p_parse`.
///
/// O elemento começa no deslocamento `i` e termina no último byte antes de
/// `i_end`.
///
/// Devolve 0 se tudo está correto. Se há problema, devolve o deslocamento do
/// erro contado a partir de 1 (erro no deslocamento 0 devolve 1).
pub fn jsonb_validity_check(
    p_parse: &JsonParse, /* JSONB de entrada. Só a_blob e n_blob são usados */
    i: u32,              /* Início do elemento em a_blob[i] */
    i_end: u32,          /* Um a mais que o último byte do elemento */
    i_depth: u32,        /* Profundidade de aninhamento atual */
) -> u32 {
    let mut n: u32;
    let mut sz: u32;
    let mut j: u32;
    let mut k: u32;
    let z: &[u8] = &p_parse.a_blob;
    let mut x: u8;
    if i_depth > JSON_MAX_DEPTH as u32 {
        return i + 1;
    }
    sz = 0;
    n = jsonb_payload_size(p_parse, i, &mut sz);
    if n == 0 {
        return i + 1; /* Verificado pelo chamador */
    }
    if i.wrapping_add(n).wrapping_add(sz) != i_end {
        return i + 1; /* Verificado pelo chamador */
    }
    x = z[i as usize] & 0x0f;
    match x {
        JSONB_NULL | JSONB_TRUE | JSONB_FALSE => {
            return if n + sz == 1 { 0 } else { i + 1 };
        }
        JSONB_INT => {
            if sz < 1 {
                return i + 1;
            }
            j = i + n;
            if z[j as usize] == b'-' {
                j += 1;
                if sz < 2 {
                    return i + 1;
                }
            }
            k = i + n + sz;
            while j < k {
                if isdigit(z[j as usize]) {
                    j += 1;
                } else {
                    return j + 1;
                }
            }
            return 0;
        }
        JSONB_INT5 => {
            if sz < 3 {
                return i + 1;
            }
            j = i + n;
            if z[j as usize] == b'-' {
                if sz < 4 {
                    return i + 1;
                }
                j += 1;
            }
            if z[j as usize] != b'0' {
                return i + 1;
            }
            if z[j as usize + 1] != b'x' && z[j as usize + 1] != b'X' {
                return j + 2;
            }
            j += 2;
            k = i + n + sz;
            while j < k {
                if isxdigit(z[j as usize]) {
                    j += 1;
                } else {
                    return j + 1;
                }
            }
            return 0;
        }
        JSONB_FLOAT | JSONB_FLOAT5 => {
            let mut seen: u8 = 0; /* 0: inicial.  1: '.' visto  2: 'e' visto */
            if sz < 2 {
                return i + 1;
            }
            j = i + n;
            k = j + sz;
            if z[j as usize] == b'-' {
                j += 1;
                if sz < 3 {
                    return i + 1;
                }
            }
            if z[j as usize] == b'.' {
                if x == JSONB_FLOAT {
                    return j + 1;
                }
                if !isdigit(z[j as usize + 1]) {
                    return j + 1;
                }
                j += 2;
                seen = 1;
            } else if z[j as usize] == b'0' && x == JSONB_FLOAT {
                if j + 3 > k {
                    return j + 1;
                }
                if z[j as usize + 1] != b'.'
                    && z[j as usize + 1] != b'e'
                    && z[j as usize + 1] != b'E'
                {
                    return j + 1;
                }
                j += 1;
            }
            while j < k {
                if isdigit(z[j as usize]) {
                    j += 1;
                    continue;
                }
                if z[j as usize] == b'.' {
                    if seen > 0 {
                        return j + 1;
                    }
                    if x == JSONB_FLOAT && (j == k - 1 || !isdigit(z[j as usize + 1])) {
                        return j + 1;
                    }
                    seen = 1;
                    j += 1;
                    continue;
                }
                if z[j as usize] == b'e' || z[j as usize] == b'E' {
                    if seen == 2 {
                        return j + 1;
                    }
                    if j == k - 1 {
                        return j + 1;
                    }
                    if z[j as usize + 1] == b'+' || z[j as usize + 1] == b'-' {
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
            return 0;
        }
        JSONB_TEXT => {
            j = i + n;
            k = j + sz;
            while j < k {
                if JSON_IS_OK[z[j as usize] as usize] == 0 && z[j as usize] != b'\'' {
                    return j + 1;
                }
                j += 1;
            }
            return 0;
        }
        JSONB_TEXTJ | JSONB_TEXT5 => {
            j = i + n;
            k = j + sz;
            while j < k {
                if JSON_IS_OK[z[j as usize] as usize] == 0 && z[j as usize] != b'\'' {
                    if z[j as usize] == b'"' {
                        if x == JSONB_TEXTJ {
                            return j + 1;
                        }
                    } else if z[j as usize] <= 0x1f {
                        /* Caracteres de controle em literais string de JSON5 são aceitos */
                        if x == JSONB_TEXTJ {
                            return j + 1;
                        }
                    } else if z[j as usize] != b'\\' || j + 1 >= k {
                        return j + 1;
                    } else if b"\"\\/bfnrt\0".contains(&z[j as usize + 1]) {
                        j += 1;
                    } else if z[j as usize + 1] == b'u' {
                        if j + 5 >= k {
                            return j + 1;
                        }
                        if json_is4_hex(&z[j as usize + 2..]) == 0 {
                            return j + 1;
                        }
                        j += 1;
                    } else if x != JSONB_TEXT5 {
                        return j + 1;
                    } else {
                        let mut c: u32 = 0;
                        let sz_c = json_unescape_one_char(&z[j as usize..], k - j, &mut c);
                        if c == JSON_INVALID_CHAR as u32 {
                            return j + 1;
                        }
                        j += sz_c - 1;
                    }
                }
                j += 1;
            }
            return 0;
        }
        JSONB_TEXTRAW => {
            return 0;
        }
        JSONB_ARRAY => {
            let mut sub: u32;
            j = i + n;
            k = j + sz;
            while j < k {
                sz = 0;
                n = jsonb_payload_size(p_parse, j, &mut sz);
                if n == 0 {
                    return j + 1;
                }
                if j.wrapping_add(n).wrapping_add(sz) > k {
                    return j + 1;
                }
                sub = jsonb_validity_check(p_parse, j, j.wrapping_add(n).wrapping_add(sz), i_depth + 1);
                if sub != 0 {
                    return sub;
                }
                j = j.wrapping_add(n).wrapping_add(sz);
            }
            return 0;
        }
        JSONB_OBJECT => {
            let mut cnt: u32 = 0;
            let mut sub: u32;
            j = i + n;
            k = j + sz;
            while j < k {
                sz = 0;
                n = jsonb_payload_size(p_parse, j, &mut sz);
                if n == 0 {
                    return j + 1;
                }
                if j.wrapping_add(n).wrapping_add(sz) > k {
                    return j + 1;
                }
                if (cnt & 1) == 0 {
                    x = z[j as usize] & 0x0f;
                    if x < JSONB_TEXT || x > JSONB_TEXTRAW {
                        return j + 1;
                    }
                }
                sub = jsonb_validity_check(p_parse, j, j.wrapping_add(n).wrapping_add(sz), i_depth + 1);
                if sub != 0 {
                    return sub;
                }
                cnt += 1;
                j = j.wrapping_add(n).wrapping_add(sz);
            }
            if (cnt & 1) != 0 {
                return j + 1;
            }
            return 0;
        }
        _ => {
            return i + 1;
        }
    }
}


// ---- part_004.rs ----

/// Traduz um único elemento do texto JSON em `p_parse.z_json[i]` para a
/// representação binária JSONB equivalente. Acrescenta a tradução em
/// `p_parse.a_blob[]` a partir de `p_parse.n_blob`. O tamanho de `a_blob[]`
/// aumenta conforme a necessidade.
///
/// Devolve o índice do primeiro caractere depois do fim do elemento
/// interpretado, ou um dos códigos especiais:
///
/// ```text
///      0    fim da entrada
///     -1    erro de sintaxe ou falta de memória
///     -2    '}' visto   \
///     -3    ']' visto    \___  nesses retornos, p_parse.i_err recebe
///     -4    ',' visto    /     o índice em z_json[] do caractere visto
///     -5    ':' visto   /
/// ```
///
/// O texto fica fora de `p_parse` durante a recursão (o C lê `pParse->zJson`
/// por um ponteiro local `z`): esta função o retira, chama a versão que
/// recebe `z` emprestado e o devolve ao objeto.
pub fn json_translate_text_to_blob(p_parse: &mut JsonParse, i: u32) -> i32 {
    let z = std::mem::take(&mut p_parse.z_json);
    let x = json_translate_text_to_blob_z(p_parse, &z, i);
    p_parse.z_json = z;
    x
}

/// Corpo recursivo de `json_translate_text_to_blob()`. O texto `z` é lido como
/// uma string do C terminada em NUL: além do fim do slice o byte é 0.
fn json_translate_text_to_blob_z(p_parse: &mut JsonParse, z: &[u8], mut i: u32) -> i32 {
    // z[k] do C e &z[k] do C.
    let at = move |k: u32| -> u8 { z.get(k as usize).copied().unwrap_or(0) };
    let tail = move |k: u32| z.get(k as usize..).unwrap_or(&[]);
    // (u32)strspn(s, jsonSpaces)
    let spaces_span = |s: &[u8]| -> u32 {
        s.iter().take_while(|b| JSON_SPACES.contains(b)).count() as u32
    };

    // json_parse_restart:
    loop {
        match at(i) {
            b'{' => {
                // Interpreta um objeto.
                let i_this: u32 = p_parse.n_blob;
                json_blob_append_node(
                    p_parse,
                    JSONB_OBJECT,
                    (p_parse.n_json as u32).wrapping_sub(i),
                    None,
                );
                p_parse.i_depth += 1;
                if p_parse.i_depth > JSON_MAX_DEPTH {
                    p_parse.i_err = i;
                    return -1;
                }
                let i_start: u32 = p_parse.n_blob;
                let mut j: u32 = i + 1;
                loop {
                    let i_blob: u32 = p_parse.n_blob;
                    let mut x: i32 = json_translate_text_to_blob_z(p_parse, z, j);
                    if x <= 0 {
                        if x == -2 {
                            j = p_parse.i_err;
                            if p_parse.n_blob != i_start {
                                p_parse.has_nonstd = 1;
                            }
                            break;
                        }
                        j += json5_whitespace(tail(j)) as u32;
                        let mut op: i32 = JSONB_TEXT as i32;
                        if json_id1(at(j))
                            || (at(j) == b'\\' && json_is4_hex_b(tail(j + 1), &mut op) != 0)
                        {
                            let mut k: u32 = j + 1;
                            while (json_id2(at(k)) && json5_whitespace(tail(k)) == 0)
                                || (at(k) == b'\\' && json_is4_hex_b(tail(k + 1), &mut op) != 0)
                            {
                                k += 1;
                            }
                            debug_assert!(i_blob == p_parse.n_blob);
                            json_blob_append_node(
                                p_parse,
                                op as u8,
                                k - j,
                                Some(&z[j as usize..k as usize]),
                            );
                            p_parse.has_nonstd = 1;
                            x = k as i32;
                        } else {
                            if x != -1 {
                                p_parse.i_err = j;
                            }
                            return -1;
                        }
                    }
                    if p_parse.oom != 0 {
                        return -1;
                    }
                    let t: u8 = p_parse.a_blob[i_blob as usize] & 0x0f;
                    if t < JSONB_TEXT || t > JSONB_TEXTRAW {
                        p_parse.i_err = j;
                        return -1;
                    }
                    j = x as u32;
                    // Vira verdadeiro ao saltar para parse_object_value.
                    let mut goto_object_value = false;
                    if at(j) == b':' {
                        j += 1;
                    } else {
                        if json_isspace(at(j)) != 0 {
                            // strspn() não ajuda aqui
                            loop {
                                j += 1;
                                if json_isspace(at(j)) == 0 {
                                    break;
                                }
                            }
                            if at(j) == b':' {
                                j += 1;
                                goto_object_value = true;
                            }
                        }
                        if !goto_object_value {
                            x = json_translate_text_to_blob_z(p_parse, z, j);
                            if x != -5 {
                                if x != -1 {
                                    p_parse.i_err = j;
                                }
                                return -1;
                            }
                            j = p_parse.i_err + 1;
                        }
                    }
                    // parse_object_value:
                    x = json_translate_text_to_blob_z(p_parse, z, j);
                    if x <= 0 {
                        if x != -1 {
                            p_parse.i_err = j;
                        }
                        return -1;
                    }
                    j = x as u32;
                    if at(j) == b',' {
                        j += 1;
                        continue;
                    } else if at(j) == b'}' {
                        break;
                    } else {
                        if json_isspace(at(j)) != 0 {
                            j += 1 + spaces_span(tail(j + 1));
                            if at(j) == b',' {
                                j += 1;
                                continue;
                            } else if at(j) == b'}' {
                                break;
                            }
                        }
                        x = json_translate_text_to_blob_z(p_parse, z, j);
                        if x == -4 {
                            j = p_parse.i_err;
                            j += 1;
                            continue;
                        }
                        if x == -2 {
                            j = p_parse.i_err;
                            break;
                        }
                    }
                    p_parse.i_err = j;
                    return -1;
                }
                json_blob_change_payload_size(p_parse, i_this, p_parse.n_blob - i_start);
                p_parse.i_depth -= 1;
                return (j + 1) as i32;
            }
            b'[' => {
                // Interpreta um array.
                let i_this: u32 = p_parse.n_blob;
                debug_assert!(i <= p_parse.n_json as u32);
                json_blob_append_node(
                    p_parse,
                    JSONB_ARRAY,
                    (p_parse.n_json as u32).wrapping_sub(i),
                    None,
                );
                let i_start: u32 = p_parse.n_blob;
                if p_parse.oom != 0 {
                    return -1;
                }
                p_parse.i_depth += 1;
                if p_parse.i_depth > JSON_MAX_DEPTH {
                    p_parse.i_err = i;
                    return -1;
                }
                let mut j: u32 = i + 1;
                loop {
                    let mut x: i32 = json_translate_text_to_blob_z(p_parse, z, j);
                    if x <= 0 {
                        if x == -3 {
                            j = p_parse.i_err;
                            if p_parse.n_blob != i_start {
                                p_parse.has_nonstd = 1;
                            }
                            break;
                        }
                        if x != -1 {
                            p_parse.i_err = j;
                        }
                        return -1;
                    }
                    j = x as u32;
                    if at(j) == b',' {
                        j += 1;
                        continue;
                    } else if at(j) == b']' {
                        break;
                    } else {
                        if json_isspace(at(j)) != 0 {
                            j += 1 + spaces_span(tail(j + 1));
                            if at(j) == b',' {
                                j += 1;
                                continue;
                            } else if at(j) == b']' {
                                break;
                            }
                        }
                        x = json_translate_text_to_blob_z(p_parse, z, j);
                        if x == -4 {
                            j = p_parse.i_err;
                            j += 1;
                            continue;
                        }
                        if x == -3 {
                            j = p_parse.i_err;
                            break;
                        }
                    }
                    p_parse.i_err = j;
                    return -1;
                }
                json_blob_change_payload_size(p_parse, i_this, p_parse.n_blob - i_start);
                p_parse.i_depth -= 1;
                return (j + 1) as i32;
            }
            b'\'' | b'"' => {
                // Interpreta uma string. O apóstrofo é extensão do JSON-5.
                if at(i) == b'\'' {
                    p_parse.has_nonstd = 1;
                }
                let mut opcode: u8 = JSONB_TEXT;
                // parse_string:
                let c_delim: u8 = at(i);
                let mut j: u32 = i + 1;
                loop {
                    if JSON_IS_OK[at(j) as usize] != 0 {
                        if JSON_IS_OK[at(j + 1) as usize] == 0 {
                            j += 1;
                        } else if JSON_IS_OK[at(j + 2) as usize] == 0 {
                            j += 2;
                        } else {
                            j += 3;
                            continue;
                        }
                    }
                    let mut c: u8 = at(j);
                    if c == c_delim {
                        break;
                    } else if c == b'\\' {
                        j += 1;
                        c = at(j);
                        if c == b'"'
                            || c == b'\\'
                            || c == b'/'
                            || c == b'b'
                            || c == b'f'
                            || c == b'n'
                            || c == b'r'
                            || c == b't'
                            || (c == b'u' && json_is4_hex(tail(j + 1)) != 0)
                        {
                            if opcode == JSONB_TEXT {
                                opcode = JSONB_TEXTJ;
                            }
                        } else if c == b'\''
                            || c == b'0'
                            || c == b'v'
                            || c == b'\n'
                            || (0xe2 == c
                                && 0x80 == at(j + 1)
                                && (0xa8 == at(j + 2) || 0xa9 == at(j + 2)))
                            || (c == b'x' && json_is2_hex(tail(j + 1)) != 0)
                        {
                            opcode = JSONB_TEXT5;
                            p_parse.has_nonstd = 1;
                        } else if c == b'\r' {
                            if at(j + 1) == b'\n' {
                                j += 1;
                            }
                            opcode = JSONB_TEXT5;
                            p_parse.has_nonstd = 1;
                        } else {
                            p_parse.i_err = j;
                            return -1;
                        }
                    } else if c <= 0x1f {
                        if c == 0 {
                            p_parse.i_err = j;
                            return -1;
                        }
                        // Caracteres de controle não são permitidos em literais
                        // de string do JSON canônico, mas são no JSON-5.
                        opcode = JSONB_TEXT5;
                        p_parse.has_nonstd = 1;
                    } else if c == b'"' {
                        opcode = JSONB_TEXT5;
                    }
                    j += 1;
                }
                json_blob_append_node(
                    p_parse,
                    opcode,
                    j - 1 - i,
                    Some(&z[i as usize + 1..j as usize]),
                );
                return (j + 1) as i32;
            }
            b't' => {
                if tail(i).starts_with(b"true") && !isalnum(at(i + 4)) {
                    json_blob_append_one_byte(p_parse, JSONB_TRUE);
                    return (i + 4) as i32;
                }
                p_parse.i_err = i;
                return -1;
            }
            b'f' => {
                if tail(i).starts_with(b"false") && !isalnum(at(i + 5)) {
                    json_blob_append_one_byte(p_parse, JSONB_FALSE);
                    return (i + 5) as i32;
                }
                p_parse.i_err = i;
                return -1;
            }
            b'+' | b'.' | b'-' | b'0'..=b'9' => {
                // Interpreta um número.
                let mut t: u8; // bit 0x01: JSON5. Bit 0x02: FLOAT
                let mut seen_e: u8 = 0;
                let mut j: u32 = 0;
                // Etapa: 0 = parse_number, 1 = parse_number_2, 2 = parse_number_finish.
                let mut stage: u8 = 0;
                match at(i) {
                    b'+' => {
                        p_parse.has_nonstd = 1;
                        t = 0x00;
                    }
                    b'.' => {
                        if isdigit(at(i + 1)) {
                            p_parse.has_nonstd = 1;
                            t = 0x03;
                            seen_e = 0;
                            stage = 1;
                        } else {
                            p_parse.i_err = i;
                            return -1;
                        }
                    }
                    _ => {
                        t = 0x00;
                    }
                }
                if stage == 0 {
                    // parse_number:
                    seen_e = 0;
                    let c: u8 = at(i);
                    if c <= b'0' {
                        if c == b'0' {
                            if (at(i + 1) == b'x' || at(i + 1) == b'X') && isxdigit(at(i + 2)) {
                                debug_assert!(t == 0x00);
                                p_parse.has_nonstd = 1;
                                t = 0x01;
                                j = i + 3;
                                while isxdigit(at(j)) {
                                    j += 1;
                                }
                                stage = 2;
                            } else if isdigit(at(i + 1)) {
                                p_parse.i_err = i + 1;
                                return -1;
                            }
                        } else if !isdigit(at(i + 1)) {
                            // O JSON5 permite "+Infinity" e "-Infinity" exatamente
                            // assim. O SQLite também aceita em qualquer caixa e
                            // aceita "+inf" e "-inf".
                            if (at(i + 1) == b'I' || at(i + 1) == b'i')
                                && str_n_i_cmp(tail(i + 1), b"inf", 3) == 0
                            {
                                p_parse.has_nonstd = 1;
                                if at(i) == b'-' {
                                    json_blob_append_node(p_parse, JSONB_FLOAT, 6, Some(b"-9e999"));
                                } else {
                                    json_blob_append_node(p_parse, JSONB_FLOAT, 5, Some(b"9e999"));
                                }
                                return (i + if str_n_i_cmp(tail(i + 4), b"inity", 5) == 0 {
                                    9
                                } else {
                                    4
                                }) as i32;
                            }
                            if at(i + 1) == b'.' {
                                p_parse.has_nonstd = 1;
                                t |= 0x01;
                                stage = 1; // goto parse_number_2
                            } else {
                                p_parse.i_err = i;
                                return -1;
                            }
                        } else if at(i + 1) == b'0' {
                            if isdigit(at(i + 2)) {
                                p_parse.i_err = i + 1;
                                return -1;
                            } else if (at(i + 2) == b'x' || at(i + 2) == b'X')
                                && isxdigit(at(i + 3))
                            {
                                p_parse.has_nonstd = 1;
                                t |= 0x01;
                                j = i + 4;
                                while isxdigit(at(j)) {
                                    j += 1;
                                }
                                stage = 2;
                            }
                        }
                    }
                    if stage == 0 {
                        stage = 1;
                    }
                }
                if stage == 1 {
                    // parse_number_2:
                    j = i + 1;
                    loop {
                        let mut c: u8 = at(j);
                        if isdigit(c) {
                            j += 1;
                            continue;
                        }
                        if c == b'.' {
                            if (t & 0x02) != 0 {
                                p_parse.i_err = j;
                                return -1;
                            }
                            t |= 0x02;
                            j += 1;
                            continue;
                        }
                        if c == b'e' || c == b'E' {
                            if at(j - 1) < b'0' {
                                if at(j - 1) == b'.'
                                    && j.wrapping_sub(2) >= i
                                    && isdigit(at(j.wrapping_sub(2)))
                                {
                                    p_parse.has_nonstd = 1;
                                    t |= 0x01;
                                } else {
                                    p_parse.i_err = j;
                                    return -1;
                                }
                            }
                            if seen_e != 0 {
                                p_parse.i_err = j;
                                return -1;
                            }
                            t |= 0x02;
                            seen_e = 1;
                            c = at(j + 1);
                            if c == b'+' || c == b'-' {
                                j += 1;
                                c = at(j + 1);
                            }
                            if c < b'0' || c > b'9' {
                                p_parse.i_err = j;
                                return -1;
                            }
                            j += 1;
                            continue;
                        }
                        break;
                    }
                    if at(j - 1) < b'0' {
                        if at(j - 1) == b'.'
                            && j.wrapping_sub(2) >= i
                            && isdigit(at(j.wrapping_sub(2)))
                        {
                            p_parse.has_nonstd = 1;
                            t |= 0x01;
                        } else {
                            p_parse.i_err = j;
                            return -1;
                        }
                    }
                }
                // parse_number_finish:
                debug_assert!(JSONB_INT + 0x01 == JSONB_INT5);
                debug_assert!(JSONB_FLOAT + 0x01 == JSONB_FLOAT5);
                debug_assert!(JSONB_INT + 0x02 == JSONB_FLOAT);
                if at(i) == b'+' {
                    i += 1;
                }
                json_blob_append_node(
                    p_parse,
                    JSONB_INT + t,
                    j - i,
                    Some(&z[i as usize..j as usize]),
                );
                return j as i32;
            }
            b'}' => {
                p_parse.i_err = i;
                return -2; // fim de {...}
            }
            b']' => {
                p_parse.i_err = i;
                return -3; // fim de [...]
            }
            b',' => {
                p_parse.i_err = i;
                return -4; // separador de lista
            }
            b':' => {
                p_parse.i_err = i;
                return -5; // separador de rótulo e valor de objeto
            }
            0 => {
                return 0; // fim do arquivo
            }
            0x09 | 0x0a | 0x0d | 0x20 => {
                i += 1 + spaces_span(tail(i + 1));
                continue; // goto json_parse_restart
            }
            0x0b | 0x0c | b'/' | 0xc2 | 0xe1 | 0xe2 | 0xe3 | 0xef => {
                let j: u32 = json5_whitespace(tail(i)) as u32;
                if j > 0 {
                    i += j;
                    p_parse.has_nonstd = 1;
                    continue; // goto json_parse_restart
                }
                p_parse.i_err = i;
                return -1;
            }
            ch => {
                // 'n' cai no caso padrão, que procura NaN, depois de testar "null".
                if ch == b'n' && tail(i).starts_with(b"null") && !isalnum(at(i + 4)) {
                    json_blob_append_one_byte(p_parse, JSONB_NULL);
                    return (i + 4) as i32;
                }
                let c: u8 = at(i);
                for entry in A_NAN_INF_NAME.iter() {
                    if c != entry.c1 && c != entry.c2 {
                        continue;
                    }
                    let nn: u32 = entry.n as u32;
                    if str_n_i_cmp(tail(i), entry.z_match, nn as _) != 0 {
                        continue;
                    }
                    if isalnum(at(i + nn)) {
                        continue;
                    }
                    if entry.e_type == JSONB_FLOAT {
                        json_blob_append_node(p_parse, JSONB_FLOAT, 5, Some(b"9e999"));
                    } else {
                        json_blob_append_one_byte(p_parse, JSONB_NULL);
                    }
                    p_parse.has_nonstd = 1;
                    return (i + nn) as i32;
                }
                p_parse.i_err = i;
                return -1; // erro de sintaxe
            }
        }
    }
}


// ---- part_005.rs ----

/// Faz o parse de um texto JSON completo. Devolve 0 em caso de sucesso ou
/// diferente de zero se houver erros. Havendo erro, libera toda a memória
/// retida por `p_parse`, mas não o próprio `p_parse`.
///
/// `p_parse` precisa ter sido inicializado como um objeto de parse vazio
/// antes de chamar esta rotina.
pub fn json_convert_text_to_blob(p_parse: &mut JsonParse, p_ctx: Option<&Sqlite3ContextRef>) -> i32 {
    // O C lê o texto terminado em NUL: fora do vetor vale 0.
    let at = |z: &[u8], k: usize| -> u8 { z.get(k).copied().unwrap_or(0) };
    let mut i: i32;
    i = json_translate_text_to_blob(p_parse, 0);
    if p_parse.oom != 0 {
        i = -1;
    }
    if i > 0 {
        while json_isspace(at(&p_parse.z_json, i as usize)) != 0 {
            i += 1;
        }
        if at(&p_parse.z_json, i as usize) != 0 {
            i += json5_whitespace(p_parse.z_json.get(i as usize..).unwrap_or(&[]));
            if at(&p_parse.z_json, i as usize) != 0 {
                if let Some(ctx) = p_ctx {
                    api::result_error(ctx, b"malformed JSON", -1);
                }
                json_parse_reset(p_parse);
                return 1;
            }
            p_parse.has_nonstd = 1;
        }
    }
    if i <= 0 {
        if let Some(ctx) = p_ctx {
            if p_parse.oom != 0 {
                api::result_error_nomem(ctx);
            } else {
                api::result_error(ctx, b"malformed JSON", -1);
            }
        }
        json_parse_reset(p_parse);
        return 1;
    }
    0
}

/// A string `p_str` é um texto JSON bem formado. Converte-a para o formato
/// JSONB e faz dela o valor de retorno da função SQL.
pub fn json_return_string_as_blob(p_str: &mut JsonString) {
    let mut px = JsonParse::default();
    json_string_terminate(p_str);
    if p_str.e_err != 0 {
        if let Some(ctx) = p_str.p_ctx.as_ref() {
            api::result_error_nomem(ctx);
        }
        return;
    }
    px.z_json = p_str.z_buf[..p_str.n_used as usize].to_vec();
    px.n_json = p_str.n_used as i32;
    if let Some(ctx) = p_str.p_ctx.as_ref() {
        px.db = api::context_db_handle(ctx);
    }
    let _ = json_translate_text_to_blob(&mut px, 0);
    if px.oom != 0 {
        // sqlite3DbFree(px.db, px.aBlob)
        px.a_blob = Vec::new();
        if let Some(ctx) = p_str.p_ctx.as_ref() {
            api::result_error_nomem(ctx);
        }
    } else {
        // O blob passa ao resultado (SQLITE_DYNAMIC): quem recebe é o dono.
        let n_blob = px.n_blob;
        let a_blob = std::mem::take(&mut px.a_blob);
        if let Some(ctx) = p_str.p_ctx.as_ref() {
            api::result_blob(ctx, &a_blob[..n_blob as usize], n_blob as i32, SQLITE_DYNAMIC);
        }
    }
}

/// O byte no índice `i` é um código de tipo de nó. Determina o tamanho do
/// payload desse nó e o grava em `*p_sz`. Devolve o deslocamento de `i` até o
/// início do payload. Devolve 0 em caso de erro.
pub fn jsonb_payload_size(p_parse: &JsonParse, i: u32, p_sz: &mut u32) -> u32 {
    let x: u8;
    let mut sz: u32;
    let n: u32;
    let b = |k: u32| -> u32 { p_parse.a_blob.get(k as usize).copied().unwrap_or(0) as u32 };
    if i > p_parse.n_blob {
        *p_sz = 0;
        return 0;
    }
    x = (b(i) >> 4) as u8;
    let mut n_out: u32;
    if x <= 11 {
        sz = x as u32;
        n_out = 1;
    } else if x == 12 {
        if i + 1 >= p_parse.n_blob {
            *p_sz = 0;
            return 0;
        }
        sz = b(i + 1);
        n_out = 2;
    } else if x == 13 {
        if i + 2 >= p_parse.n_blob {
            *p_sz = 0;
            return 0;
        }
        sz = (b(i + 1) << 8).wrapping_add(b(i + 2));
        n_out = 3;
    } else if x == 14 {
        if i + 4 >= p_parse.n_blob {
            *p_sz = 0;
            return 0;
        }
        sz = (b(i + 1) << 24)
            .wrapping_add(b(i + 2) << 16)
            .wrapping_add(b(i + 3) << 8)
            .wrapping_add(b(i + 4));
        n_out = 5;
    } else {
        if i + 8 >= p_parse.n_blob
            || b(i + 1) != 0
            || b(i + 2) != 0
            || b(i + 3) != 0
            || b(i + 4) != 0
        {
            *p_sz = 0;
            return 0;
        }
        sz = (b(i + 5) << 24)
            .wrapping_add(b(i + 6) << 16)
            .wrapping_add(b(i + 7) << 8)
            .wrapping_add(b(i + 8));
        n_out = 9;
    }
    n = n_out;
    // nBlob-delta em C: u32 menos int, em aritmética sem sinal de 32 bits.
    let lim: i64 = p_parse.n_blob.wrapping_sub(p_parse.delta as u32) as i64;
    if (i as i64) + (sz as i64) + (n as i64) > p_parse.n_blob as i64
        && (i as i64) + (sz as i64) + (n as i64) > lim
    {
        sz = 0;
        n_out = 0;
    }
    *p_sz = sz;
    n_out
}

/// Traduz a representação binária JSONB do JSON que começa em
/// `p_parse.a_blob[i]` para um texto JSON. Acrescenta o texto ao fim de
/// `p_out`. Devolve o índice em `a_blob[]` do primeiro byte depois do fim do
/// elemento traduzido.
///
/// Se for detectado um erro na entrada BLOB, o flag `p_out.e_err` pode receber
/// `JSTRING_MALFORMED`. Mas nem todos os erros da entrada são detectados: um
/// JSONB malformado pode resultar em erro ou em JSON incorreto.
///
/// O flag `JSTRING_OOM` de `p_out.e_err` é ligado quando falta memória.
pub fn json_translate_blob_to_text(p_parse: &JsonParse, i: u32, p_out: &mut JsonString) -> u32 {
    let mut sz: u32 = 0;
    let n: u32;
    let mut j: u32;
    let i_end: u32;
    // O C lê o texto terminado em NUL: fora do vetor vale 0.
    let at = |z: &[u8], k: u32| -> u8 { z.get(k as usize).copied().unwrap_or(0) };

    n = jsonb_payload_size(p_parse, i, &mut sz);
    if n == 0 {
        p_out.e_err |= JSTRING_MALFORMED;
        return p_parse.n_blob.wrapping_add(1);
    }
    let start: usize = (i.wrapping_add(n)) as usize;
    let mut malformed_jsonb = false;
    'sw: {
        match at(&p_parse.a_blob, i) & 0x0f {
            JSONB_NULL => {
                json_append_raw_nz(p_out, b"null", 4);
                return i.wrapping_add(1);
            }
            JSONB_TRUE => {
                json_append_raw_nz(p_out, b"true", 4);
                return i.wrapping_add(1);
            }
            JSONB_FALSE => {
                json_append_raw_nz(p_out, b"false", 5);
                return i.wrapping_add(1);
            }
            JSONB_INT | JSONB_FLOAT => {
                if sz == 0 {
                    malformed_jsonb = true;
                    break 'sw;
                }
                json_append_raw(p_out, p_parse.a_blob.get(start..).unwrap_or(&[]), sz);
            }
            JSONB_INT5 => {
                // Literal inteiro em notação hexadecimal
                let mut k: u32 = 2;
                let mut u: u64 = 0;
                let z_in: &[u8] = p_parse.a_blob.get(start..).unwrap_or(&[]);
                let mut b_overflow = false;
                if sz == 0 {
                    malformed_jsonb = true;
                    break 'sw;
                }
                if at(z_in, 0) == b'-' {
                    json_append_char(p_out, b'-');
                    k += 1;
                } else if at(z_in, 0) == b'+' {
                    k += 1;
                }
                while k < sz {
                    if !isxdigit(at(z_in, k)) {
                        p_out.e_err |= JSTRING_MALFORMED;
                        break;
                    } else if (u >> 60) != 0 {
                        b_overflow = true;
                    } else {
                        u = u * 16 + hex_to_int(at(z_in, k)) as u64;
                    }
                    k += 1;
                }
                json_printf(
                    100,
                    p_out,
                    if b_overflow { &b"9.0e999"[..] } else { &b"%llu"[..] },
                    &[PrintfArg::U64(u)],
                );
            }
            JSONB_FLOAT5 => {
                // Literal de ponto flutuante sem dígitos ao lado do "."
                let mut k: u32 = 0;
                let z_in: &[u8] = p_parse.a_blob.get(start..).unwrap_or(&[]);
                if sz == 0 {
                    malformed_jsonb = true;
                    break 'sw;
                }
                if at(z_in, 0) == b'-' {
                    json_append_char(p_out, b'-');
                    k += 1;
                }
                if at(z_in, k) == b'.' {
                    json_append_char(p_out, b'0');
                }
                while k < sz {
                    json_append_char(p_out, at(z_in, k));
                    if at(z_in, k) == b'.' && (k + 1 == sz || !isdigit(at(z_in, k + 1))) {
                        json_append_char(p_out, b'0');
                    }
                    k += 1;
                }
            }
            JSONB_TEXT | JSONB_TEXTJ => {
                json_append_char(p_out, b'"');
                json_append_raw(p_out, p_parse.a_blob.get(start..).unwrap_or(&[]), sz);
                json_append_char(p_out, b'"');
            }
            JSONB_TEXT5 => {
                let mut z_in: &[u8] = p_parse.a_blob.get(start..).unwrap_or(&[]);
                let mut k: u32;
                let mut sz2: u32 = sz;
                json_append_char(p_out, b'"');
                while sz2 > 0 {
                    k = 0;
                    while k < sz2
                        && (JSON_IS_OK[at(z_in, k) as usize] != 0 || at(z_in, k) == b'\'')
                    {
                        k += 1;
                    }
                    if k > 0 {
                        json_append_raw_nz(p_out, z_in, k);
                        if k >= sz2 {
                            break;
                        }
                        z_in = z_in.get(k as usize..).unwrap_or(&[]);
                        sz2 -= k;
                    }
                    if at(z_in, 0) == b'"' {
                        json_append_raw_nz(p_out, b"\\\"", 2);
                        z_in = z_in.get(1..).unwrap_or(&[]);
                        sz2 -= 1;
                        continue;
                    }
                    if at(z_in, 0) <= 0x1f {
                        if p_out.n_used + 7 > p_out.n_alloc && json_string_grow(p_out, 7) != 0 {
                            break;
                        }
                        json_append_control_char(p_out, at(z_in, 0));
                        z_in = z_in.get(1..).unwrap_or(&[]);
                        sz2 -= 1;
                        continue;
                    }
                    if sz2 < 2 {
                        p_out.e_err |= JSTRING_MALFORMED;
                        break;
                    }
                    match at(z_in, 1) {
                        b'\'' => {
                            json_append_char(p_out, b'\'');
                        }
                        b'v' => {
                            json_append_raw_nz(p_out, b"\\u0009", 6);
                        }
                        b'x' => {
                            if sz2 < 4 {
                                p_out.e_err |= JSTRING_MALFORMED;
                                sz2 = 2;
                            } else {
                                json_append_raw_nz(p_out, b"\\u00", 4);
                                json_append_raw_nz(p_out, z_in.get(2..).unwrap_or(&[]), 2);
                                z_in = z_in.get(2..).unwrap_or(&[]);
                                sz2 -= 2;
                            }
                        }
                        b'0' => {
                            json_append_raw_nz(p_out, b"\\u0000", 6);
                        }
                        b'\r' => {
                            if sz2 > 2 && at(z_in, 2) == b'\n' {
                                z_in = z_in.get(1..).unwrap_or(&[]);
                                sz2 -= 1;
                            }
                        }
                        b'\n' => {}
                        0xe2 => {
                            // '\' seguido de U+2028 ou U+2029 é ignorado como
                            // espaço em branco. Em UTF8, U+2028 é 0xe2 0x80 0xa8.
                            // U+2029 é igual, exceto pelo último byte.
                            if sz2 < 4
                                || 0x80 != at(z_in, 2)
                                || (0xa8 != at(z_in, 3) && 0xa9 != at(z_in, 3))
                            {
                                p_out.e_err |= JSTRING_MALFORMED;
                                sz2 = 2;
                            } else {
                                z_in = z_in.get(2..).unwrap_or(&[]);
                                sz2 -= 2;
                            }
                        }
                        _ => {
                            json_append_raw_nz(p_out, z_in, 2);
                        }
                    }
                    z_in = z_in.get(2..).unwrap_or(&[]);
                    sz2 -= 2;
                }
                json_append_char(p_out, b'"');
            }
            JSONB_TEXTRAW => {
                json_append_string(p_out, p_parse.a_blob.get(start..).unwrap_or(&[]), sz);
            }
            JSONB_ARRAY => {
                json_append_char(p_out, b'[');
                j = i.wrapping_add(n);
                i_end = j.wrapping_add(sz);
                while j < i_end && p_out.e_err == 0 {
                    j = json_translate_blob_to_text(p_parse, j, p_out);
                    json_append_char(p_out, b',');
                }
                if j > i_end {
                    p_out.e_err |= JSTRING_MALFORMED;
                }
                if sz > 0 {
                    json_string_trim_one_char(p_out);
                }
                json_append_char(p_out, b']');
            }
            JSONB_OBJECT => {
                let mut x: i32 = 0;
                json_append_char(p_out, b'{');
                j = i.wrapping_add(n);
                i_end = j.wrapping_add(sz);
                while j < i_end && p_out.e_err == 0 {
                    j = json_translate_blob_to_text(p_parse, j, p_out);
                    let sep = if (x & 1) != 0 { b',' } else { b':' };
                    x = x.wrapping_add(1);
                    json_append_char(p_out, sep);
                }
                if (x & 1) != 0 || j > i_end {
                    p_out.e_err |= JSTRING_MALFORMED;
                }
                if sz > 0 {
                    json_string_trim_one_char(p_out);
                }
                json_append_char(p_out, b'}');
            }
            _ => {
                malformed_jsonb = true;
            }
        }
    }
    if malformed_jsonb {
        // malformed_jsonb:
        p_out.e_err |= JSTRING_MALFORMED;
    }
    i.wrapping_add(n).wrapping_add(sz)
}


// ---- part_006.rs ----

/// Contexto da recursão de `json_pretty()`.
pub struct JsonPretty<'a> {
    pub p_parse: &'a JsonParse,     // o BLOB que está sendo renderizado
    pub p_out: &'a mut JsonString,  // gera a saída "pretty" nesta string
    pub z_indent: &'a [u8],         // usa este texto para indentar
    pub sz_indent: u32,             // bytes em z_indent[]
    pub n_indent: u32,              // nível de indentação atual
}

/// Acrescenta a indentação ao JSON "pretty" em construção.
pub fn json_pretty_indent(p_pretty: &mut JsonPretty) {
    let mut jj: u32 = 0;
    while jj < p_pretty.n_indent {
        json_append_raw(p_pretty.p_out, p_pretty.z_indent, p_pretty.sz_indent);
        jj += 1;
    }
}

/// Traduz a representação binária JSONB que começa em `p_parse.a_blob[i]`
/// para uma string de texto JSON. Acrescenta o texto ao final de `p_out`.
/// Devolve o índice em `p_parse.a_blob[]` do primeiro byte depois do fim do
/// elemento traduzido.
///
/// É uma variante de `json_translate_blob_to_text()` que formata a saída
/// ("pretty-print"), inserindo espaços em branco extras para facilitar a
/// leitura por humanos.
///
/// Se um erro for detectado no BLOB de entrada, o flag `p_out.e_err` pode
/// receber `JSTRING_MALFORMED`. Mas nem todo erro de entrada é detectado:
/// um JSONB malformado pode resultar em erro ou em JSON incorreto.
///
/// O flag `JSTRING_OOM` de `p_out.e_err` é ligado em caso de OOM.
pub fn json_translate_blob_to_pretty_text(
    p_pretty: &mut JsonPretty, /* Contexto de pretty-printing */
    i: u32,                    /* Começa a renderizar neste índice */
) -> u32 {
    let mut i = i;
    let mut sz: u32 = 0;
    let n: u32;
    let mut j: u32;
    let i_end: u32;
    let p_parse: &JsonParse = p_pretty.p_parse;
    n = jsonb_payload_size(p_parse, i, &mut sz);
    if n == 0 {
        p_pretty.p_out.e_err |= JSTRING_MALFORMED;
        return p_parse.n_blob.wrapping_add(1);
    }
    match p_parse.a_blob[i as usize] & 0x0f {
        JSONB_ARRAY => {
            j = i.wrapping_add(n);
            i_end = j.wrapping_add(sz);
            json_append_char(p_pretty.p_out, b'[');
            if j < i_end {
                json_append_char(p_pretty.p_out, b'\n');
                p_pretty.n_indent = p_pretty.n_indent.wrapping_add(1);
                while p_pretty.p_out.e_err == 0 {
                    json_pretty_indent(p_pretty);
                    j = json_translate_blob_to_pretty_text(p_pretty, j);
                    if j >= i_end {
                        break;
                    }
                    json_append_raw_nz(p_pretty.p_out, b",\n", 2);
                }
                json_append_char(p_pretty.p_out, b'\n');
                p_pretty.n_indent = p_pretty.n_indent.wrapping_sub(1);
                json_pretty_indent(p_pretty);
            }
            json_append_char(p_pretty.p_out, b']');
            i = i_end;
        }
        JSONB_OBJECT => {
            j = i.wrapping_add(n);
            i_end = j.wrapping_add(sz);
            json_append_char(p_pretty.p_out, b'{');
            if j < i_end {
                json_append_char(p_pretty.p_out, b'\n');
                p_pretty.n_indent = p_pretty.n_indent.wrapping_add(1);
                while p_pretty.p_out.e_err == 0 {
                    json_pretty_indent(p_pretty);
                    j = json_translate_blob_to_text(p_parse, j, p_pretty.p_out);
                    if j > i_end {
                        p_pretty.p_out.e_err |= JSTRING_MALFORMED;
                        break;
                    }
                    json_append_raw_nz(p_pretty.p_out, b": ", 2);
                    j = json_translate_blob_to_pretty_text(p_pretty, j);
                    if j >= i_end {
                        break;
                    }
                    json_append_raw_nz(p_pretty.p_out, b",\n", 2);
                }
                json_append_char(p_pretty.p_out, b'\n');
                p_pretty.n_indent = p_pretty.n_indent.wrapping_sub(1);
                json_pretty_indent(p_pretty);
            }
            json_append_char(p_pretty.p_out, b'}');
            i = i_end;
        }
        _ => {
            i = json_translate_blob_to_text(p_parse, i, p_pretty.p_out);
        }
    }
    i
}

/// Devolve verdadeiro se a entrada `p_json` pode ser JSONB.
///
/// Por desempenho, esta rotina não faz uma verificação detalhada do BLOB de
/// entrada para garantir que esteja bem formado. Portanto, falsos positivos
/// são possíveis. Falsos negativos nunca devem ocorrer.
pub fn json_func_arg_might_be_binary(p_json: &mut Mem) -> i32 {
    let mut sz: u32 = 0;
    let n: u32;
    if api::value_type(p_json) != SQLITE_BLOB {
        return 0;
    }
    let a_blob: Vec<u8> = api::value_blob(p_json).map(|s| s.to_vec()).unwrap_or_default();
    let n_blob: i32 = api::value_bytes(p_json);
    if n_blob < 1 {
        return 0;
    }
    if a_blob.is_empty() || (a_blob[0] & 0x0f) > JSONB_OBJECT {
        return 0;
    }
    let mut s = JsonParse::default();
    s.a_blob = a_blob;
    s.n_blob = n_blob as u32;
    n = jsonb_payload_size(&s, 0, &mut sz);
    if n == 0 {
        return 0;
    }
    if sz.wrapping_add(n) != n_blob as u32 {
        return 0;
    }
    if (s.a_blob[0] & 0x0f) <= JSONB_FALSE && sz > 0 {
        return 0;
    }
    (sz.wrapping_add(n) == n_blob as u32) as i32
}

/// Dado que um objeto `JSONB_ARRAY` começa no deslocamento `i_root`,
/// devolve o número de entradas desse array.
pub fn jsonb_array_count(p_parse: &mut JsonParse, i_root: u32) -> u32 {
    let mut sz: u32 = 0;
    let mut n: u32;
    let mut i: u32;
    let i_end: u32;
    let mut k: u32 = 0;
    n = jsonb_payload_size(p_parse, i_root, &mut sz);
    i_end = i_root.wrapping_add(n).wrapping_add(sz);
    i = i_root.wrapping_add(n);
    while n > 0 && i < i_end {
        n = jsonb_payload_size(p_parse, i, &mut sz);
        i = i.wrapping_add(sz).wrapping_add(n);
        k = k.wrapping_add(1);
    }
    k
}

/// Edita o tamanho do payload do elemento em `i_root` pela quantidade em
/// `p_parse.delta`.
pub fn json_after_edit_size_adjust(p_parse: &mut JsonParse, i_root: u32) {
    let mut sz: u32 = 0;
    debug_assert!(p_parse.delta != 0);
    debug_assert!(p_parse.n_blob_alloc >= p_parse.n_blob);
    let n_blob: u32 = p_parse.n_blob;
    p_parse.n_blob = p_parse.n_blob_alloc;
    let _ = jsonb_payload_size(p_parse, i_root, &mut sz);
    p_parse.n_blob = n_blob;
    sz = sz.wrapping_add(p_parse.delta as u32);
    let d = json_blob_change_payload_size(p_parse, i_root, sz);
    p_parse.delta = p_parse.delta.wrapping_add(d);
}

/// Modifica o JSONB em `p_parse.a_blob` removendo `n_del` bytes de conteúdo
/// a partir de `i_del` e substituindo-os por `n_ins` bytes de conteúdo dados
/// por `a_ins`.
///
/// `n_del` pode ser zero, caso em que nenhum byte é removido. Mas `i_del`
/// continua importante, pois os novos bytes são inseridos a partir de `i_del`.
///
/// `a_ins` pode ser `None`, caso em que é criado espaço para `n_ins` bytes a
/// partir de `i_del`, mas esse espaço fica sem inicializar.
///
/// Liga `p_parse.oom` se ocorrer OOM.
pub fn json_blob_edit(
    p_parse: &mut JsonParse, /* O JSONB a modificar está em p_parse.a_blob */
    i_del: u32,              /* Primeiro byte a remover */
    n_del: u32,              /* Número de bytes a remover */
    a_ins: Option<&[u8]>,    /* Conteúdo a inserir */
    n_ins: u32,              /* Bytes de conteúdo a inserir */
) {
    let d: i64 = (n_ins as i64) - (n_del as i64);
    if d != 0 {
        if (p_parse.n_blob as i64) + d > p_parse.n_blob_alloc as i64 {
            json_blob_expand(p_parse, ((p_parse.n_blob as i64) + d) as u32);
            if p_parse.oom != 0 {
                return;
            }
        }
        let src_start = (i_del as usize) + (n_del as usize);
        let src_end = p_parse.n_blob as usize;
        let dst = (i_del as usize) + (n_ins as usize);
        p_parse.a_blob.copy_within(src_start..src_end, dst);
        p_parse.n_blob = ((p_parse.n_blob as i64) + d) as u32;
        p_parse.delta = p_parse.delta.wrapping_add(d as i32);
    }
    if n_ins != 0 {
        if let Some(a_ins) = a_ins {
            let st = i_del as usize;
            let nn = n_ins as usize;
            p_parse.a_blob[st..st + nn].copy_from_slice(&a_ins[..nn]);
        }
    }
}

/// Devolve o número de newlines escapados a serem ignorados. Um newline
/// escapado é uma das seguintes sequências de bytes:
///
///    0x5c 0x0a
///    0x5c 0x0d
///    0x5c 0x0d 0x0a
///    0x5c 0xe2 0x80 0xa8
///    0x5c 0xe2 0x80 0xa9
pub fn json_bytes_to_bypass(z: &[u8], n: u32) -> u32 {
    let mut i: u32 = 0;
    while i.wrapping_add(1) < n {
        let iu = i as usize;
        if z[iu] != b'\\' {
            return i;
        }
        if z[iu + 1] == b'\n' {
            i += 2;
            continue;
        }
        if z[iu + 1] == b'\r' {
            if i + 2 < n && z[iu + 2] == b'\n' {
                i += 3;
            } else {
                i += 2;
            }
            continue;
        }
        if 0xe2 == z[iu + 1]
            && i + 3 < n
            && 0x80 == z[iu + 2]
            && (0xa8 == z[iu + 3] || 0xa9 == z[iu + 3])
        {
            i += 4;
            continue;
        }
        break;
    }
    i
}

/// A entrada `z[0..n]` define uma sequência de escape JSON incluindo a '\\'
/// inicial. Decodifica a sequência em um único caractere. Grava o caractere
/// em `*pi_out`. Devolve o número de bytes da sequência de escape.
///
/// Se houver erro de sintaxe (por exemplo, poucos caracteres depois da '\\'
/// para completar a codificação), `*pi_out` recebe `JSON_INVALID_CHAR`.
pub fn json_unescape_one_char(z: &[u8], n: u32, pi_out: &mut u32) -> u32 {
    debug_assert!(n > 0);
    debug_assert!(z[0] == b'\\');
    if n < 2 {
        *pi_out = JSON_INVALID_CHAR;
        return n;
    }
    match z[1] {
        b'u' => {
            if n < 6 {
                *pi_out = JSON_INVALID_CHAR;
                return n;
            }
            let v: u32 = json_hex_to_int4(&z[2..]);
            if (v & 0xfc00) == 0xd800 && n >= 12 && z[6] == b'\\' && z[7] == b'u' {
                let vlo: u32 = json_hex_to_int4(&z[8..]);
                if (vlo & 0xfc00) == 0xdc00 {
                    *pi_out = ((v & 0x3ff) << 10)
                        .wrapping_add(vlo & 0x3ff)
                        .wrapping_add(0x10000);
                    return 12;
                }
            }
            *pi_out = v;
            6
        }
        b'b' => {
            *pi_out = 0x08;
            2
        }
        b'f' => {
            *pi_out = 0x0c;
            2
        }
        b'n' => {
            *pi_out = b'\n' as u32;
            2
        }
        b'r' => {
            *pi_out = b'\r' as u32;
            2
        }
        b't' => {
            *pi_out = b'\t' as u32;
            2
        }
        b'v' => {
            *pi_out = 0x0b;
            2
        }
        b'0' => {
            *pi_out = 0;
            2
        }
        b'\'' | b'"' | b'/' | b'\\' => {
            *pi_out = z[1] as u32;
            2
        }
        b'x' => {
            if n < 4 {
                *pi_out = JSON_INVALID_CHAR;
                return n;
            }
            *pi_out = ((json_hex_to_int(z[2] as i8 as i32) as u32) << 4)
                | (json_hex_to_int(z[3] as i8 as i32) as u32);
            4
        }
        0xe2 | b'\r' | b'\n' => {
            let n_skip: u32 = json_bytes_to_bypass(z, n);
            if n_skip == 0 {
                *pi_out = JSON_INVALID_CHAR;
                n
            } else if n_skip == n {
                *pi_out = 0;
                n
            } else if z[n_skip as usize] == b'\\' {
                n_skip + json_unescape_one_char(&z[n_skip as usize..], n - n_skip, pi_out)
            } else {
                let sz = utf8_read_limited(&z[n_skip as usize..], (n - n_skip) as usize, pi_out);
                n_skip + sz as u32
            }
        }
        _ => {
            *pi_out = JSON_INVALID_CHAR;
            2
        }
    }
}

/// Compara dois rótulos de objeto. Devolve 1 se iguais e 0 se diferentes.
///
/// Nesta versão, sabemos que um dos comparandos, ou ambos, contém uma
/// sequência de escape.
#[inline(never)]
pub fn json_label_compare_escaped(
    z_left: &[u8],  /* O rótulo da esquerda */
    n_left: u32,    /* Tamanho do rótulo da esquerda em bytes */
    raw_left: i32,  /* Verdadeiro se z_left não contém escapes */
    z_right: &[u8], /* O rótulo da direita */
    n_right: u32,   /* Tamanho do rótulo da direita em bytes */
    raw_right: i32, /* Verdadeiro se z_right é livre de escapes */
) -> i32 {
    let mut n_left = n_left;
    let mut n_right = n_right;
    let mut il: usize = 0;
    let mut ir: usize = 0;
    let mut c_left: u32 = 0;
    let mut c_right: u32 = 0;
    debug_assert!(raw_left == 0 || raw_right == 0);
    loop {
        /* sai por return */
        if n_left == 0 {
            c_left = 0;
        } else if raw_left != 0 || z_left[il] != b'\\' {
            c_left = z_left[il] as u32;
            if c_left >= 0xc0 {
                let sz = utf8_read_limited(&z_left[il..], n_left as usize, &mut c_left);
                il += sz;
                n_left -= sz as u32;
            } else {
                il += 1;
                n_left -= 1;
            }
        } else {
            let n = json_unescape_one_char(&z_left[il..], n_left, &mut c_left);
            il += n as usize;
            debug_assert!(n <= n_left);
            n_left -= n;
        }
        if n_right == 0 {
            c_right = 0;
        } else if raw_right != 0 || z_right[ir] != b'\\' {
            c_right = z_right[ir] as u32;
            if c_right >= 0xc0 {
                let sz = utf8_read_limited(&z_right[ir..], n_right as usize, &mut c_right);
                ir += sz;
                n_right -= sz as u32;
            } else {
                ir += 1;
                n_right -= 1;
            }
        } else {
            let n = json_unescape_one_char(&z_right[ir..], n_right, &mut c_right);
            ir += n as usize;
            debug_assert!(n <= n_right);
            n_right -= n;
        }
        if c_left != c_right {
            return 0;
        }
        if c_left == 0 {
            return 1;
        }
    }
}


// ---- part_007.rs ----

/// Lê o byte `i` de `z` como o C lê uma string terminada em NUL: além do fim
/// do slice o resultado é 0.
#[inline]
fn json_path_at(z: &[u8], i: u32) -> u8 {
    z.get(i as usize).copied().unwrap_or(0)
}

/// Equivalente de `&zPath[i]`: o resto do caminho a partir de `i` (vazio se
/// `i` passa do fim, que no C seria o NUL terminador).
#[inline]
fn json_path_tail(z: &[u8], i: u32) -> &[u8] {
    if (i as usize) >= z.len() {
        &[]
    } else {
        &z[i as usize..]
    }
}

/// Compara dois rótulos de objeto. Devolve 1 se são iguais e 0 se diferem.
/// Devolveria -1 em caso de OOM.
pub fn json_label_compare(
    z_left: &[u8],   // O rótulo da esquerda
    n_left: u32,     // Tamanho do rótulo da esquerda em bytes
    raw_left: i32,   // Verdadeiro se z_left não contém escapes
    z_right: &[u8],  // O rótulo da direita
    n_right: u32,    // Tamanho do rótulo da direita em bytes
    raw_right: i32,  // Verdadeiro se z_right não tem escapes
) -> i32 {
    if raw_left != 0 && raw_right != 0 {
        // Caso mais simples: nenhum rótulo contém escapes. Um memcmp() basta.
        if n_left != n_right {
            return 0;
        }
        return (z_left[..n_left as usize] == z_right[..n_left as usize]) as i32;
    } else {
        return json_label_compare_escaped(z_left, n_left, raw_left, z_right, n_right, raw_right);
    }
}

// Códigos de erro devolvidos por json_lookup_step()
pub const JSON_LOOKUP_ERROR: u32 = 0xffffffff;
pub const JSON_LOOKUP_NOTFOUND: u32 = 0xfffffffe;
pub const JSON_LOOKUP_PATHERROR: u32 = 0xfffffffd;

/// Equivalente do macro `JSON_LOOKUP_ISERROR(x)`.
#[inline]
pub fn json_lookup_iserror(x: u32) -> bool {
    x >= JSON_LOOKUP_PATHERROR
}

/// Rotina auxiliar de `json_lookup_step()` que preenche `p_ins` com os dados
/// binários a inserir em `p_parse`.
///
/// No caso comum, `p_ins` apenas aponta para `a_ins` e `n_ins` de `p_parse`.
/// Mas se o `z_path` da operação de edição original inclui elementos de
/// caminho mais profundos, é preciso criar subestrutura adicional.
///
/// Por exemplo:
///
///     json_insert('{}', '$.a.b.c', 123);
///
/// A busca para em '$.a'. Mas é preciso criar subestrutura adicional para a
/// parte ".b.c" do patch, de modo que o resultado final seja:
/// {"a":{"b":{"c":123}}}. Esta rotina preenche `p_ins` com o equivalente
/// binário de {"b":{"c":123}} para que possa ser inserido.
///
/// O chamador é responsável por reiniciar `p_ins` ao terminar de usar a
/// subestrutura.
pub fn json_create_edit_substructure(
    p_parse: &mut JsonParse, // O JSONB original que está sendo editado
    p_ins: &mut JsonParse,   // Preenche com os dados do blob a inserir
    z_tail: &[u8],           // Cauda do caminho que determina a subestrutura
) -> u32 {
    const EMPTY_OBJECT: [u8; 2] = [JSONB_ARRAY, JSONB_OBJECT];
    let rc: u32;
    *p_ins = JsonParse::default();
    p_ins.db = p_parse.db.clone();
    if json_path_at(z_tail, 0) == 0 {
        // Sem subestrutura. Só insere o que foi dado em p_parse. No C o blob
        // aponta para a_ins; aqui é uma cópia (n_blob_alloc fica 0, então o
        // blob continua sendo tratado como não editável).
        p_ins.a_blob = p_parse.a_ins.clone();
        p_ins.n_blob = p_parse.n_ins;
        rc = 0;
    } else {
        // Constrói a subestrutura binária
        p_ins.n_blob = 1;
        p_ins.a_blob = vec![EMPTY_OBJECT[(json_path_at(z_tail, 0) == b'.') as usize]];
        p_ins.e_edit = p_parse.e_edit;
        p_ins.n_ins = p_parse.n_ins;
        p_ins.a_ins = p_parse.a_ins.clone();
        rc = json_lookup_step(p_ins, 0, z_tail, 0);
        p_parse.oom |= p_ins.oom;
    }
    rc // Só o código de erro
}

/// Busca ao longo de `z_path` para achar o elemento JSON especificado. Devolve
/// um índice em `p_parse.a_blob[]` para o início do valor desse elemento.
///
/// Se o valor achado por esta rotina é a metade de valor de um par
/// rótulo/valor dentro de um objeto, então `p_parse.i_label` é posto no início
/// do rótulo correspondente antes de retornar.
///
/// Devolve um dos códigos de erro JSON_LOOKUP se há problemas.
///
/// Esta rotina também modifica o blob. Se `p_parse.e_edit` é um de JEDIT_DEL,
/// JEDIT_REPL, JEDIT_INS ou JEDIT_SET, mudanças podem ser feitas no valor
/// selecionado. Se uma edição é feita, o valor de retorno não aponta
/// necessariamente para o elemento selecionado e só serve para detectar
/// condições de erro.
pub fn json_lookup_step(
    p_parse: &mut JsonParse, // O JSON a pesquisar
    i_root: u32,             // Começa a busca neste elemento de a_blob[]
    z_path: &[u8],           // O caminho a pesquisar
    i_label: u32,            // Rótulo se i_root é um valor dentro de um objeto
) -> u32 {
    let mut i: u32;
    let mut j: u32;
    let mut k: u32;
    let n_key: u32;
    let mut sz: u32 = 0;
    let mut n: u32;
    let i_end: u32;
    let rc: u32;
    let mut x: u8;
    let mut i_root = i_root;

    if json_path_at(z_path, 0) == 0 {
        if p_parse.e_edit != 0 && json_blob_make_editable(p_parse, p_parse.n_ins) != 0 {
            n = jsonb_payload_size(p_parse, i_root, &mut sz);
            sz = sz.wrapping_add(n);
            if p_parse.e_edit == JEDIT_DEL {
                if i_label > 0 {
                    sz = sz.wrapping_add(i_root.wrapping_sub(i_label));
                    i_root = i_label;
                }
                json_blob_edit(p_parse, i_root, sz, None, 0);
            } else if p_parse.e_edit == JEDIT_INS {
                // Já existe, então json_insert() não faz nada
            } else {
                // json_set() ou json_replace()
                let a_ins = p_parse.a_ins.clone();
                let n_ins = p_parse.n_ins;
                json_blob_edit(p_parse, i_root, sz, Some(&a_ins[..]), n_ins);
            }
        }
        p_parse.i_label = i_label;
        return i_root;
    }
    if json_path_at(z_path, 0) == b'.' {
        let mut raw_key: bool = true;
        x = p_parse.a_blob[i_root as usize];
        let z_path = &z_path[1..];
        let z_key: &[u8];
        if json_path_at(z_path, 0) == b'"' {
            i = 1;
            while json_path_at(z_path, i) != 0 && json_path_at(z_path, i) != b'"' {
                i += 1;
            }
            n_key = i - 1;
            z_key = &z_path[1..1 + n_key as usize];
            if json_path_at(z_path, i) != 0 {
                i += 1;
            } else {
                return JSON_LOOKUP_PATHERROR;
            }
            raw_key = !z_key.contains(&b'\\');
        } else {
            i = 0;
            while json_path_at(z_path, i) != 0
                && json_path_at(z_path, i) != b'.'
                && json_path_at(z_path, i) != b'['
            {
                i += 1;
            }
            n_key = i;
            z_key = &z_path[..n_key as usize];
            if n_key == 0 {
                return JSON_LOOKUP_PATHERROR;
            }
        }
        if (x & 0x0f) != JSONB_OBJECT {
            return JSON_LOOKUP_NOTFOUND;
        }
        n = jsonb_payload_size(p_parse, i_root, &mut sz);
        j = i_root.wrapping_add(n); // j é o índice de um rótulo
        i_end = j.wrapping_add(sz);
        while j < i_end {
            x = p_parse.a_blob[j as usize] & 0x0f;
            if x < JSONB_TEXT || x > JSONB_TEXTRAW {
                return JSON_LOOKUP_ERROR;
            }
            n = jsonb_payload_size(p_parse, j, &mut sz);
            if n == 0 {
                return JSON_LOOKUP_ERROR;
            }
            k = j.wrapping_add(n); // k é o índice do texto do rótulo
            if k.wrapping_add(sz) >= i_end {
                return JSON_LOOKUP_ERROR;
            }
            let raw_label: bool = x == JSONB_TEXT || x == JSONB_TEXTRAW;
            let equal = {
                let z_label = &p_parse.a_blob[k as usize..(k + sz) as usize];
                json_label_compare(z_key, n_key, raw_key as i32, z_label, sz, raw_label as i32)
            };
            if equal != 0 {
                let v: u32 = k + sz; // v é o índice do valor
                if (p_parse.a_blob[v as usize] & 0x0f) > JSONB_OBJECT {
                    return JSON_LOOKUP_ERROR;
                }
                n = jsonb_payload_size(p_parse, v, &mut sz);
                if n == 0 || v.wrapping_add(n).wrapping_add(sz) > i_end {
                    return JSON_LOOKUP_ERROR;
                }
                debug_assert!(j > 0);
                rc = json_lookup_step(p_parse, v, json_path_tail(z_path, i), j);
                if p_parse.delta != 0 {
                    json_after_edit_size_adjust(p_parse, i_root);
                }
                return rc;
            }
            j = k.wrapping_add(sz);
            if (p_parse.a_blob[j as usize] & 0x0f) > JSONB_OBJECT {
                return JSON_LOOKUP_ERROR;
            }
            n = jsonb_payload_size(p_parse, j, &mut sz);
            if n == 0 {
                return JSON_LOOKUP_ERROR;
            }
            j = j.wrapping_add(n).wrapping_add(sz);
        }
        if j > i_end {
            return JSON_LOOKUP_ERROR;
        }
        if p_parse.e_edit >= JEDIT_INS {
            let n_ins: u32; // Total de bytes a inserir (rótulo mais valor)
            let mut v = JsonParse::default(); // Codificação BLOB do valor a inserir
            let mut ix = JsonParse::default(); // Cabeçalho do rótulo a inserir
            ix.db = p_parse.db.clone();
            json_blob_append_node(
                &mut ix,
                if raw_key { JSONB_TEXTRAW } else { JSONB_TEXT5 },
                n_key,
                None,
            );
            p_parse.oom |= ix.oom;
            let rc = json_create_edit_substructure(p_parse, &mut v, json_path_tail(z_path, i));
            if !json_lookup_iserror(rc)
                && json_blob_make_editable(
                    p_parse,
                    ix.n_blob.wrapping_add(n_key).wrapping_add(v.n_blob),
                ) != 0
            {
                debug_assert!(p_parse.oom == 0);
                n_ins = ix.n_blob.wrapping_add(n_key).wrapping_add(v.n_blob);
                json_blob_edit(p_parse, j, 0, None, n_ins);
                if p_parse.oom == 0 {
                    debug_assert!(!p_parse.a_blob.is_empty()); // Porque p_parse.oom é 0
                    debug_assert!(!ix.a_blob.is_empty()); // Porque p_parse.oom é 0
                    let jj = j as usize;
                    let nb = ix.n_blob as usize;
                    p_parse.a_blob[jj..jj + nb].copy_from_slice(&ix.a_blob[..nb]);
                    k = j + ix.n_blob;
                    let kk = k as usize;
                    p_parse.a_blob[kk..kk + n_key as usize].copy_from_slice(z_key);
                    k += n_key;
                    let kk = k as usize;
                    let vb = v.n_blob as usize;
                    p_parse.a_blob[kk..kk + vb].copy_from_slice(&v.a_blob[..vb]);
                    if p_parse.delta != 0 {
                        json_after_edit_size_adjust(p_parse, i_root);
                    }
                }
            }
            json_parse_reset(&mut v);
            json_parse_reset(&mut ix);
            return rc;
        }
    } else if json_path_at(z_path, 0) == b'[' {
        x = p_parse.a_blob[i_root as usize] & 0x0f;
        if x != JSONB_ARRAY {
            return JSON_LOOKUP_NOTFOUND;
        }
        n = jsonb_payload_size(p_parse, i_root, &mut sz);
        k = 0;
        i = 1;
        while isdigit(json_path_at(z_path, i)) {
            k = k
                .wrapping_mul(10)
                .wrapping_add(json_path_at(z_path, i) as u32)
                .wrapping_sub(b'0' as u32);
            i += 1;
        }
        if i < 2 || json_path_at(z_path, i) != b']' {
            if json_path_at(z_path, 1) == b'#' {
                k = jsonb_array_count(p_parse, i_root);
                i = 2;
                if json_path_at(z_path, 2) == b'-' && isdigit(json_path_at(z_path, 3)) {
                    let mut nn: u32 = 0;
                    i = 3;
                    loop {
                        nn = nn
                            .wrapping_mul(10)
                            .wrapping_add(json_path_at(z_path, i) as u32)
                            .wrapping_sub(b'0' as u32);
                        i += 1;
                        if !isdigit(json_path_at(z_path, i)) {
                            break;
                        }
                    }
                    if nn > k {
                        return JSON_LOOKUP_NOTFOUND;
                    }
                    k -= nn;
                }
                if json_path_at(z_path, i) != b']' {
                    return JSON_LOOKUP_PATHERROR;
                }
            } else {
                return JSON_LOOKUP_PATHERROR;
            }
        }
        j = i_root.wrapping_add(n);
        i_end = j.wrapping_add(sz);
        while j < i_end {
            if k == 0 {
                rc = json_lookup_step(p_parse, j, json_path_tail(z_path, i + 1), 0);
                if p_parse.delta != 0 {
                    json_after_edit_size_adjust(p_parse, i_root);
                }
                return rc;
            }
            k -= 1;
            n = jsonb_payload_size(p_parse, j, &mut sz);
            if n == 0 {
                return JSON_LOOKUP_ERROR;
            }
            j = j.wrapping_add(n).wrapping_add(sz);
        }
        if j > i_end {
            return JSON_LOOKUP_ERROR;
        }
        if k > 0 {
            return JSON_LOOKUP_NOTFOUND;
        }
        if p_parse.e_edit >= JEDIT_INS {
            let mut v = JsonParse::default();
            let rc = json_create_edit_substructure(p_parse, &mut v, json_path_tail(z_path, i + 1));
            if !json_lookup_iserror(rc) && json_blob_make_editable(p_parse, v.n_blob) != 0 {
                debug_assert!(p_parse.oom == 0);
                let vb = v.n_blob as usize;
                json_blob_edit(p_parse, j, 0, Some(&v.a_blob[..vb]), v.n_blob);
            }
            json_parse_reset(&mut v);
            if p_parse.delta != 0 {
                json_after_edit_size_adjust(p_parse, i_root);
            }
            return rc;
        }
    } else {
        return JSON_LOOKUP_PATHERROR;
    }
    JSON_LOOKUP_NOTFOUND
}

/// Converte um BLOB JSON em texto e faz desse texto o valor de retorno de uma
/// função SQL.
pub fn json_return_text_json_from_blob(
    ctx: &Sqlite3ContextRef,
    a_blob: &[u8],
    n_blob: u32,
) {
    // O NEVER(a_blob==0) do C não tem equivalente: um slice nunca é nulo.
    let mut x = JsonParse::default();
    let mut s = JsonString {
        p_ctx: None,
        z_buf: Vec::new(),
        n_alloc: 0,
        n_used: 0,
        b_static: 0,
        e_err: 0,
        z_space: [0; 100],
    };

    x.a_blob = a_blob.to_vec();
    x.n_blob = n_blob;
    json_string_init(&mut s, Some(ctx.clone()));
    json_translate_blob_to_text(&x, 0, &mut s);
    json_return_string(&mut s, None, None);
}

/// Devolve o valor do nó BLOB no índice `i`.
///
/// Se o valor é primitivo, devolve-o como valor SQL. Se é um array ou objeto,
/// devolve-o como texto JSON ou como a codificação BLOB, conforme o flag
/// JSON_B no userdata.
pub fn json_return_from_blob(
    p_parse: &JsonParse,     // Árvore de parse JSON completa
    i: u32,                  // Índice do nó
    p_ctx: &Sqlite3ContextRef, // Valor de retorno desta função
    text_only: i32,                      // Devolve JSON em texto. Ignora o userdata
) {
    let mut n: u32;
    let mut sz: u32 = 0;
    let rc: i32;
    // O C pega o `db` da conexão aqui só para as alocações; em Rust as cópias
    // abaixo não falham, então os ramos `returnfromblob_oom` não existem.

    n = jsonb_payload_size(p_parse, i, &mut sz);
    if n == 0 {
        api::result_error(p_ctx, b"malformed JSON", -1);
        return;
    }
    let mut malformed = false;
    let mut to_double = false;
    match p_parse.a_blob[i as usize] & 0x0f {
        JSONB_NULL => {
            if sz != 0 {
                malformed = true;
            } else {
                api::result_null(p_ctx);
            }
        }
        JSONB_TRUE => {
            if sz != 0 {
                malformed = true;
            } else {
                api::result_int(p_ctx, 1);
            }
        }
        JSONB_FALSE => {
            if sz != 0 {
                malformed = true;
            } else {
                api::result_int(p_ctx, 0);
            }
        }
        JSONB_INT5 | JSONB_INT => {
            let mut i_res: i64 = 0;
            let mut b_neg = false;
            if sz == 0 {
                malformed = true;
            } else {
                let x: u8 = p_parse.a_blob[(i + n) as usize];
                if x == b'-' && sz < 2 {
                    malformed = true;
                } else {
                    if x == b'-' {
                        n += 1;
                        sz -= 1;
                        b_neg = true;
                    }
                    let mut z: Vec<u8> =
                        p_parse.a_blob[(i + n) as usize..(i + n + sz) as usize].to_vec();
                    z.push(0);
                    rc = dec_or_hex_to_i64(&z, &mut i_res);
                    if rc == 0 {
                        api::result_int64(
                            p_ctx,
                            if b_neg { i_res.wrapping_neg() } else { i_res },
                        );
                    } else if rc == 3 && b_neg {
                        api::result_int64(p_ctx, SMALLEST_INT64);
                    } else if rc == 1 {
                        malformed = true;
                    } else {
                        if b_neg {
                            n -= 1;
                            sz += 1;
                        }
                        to_double = true;
                    }
                }
            }
        }
        JSONB_FLOAT5 | JSONB_FLOAT => {
            if sz == 0 {
                malformed = true;
            } else {
                to_double = true;
            }
        }
        JSONB_TEXTRAW | JSONB_TEXT => {
            let z = &p_parse.a_blob[(i + n) as usize..(i + n + sz) as usize];
            api::result_text(p_ctx, z, sz as i32, SQLITE_TRANSIENT);
        }
        JSONB_TEXT5 | JSONB_TEXTJ => {
            // Traduz a string no formato JSON para texto puro
            let mut i_in: u32;
            let n_out: u32 = sz;
            let z: &[u8] = &p_parse.a_blob[(i + n) as usize..(i + n + sz) as usize];
            let mut z_out: Vec<u8> = Vec::with_capacity(n_out as usize + 1);
            i_in = 0;
            while i_in < sz {
                let c: u8 = z[i_in as usize];
                if c == b'\\' {
                    let mut v: u32 = 0;
                    let sz_escape: u32 =
                        json_unescape_one_char(&z[i_in as usize..], sz - i_in, &mut v);
                    if v <= 0x7f {
                        z_out.push(v as u8);
                    } else if v <= 0x7ff {
                        debug_assert!(sz_escape >= 2);
                        z_out.push((0xc0 | (v >> 6)) as u8);
                        z_out.push((0x80 | (v & 0x3f)) as u8);
                    } else if v < 0x10000 {
                        debug_assert!(sz_escape >= 3);
                        z_out.push((0xe0 | (v >> 12)) as u8);
                        z_out.push((0x80 | ((v >> 6) & 0x3f)) as u8);
                        z_out.push((0x80 | (v & 0x3f)) as u8);
                    } else if v == JSON_INVALID_CHAR {
                        // Ignora silenciosamente o unicode ilegal
                    } else {
                        debug_assert!(sz_escape >= 4);
                        z_out.push((0xf0 | (v >> 18)) as u8);
                        z_out.push((0x80 | ((v >> 12) & 0x3f)) as u8);
                        z_out.push((0x80 | ((v >> 6) & 0x3f)) as u8);
                        z_out.push((0x80 | (v & 0x3f)) as u8);
                    }
                    i_in = i_in.wrapping_add(sz_escape.wrapping_sub(1));
                } else {
                    z_out.push(c);
                }
                i_in = i_in.wrapping_add(1);
            } // fim do for
            debug_assert!(z_out.len() as u32 <= n_out);
            // O C entrega o buffer com SQLITE_DYNAMIC; aqui o resultado é copiado.
            let i_out = z_out.len() as i32;
            api::result_text(p_ctx, &z_out, i_out, SQLITE_TRANSIENT);
        }
        JSONB_ARRAY | JSONB_OBJECT => {
            let flags: i32 = if text_only != 0 {
                0
            } else {
                sqlite_ptr_to_int(api::user_data(p_ctx)) as i32
            };
            let blob = &p_parse.a_blob[i as usize..(i + sz + n) as usize];
            if flags & JSON_BLOB != 0 {
                api::result_blob(
                    p_ctx,
                    blob,
                    (sz + n) as i32,
                    SQLITE_TRANSIENT,
                );
            } else {
                json_return_text_json_from_blob(p_ctx, blob, sz + n);
            }
        }
        _ => {
            malformed = true;
        }
    }
    if !malformed && to_double {
        // Rótulo to_double do C: converte o texto para double
        let mut z: Vec<u8> = p_parse.a_blob[(i + n) as usize..(i + n + sz) as usize].to_vec();
        z.push(0);
        let n_len = z.iter().position(|&c| c == 0).unwrap_or(z.len());
        let (rc, r) = ato_f(&z, n_len, SQLITE_UTF8);
        if rc <= 0 {
            malformed = true;
        } else {
            api::result_double(p_ctx, r);
        }
    }
    if malformed {
        api::result_error(p_ctx, b"malformed JSON", -1);
    }
}


// ---- part_008.rs ----

/// `p_arg` é um argumento de função que pode ser um valor SQL ou um valor
/// JSON. Descobre qual é e o codifica como um blob JSONB. O resultado fica em
/// `p_parse`.
///
/// `p_parse` não está inicializado na entrada. Esta rotina cuida da
/// inicialização. O resultado fica em `p_parse.a_blob` e `p_parse.n_blob`. No
/// C, `a_blob` podia ser alocado dinamicamente (se `n_blob_alloc` fosse maior
/// que zero), podia ser uma string estática ou um valor obtido de
/// `value_blob(p_arg)`; aqui `a_blob` é sempre um `Vec<u8>` do próprio
/// `p_parse`.
///
/// Se o argumento é um BLOB que claramente não é JSONB, a função pode gravar
/// uma mensagem de erro em `ctx` e devolver não zero. Também pode gravar um
/// erro e devolver não zero em caso de OOM.
fn json_function_arg_to_blob(
    ctx: &Rc<RefCell<sqlite3_context>>,
    p_arg: &MemRef,
    p_parse: &mut JsonParse,
) -> i32 {
    let e_type = api::value_type(&mut p_arg.borrow_mut());
    // static u8 aNull[] = { 0x00 };
    const A_NULL: [u8; 1] = [0x00];
    // memset(pParse, 0, sizeof(pParse[0]));
    *p_parse = JsonParse::default();
    p_parse.db = Some(api::context_db_handle(&ctx.borrow()));
    match e_type {
        SQLITE_BLOB => {
            if json_func_arg_might_be_binary(&mut p_arg.borrow_mut()) != 0 {
                p_parse.a_blob = api::value_blob(&mut p_arg.borrow_mut())
                    .map(|s| s.to_vec())
                    .unwrap_or_default();
                p_parse.n_blob = api::value_bytes(&mut p_arg.borrow_mut()) as u32;
            } else {
                api::result_error(&mut ctx.borrow_mut(), b"JSON cannot hold BLOB values", -1);
                return 1;
            }
        }
        SQLITE_TEXT => {
            let z_json: Option<Vec<u8>> =
                api::value_text(&mut p_arg.borrow_mut()).map(|s| s.to_vec());
            let n_json = api::value_bytes(&mut p_arg.borrow_mut());
            let z_json = match z_json {
                None => return 1,
                Some(z) => z,
            };
            if api::value_subtype(&mut p_arg.borrow_mut()) == JSON_SUBTYPE {
                p_parse.z_json = z_json;
                p_parse.n_json = n_json;
                if json_convert_text_to_blob(p_parse, Some(ctx)) != 0 {
                    api::result_error(&mut ctx.borrow_mut(), b"malformed JSON", -1);
                    // sqlite3DbFree(pParse->db, pParse->aBlob);
                    // memset(pParse, 0, sizeof(pParse[0]));
                    *p_parse = JsonParse::default();
                    return 1;
                }
            } else {
                json_blob_append_node(p_parse, JSONB_TEXTRAW, n_json as u32, Some(&z_json));
            }
        }
        SQLITE_FLOAT => {
            let r = api::value_double(&mut p_arg.borrow_mut());
            if never(is_nan(r)) {
                json_blob_append_node(p_parse, JSONB_NULL, 0, None);
            } else {
                let n = api::value_bytes(&mut p_arg.borrow_mut());
                let z: Vec<u8> = match api::value_text(&mut p_arg.borrow_mut()) {
                    None => return 1,
                    Some(z) => z.to_vec(),
                };
                // O C lê o texto terminado em NUL: fora do vetor vale 0.
                let z0 = z.first().copied().unwrap_or(0);
                let z1 = z.get(1).copied().unwrap_or(0);
                if z0 == b'I' {
                    json_blob_append_node(p_parse, JSONB_FLOAT, 5, Some(b"9e999"));
                } else if z0 == b'-' && z1 == b'I' {
                    json_blob_append_node(p_parse, JSONB_FLOAT, 6, Some(b"-9e999"));
                } else {
                    json_blob_append_node(p_parse, JSONB_FLOAT, n as u32, Some(&z));
                }
            }
        }
        SQLITE_INTEGER => {
            let n = api::value_bytes(&mut p_arg.borrow_mut());
            let z: Vec<u8> = match api::value_text(&mut p_arg.borrow_mut()) {
                None => return 1,
                Some(z) => z.to_vec(),
            };
            json_blob_append_node(p_parse, JSONB_INT, n as u32, Some(&z));
        }
        _ => {
            // default: (SQLITE_NULL)
            p_parse.a_blob = A_NULL.to_vec();
            p_parse.n_blob = 1;
            return 0;
        }
    }
    if p_parse.oom != 0 {
        api::result_error_nomem(&mut ctx.borrow_mut());
        1
    } else {
        0
    }
}

/// Gera um erro de caminho inválido.
///
/// Se `ctx` não é nulo, grava a mensagem de erro em `ctx` e devolve `None`.
/// Se `ctx` é nulo, devolve o texto da mensagem de erro.
fn json_bad_path_error<'a>(
    ctx: impl Into<Option<&'a Rc<RefCell<sqlite3_context>>>>, // A chamada de função que contém o erro
    z_path: &[u8],                                            // O caminho com o problema
) -> Option<Vec<u8>> {
    let ctx: Option<&Rc<RefCell<sqlite3_context>>> = ctx.into();
    // sqlite3_mprintf("bad JSON path: %Q", zPath): o %Q envolve o texto em
    // apóstrofos e dobra cada apóstrofo interno.
    let mut z_msg: Vec<u8> = Vec::with_capacity(z_path.len() + 20);
    z_msg.extend_from_slice(b"bad JSON path: '");
    for &c in z_path {
        if c == b'\'' {
            z_msg.push(b'\'');
        }
        z_msg.push(c);
    }
    z_msg.push(b'\'');
    let ctx = match ctx {
        None => return Some(z_msg),
        Some(ctx) => ctx,
    };
    // A alocação em Rust não falha: o ramo `zMsg==0` (result_error_nomem) não
    // existe aqui.
    api::result_error(&mut ctx.borrow_mut(), &z_msg, -1);
    None
}

/// `argv[0]` é um BLOB que parece ser um JSONB. Os argumentos seguintes vêm em
/// pares, cada um com um caminho JSON e o conteúdo a inserir ou gravar nesse
/// caminho. Faz as alterações e devolve o resultado.
///
/// A operação específica é determinada por `e_edit`, que pode ser JEDIT_INS,
/// JEDIT_REPL ou JEDIT_SET.
fn json_insert_into_blob(
    ctx: &Rc<RefCell<sqlite3_context>>,
    argc: i32,
    argv: &[MemRef],
    e_edit: u8, // JEDIT_INS, JEDIT_REPL ou JEDIT_SET
) {
    let mut rc: u32 = 0;
    let mut z_path: Vec<u8> = Vec::new();
    let mut path_error = false;

    debug_assert!((argc & 1) == 1);
    let flgs: u32 = if argc == 1 { 0 } else { JSON_EDITABLE };
    let p = match json_parse_func_arg(ctx, &argv[0], flgs) {
        None => return,
        Some(p) => p,
    };
    let mut i: i32 = 1;
    while i < argc - 1 {
        let cur = i as usize;
        i += 2;
        if api::value_type(&mut argv[cur].borrow_mut()) == SQLITE_NULL {
            continue;
        }
        let z_text: Option<Vec<u8>> =
            api::value_text(&mut argv[cur].borrow_mut()).map(|s| s.to_vec());
        match z_text {
            None => {
                api::result_error_nomem(&mut ctx.borrow_mut());
                json_parse_free(Some(p));
                return;
            }
            Some(mut z) => {
                // O C lê o caminho como string terminada em NUL.
                if let Some(pos) = z.iter().position(|&c| c == 0) {
                    z.truncate(pos);
                }
                z_path = z;
            }
        }
        if z_path.first().copied().unwrap_or(0) != b'$' {
            path_error = true; // goto jsonInsertIntoBlob_patherror
            break;
        }
        let mut ax = JsonParse::default();
        if json_function_arg_to_blob(ctx, &argv[cur + 1], &mut ax) != 0 {
            json_parse_reset(&mut ax);
            json_parse_free(Some(p));
            return;
        }
        if z_path.get(1).copied().unwrap_or(0) == 0 {
            if e_edit == JEDIT_REPL || e_edit == JEDIT_SET {
                let mut pb = p.borrow_mut();
                let n_blob = pb.n_blob;
                json_blob_edit(
                    &mut pb,
                    0,
                    n_blob,
                    Some(&ax.a_blob[..ax.n_blob as usize]),
                    ax.n_blob,
                );
            }
            rc = 0;
        } else {
            let mut pb = p.borrow_mut();
            pb.e_edit = e_edit;
            pb.n_ins = ax.n_blob;
            pb.a_ins = ax.a_blob[..ax.n_blob as usize].to_vec();
            pb.delta = 0;
            rc = json_lookup_step(&mut pb, 0, &z_path[1..], 0);
        }
        json_parse_reset(&mut ax);
        if rc == JSON_LOOKUP_NOTFOUND {
            continue;
        }
        if json_lookup_iserror(rc) {
            path_error = true; // goto jsonInsertIntoBlob_patherror
            break;
        }
    }
    if !path_error {
        json_return_parse(ctx, &p);
        json_parse_free(Some(p));
        return;
    }

    // jsonInsertIntoBlob_patherror:
    json_parse_free(Some(p));
    if rc == JSON_LOOKUP_ERROR {
        api::result_error(&mut ctx.borrow_mut(), b"malformed JSON", -1);
    } else {
        json_bad_path_error(ctx, &z_path);
    }
}

/// Se `p_arg` é um blob que parece um blob JSONB, inicializa `p` para apontar
/// para esse JSONB e devolve VERDADEIRO. Se `p_arg` não parece um blob JSONB,
/// devolve FALSO.
///
/// Esta rotina só é chamada quando já se sabe que `p_arg` é um blob. A única
/// dúvida em aberto é se o blob parece ser um blob JSONB.
fn json_arg_is_jsonb(p_arg: &MemRef, p: &mut JsonParse) -> i32 {
    let mut n: u32;
    let mut sz: u32 = 0;
    p.a_blob = api::value_blob(&mut p_arg.borrow_mut())
        .map(|s| s.to_vec())
        .unwrap_or_default();
    p.n_blob = api::value_bytes(&mut p_arg.borrow_mut()) as u32;
    if p.n_blob == 0 {
        p.a_blob = Vec::new();
        return 0;
    }
    if never(p.a_blob.is_empty()) {
        return 0;
    }
    if (p.a_blob[0] & 0x0f) <= JSONB_OBJECT && {
        n = jsonb_payload_size(p, 0, &mut sz);
        n > 0
    } && sz.wrapping_add(n) == p.n_blob
        && ((p.a_blob[0] & 0x0f) > JSONB_FALSE || sz == 0)
    {
        return 1;
    }
    p.a_blob = Vec::new();
    p.n_blob = 0;
    0
}

/// Gera um objeto JsonParse, contendo JSONB válido em `a_blob` e `n_blob`, a
/// partir do argumento de função SQL `p_arg`. Devolve o novo objeto JsonParse.
///
/// A posse do novo objeto JsonParse passa para quem chamou. Quem chamou deve
/// invocar `json_parse_free()` sobre o valor devolvido quando terminar de
/// usá-lo.
///
/// Se algum erro for detectado, uma mensagem de erro apropriada é gravada com
/// `result_error()` ou equivalente e esta rotina devolve `None`. Também
/// devolve `None` se `p_arg` é um valor SQL NULL, mas sem mensagem de erro.
/// Assim as funções SQL que recebem argumentos NULL devolvem NULL.
fn json_parse_func_arg(
    ctx: &Rc<RefCell<sqlite3_context>>,
    p_arg: &MemRef,
    flgs: u32,
) -> Option<JsonParseRef> {
    let e_type = api::value_type(&mut p_arg.borrow_mut()); // Tipo de dado de p_arg
    let mut p_from_cache: Option<JsonParseRef>; // Valor tirado do cache

    if e_type == SQLITE_NULL {
        return None;
    }
    p_from_cache = json_cache_search(&ctx.borrow(), p_arg);
    if let Some(pc) = &p_from_cache {
        pc.borrow_mut().n_jp_ref += 1;
        if (flgs & JSON_EDITABLE) == 0 {
            return p_from_cache;
        }
    }
    let db: Option<Sqlite3Ref> = Some(api::context_db_handle(&ctx.borrow())); // A conexão com o banco

    // json_pfa_oom: libera o que existe e informa falta de memória.
    let pfa_oom = |p_from_cache: Option<JsonParseRef>, p: Option<JsonParseRef>| -> Option<JsonParseRef> {
        json_parse_free(p_from_cache);
        json_parse_free(p);
        api::result_error_nomem(&mut ctx.borrow_mut());
        None
    };
    // json_pfa_malformed
    let pfa_malformed = |p: JsonParseRef| -> Option<JsonParseRef> {
        if (flgs & JSON_KEEPERROR) != 0 {
            p.borrow_mut().n_err = 1;
            Some(p)
        } else {
            json_parse_free(Some(p));
            api::result_error(&mut ctx.borrow_mut(), b"malformed JSON", -1);
            None
        }
    };

    // rebuild_from_cache:
    loop {
        // sqlite3DbMallocZero + memset: a alocação em Rust não falha.
        let p: JsonParseRef = Rc::new(RefCell::new(JsonParse::default()));
        {
            let mut pb = p.borrow_mut();
            pb.db = db.clone();
            pb.n_jp_ref = 1;
        }
        if let Some(pc) = p_from_cache.take() {
            {
                let src = pc.borrow();
                let n_blob = src.n_blob;
                let mut pb = p.borrow_mut();
                pb.a_blob = src.a_blob[..n_blob as usize].to_vec();
                pb.n_blob = n_blob;
                pb.n_blob_alloc = n_blob;
                pb.has_nonstd = src.has_nonstd;
            }
            json_parse_free(Some(pc));
            return Some(p);
        }
        if e_type == SQLITE_BLOB {
            if json_arg_is_jsonb(p_arg, &mut p.borrow_mut()) != 0 {
                if (flgs & JSON_EDITABLE) != 0
                    && json_blob_make_editable(&mut p.borrow_mut(), 0) == 0
                {
                    return pfa_oom(p_from_cache.take(), Some(p));
                }
                return Some(p);
            }
            // Se o blob não é JSONB válido, cai na tentativa de convertê-lo em
            // texto, que então é interpretado como JSON. (tag-20240123-a)
            //
            // Isso contraria toda a documentação histórica sobre como as
            // funções JSON do SQLite deviam funcionar. Desde o início, blob
            // era reservado para expansão e um valor blob deveria gerar erro.
            // Mas não gerava, por causa de um bug. E muitas aplicações passaram
            // a depender desse comportamento, especialmente usando a CLI e
            // lendo texto JSON com readfile(), que devolve um blob. Por isso o
            // bug continua sendo suportado daqui para a frente.
            // Ver por exemplo https://sqlite.org/forum/forumpost/012136abd5292b8d
        }
        let z_json: Vec<u8> = api::value_text(&mut p_arg.borrow_mut())
            .map(|s| s.to_vec())
            .unwrap_or_default();
        let n_json = api::value_bytes(&mut p_arg.borrow_mut());
        {
            let mut pb = p.borrow_mut();
            pb.z_json = z_json;
            pb.n_json = n_json;
        }
        if db.as_ref().map_or(false, |d| d.borrow().malloc_failed != 0) {
            return pfa_oom(p_from_cache.take(), Some(p));
        }
        if n_json == 0 {
            return pfa_malformed(p);
        }
        debug_assert!(!p.borrow().z_json.is_empty());
        let conv_ctx = if (flgs & JSON_KEEPERROR) != 0 { None } else { Some(ctx) };
        if json_convert_text_to_blob(&mut p.borrow_mut(), conv_ctx) != 0 {
            if (flgs & JSON_KEEPERROR) != 0 {
                p.borrow_mut().n_err = 1;
                return Some(p);
            } else {
                json_parse_free(Some(p));
                return None;
            }
        } else {
            let is_rc_str = value_is_of_class(&p_arg.borrow(), rc_str_unref);
            if is_rc_str == 0 {
                let n = p.borrow().n_json as usize;
                let mut z_new = match rc_str_new(n as u64) {
                    None => return pfa_oom(p_from_cache.take(), Some(p)),
                    Some(z) => z,
                };
                if z_new.len() < n + 1 {
                    z_new.resize(n + 1, 0);
                }
                {
                    let mut pb = p.borrow_mut();
                    z_new[..n].copy_from_slice(&pb.z_json[..n]);
                    pb.z_json = z_new;
                    pb.z_json[n] = 0;
                }
            } else {
                rc_str_ref(&mut p.borrow_mut().z_json);
            }
            p.borrow_mut().b_json_is_rc_str = 1;
            let rc = json_cache_insert(&mut ctx.borrow_mut(), &p);
            if rc == SQLITE_NOMEM {
                return pfa_oom(p_from_cache.take(), Some(p));
            }
            if (flgs & JSON_EDITABLE) != 0 {
                p_from_cache = Some(p);
                continue; // goto rebuild_from_cache
            }
        }
        return Some(p);
    }
}

/// Faz o valor de retorno de uma função JSON ser o blob JSONB cru ou o texto
/// JSON, conforme o flag JSON_BLOB esteja ou não ligado na função.
fn json_return_parse(ctx: &Rc<RefCell<sqlite3_context>>, p: &JsonParseRef) {
    if p.borrow().oom != 0 {
        api::result_error_nomem(&mut ctx.borrow_mut());
        return;
    }
    let flgs: i32 = ptr_to_int(api::user_data(&ctx.borrow())) as i32;
    if (flgs & JSON_BLOB) != 0 {
        let mut pb = p.borrow_mut();
        let n_blob = pb.n_blob as usize;
        if pb.n_blob_alloc > 0 && pb.b_read_only == 0 {
            // SQLITE_DYNAMIC: o resultado fica com o conteúdo e `p` deixa de
            // ser o dono (em Rust o conteúdo é copiado para o resultado).
            api::result_blob(&mut ctx.borrow_mut(), Some(&pb.a_blob[..n_blob]), n_blob as i32, SQLITE_TRANSIENT);
            pb.n_blob_alloc = 0;
        } else {
            api::result_blob(&mut ctx.borrow_mut(), Some(&pb.a_blob[..n_blob]), n_blob as i32, SQLITE_TRANSIENT);
        }
    } else {
        let mut s = json_string_blank();
        json_string_init(&mut s, Some(ctx.clone()));
        p.borrow_mut().delta = 0;
        {
            let pb = p.borrow();
            json_translate_blob_to_text(&pb, 0, &mut s);
        }
        json_return_string(&mut s, Some(p), Some(ctx));
        api::result_subtype(&mut ctx.borrow_mut(), JSON_SUBTYPE);
    }
}


// ---- part_009.rs ----

// ************************************************************************
// Funções SQL usadas para teste e depuração
// ************************************************************************
//
// `json_debug_print_blob()`, `json_show_parse()` e `json_parse_func()` só
// existem sob `SQLITE_DEBUG`, que o Debian 13 não ativa: não são traduzidas.

/// Cria um `JsonString` zerado, ainda não inicializado por
/// `json_string_init()`. É o equivalente da declaração `JsonString jx;` do C,
/// que deixa a memória sem valor até o `jsonStringInit()`.
fn json_string_blank() -> JsonString {
    JsonString {
        p_ctx: None,
        z_buf: Vec::new(),
        n_alloc: 0,
        n_used: 0,
        b_static: 0,
        e_err: 0,
        z_space: [0u8; 100],
    }
}

/// Copia o texto de um `sqlite3_value` como o C o lê: uma string terminada em
/// NUL. Devolve `None` quando `sqlite3_value_text()` devolveria NULL.
fn json_value_path(p_value: &MemRef) -> Option<Vec<u8>> {
    let mut z: Vec<u8> = api::value_text(&mut p_value.borrow_mut())?.to_vec();
    if let Some(pos) = z.iter().position(|&c| c == 0) {
        z.truncate(pos);
    }
    Some(z)
}

// ************************************************************************
// Implementações das funções SQL escalares
// ************************************************************************

/// Implementação da função json_quote(VALUE). Devolve um valor JSON
/// correspondente ao valor SQL de entrada. Na prática, isso significa pôr
/// aspas duplas nas strings e devolver a string sem aspas "null" quando a
/// entrada é NULL.
pub fn json_quote_func(ctx: &Rc<RefCell<sqlite3_context>>, _argc: i32, argv: &[MemRef]) {
    let mut jx = json_string_blank();

    json_string_init(&mut jx, Some(ctx.clone()));
    json_append_sql_value(&mut jx, &mut argv[0].borrow_mut());
    json_return_string(&mut jx, None, None);
    api::result_subtype(&mut ctx.borrow_mut(), JSON_SUBTYPE);
}

/// Implementação da função json_array(VALUE,...). Devolve um array JSON com
/// todos os valores dados nos argumentos. Se algum argumento for um BLOB,
/// gera erro.
pub fn json_array_func(ctx: &Rc<RefCell<sqlite3_context>>, argc: i32, argv: &[MemRef]) {
    let mut jx = json_string_blank();

    json_string_init(&mut jx, Some(ctx.clone()));
    json_append_char(&mut jx, b'[');
    let mut i: i32 = 0;
    while i < argc {
        json_append_separator(&mut jx);
        json_append_sql_value(&mut jx, &mut argv[i as usize].borrow_mut());
        i += 1;
    }
    json_append_char(&mut jx, b']');
    json_return_string(&mut jx, None, None);
    api::result_subtype(&mut ctx.borrow_mut(), JSON_SUBTYPE);
}

/// json_array_length(JSON)
/// json_array_length(JSON, PATH)
///
/// Devolve o número de elementos do array JSON de nível superior. Devolve 0
/// se a entrada não for um array JSON bem formado.
pub fn json_array_length_func(
    ctx: &Rc<RefCell<sqlite3_context>>,
    argc: i32,
    argv: &[MemRef],
) {
    let mut cnt: i64 = 0;
    let mut i: u32;
    let mut e_err: u8 = 0;

    let p = match json_parse_func_arg(ctx, &argv[0], 0) {
        Some(p) => p,
        None => return,
    };
    if argc == 2 {
        let z_path = match json_value_path(&argv[1]) {
            Some(z) => z,
            None => {
                json_parse_free(Some(p));
                return;
            }
        };
        let z0 = z_path.first().copied().unwrap_or(0);
        i = json_lookup_step(
            &mut p.borrow_mut(),
            0,
            if z0 == b'$' { &z_path[1..] } else { b"@" },
            0,
        );
        if json_lookup_iserror(i) {
            if i == JSON_LOOKUP_NOTFOUND {
                // no-op
            } else if i == JSON_LOOKUP_PATHERROR {
                json_bad_path_error(ctx, &z_path);
            } else {
                api::result_error(&mut ctx.borrow_mut(), b"malformed JSON", -1);
            }
            e_err = 1;
            i = 0;
        }
    } else {
        i = 0;
    }
    if (p.borrow().a_blob[i as usize] & 0x0f) == JSONB_ARRAY {
        cnt = jsonb_array_count(&mut p.borrow_mut(), i) as i64;
    }
    if e_err == 0 {
        api::result_int64(&mut ctx.borrow_mut(), cnt);
    }
    json_parse_free(Some(p));
}

/// Verdadeiro se a string tem só alfanuméricos e sublinhados.
fn json_all_alphanum(z: &[u8], n: i32) -> i32 {
    let mut i: i32 = 0;
    while i < n && (isalnum(z[i as usize]) || z[i as usize] == b'_') {
        i += 1;
    }
    (i == n) as i32
}

/// json_extract(JSON, PATH, ...)
/// "->"(JSON,PATH)
/// "->>"(JSON,PATH)
///
/// Devolve o elemento descrito por PATH. Devolve NULL se esse elemento do
/// caminho não for encontrado.
///
/// Se JSON_JSON está ligado, ou se mais de um argumento PATH é dado, o
/// resultado é sempre uma representação JSON. Se JSON_SQL está ligado, o
/// resultado é sempre uma representação SQL. Se nenhum dos dois está ligado e
/// argc==2, devolve JSON para objetos e arrays e SQL para os demais valores.
///
/// Quando vários argumentos PATH são dados, o resultado é um array JSON com o
/// resultado de cada PATH.
///
/// Caminhos JSON abreviados são aceitos se JSON_ABPATH, por compatibilidade
/// com o PG.
pub fn json_extract_func(ctx: &Rc<RefCell<sqlite3_context>>, argc: i32, argv: &[MemRef]) {
    let mut jx = json_string_blank(); // String para o resultado em array

    if argc < 2 {
        return;
    }
    let p = match json_parse_func_arg(ctx, &argv[0], 0) {
        Some(p) => p,
        None => return,
    };
    let flags: i32 = ptr_to_int(api::user_data(&ctx.borrow())) as i32;
    json_string_init(&mut jx, Some(ctx.clone()));
    if argc > 2 {
        json_append_char(&mut jx, b'[');
    }
    // O rótulo `json_extract_error` do C: `break 'body` leva à limpeza final.
    'body: {
        let mut i: i32 = 1;
        while i < argc {
            // Com um único argumento PATH
            let z_path = match json_value_path(&argv[i as usize]) {
                Some(z) => z,
                None => break 'body,
            };
            let n_path: i32 = strlen30(&z_path);
            let z0 = z_path.first().copied().unwrap_or(0);
            let j: u32;
            if z0 == b'$' {
                j = json_lookup_step(&mut p.borrow_mut(), 0, &z_path[1..], 0);
            } else if (flags & JSON_ABPATH) != 0 {
                // Os operadores -> e ->> aceitam argumentos PATH abreviados.
                // Isso é principalmente por compatibilidade com o
                // PostgreSQL, mas também por conveniência.
                //
                //     NÚMERO   ==>  $[NÚMERO]     // compatível com o PG
                //     RÓTULO   ==>  $.RÓTULO      // compatível com o PG
                //     [NÚMERO] ==>  $[NÚMERO]     // Não é do PG. Só conveniência
                json_string_init(&mut jx, Some(ctx.clone()));
                if api::value_type(&mut argv[i as usize].borrow_mut()) == SQLITE_INTEGER {
                    json_append_raw_nz(&mut jx, b"[", 1);
                    json_append_raw(&mut jx, &z_path, n_path as u32);
                    // No C a chamada passa n=2 sobre "]", lendo o NUL final.
                    json_append_raw_nz(&mut jx, b"]\0", 2);
                } else if json_all_alphanum(&z_path, n_path) != 0 {
                    json_append_raw_nz(&mut jx, b".", 1);
                    json_append_raw(&mut jx, &z_path, n_path as u32);
                } else if z0 == b'[' && n_path >= 3 && z_path[(n_path - 1) as usize] == b']' {
                    json_append_raw(&mut jx, &z_path, n_path as u32);
                } else {
                    json_append_raw_nz(&mut jx, b".\"", 2);
                    json_append_raw(&mut jx, &z_path, n_path as u32);
                    json_append_raw_nz(&mut jx, b"\"", 1);
                }
                json_string_terminate(&mut jx);
                let z_ab: Vec<u8> = jx.z_buf[..jx.n_used as usize].to_vec();
                j = json_lookup_step(&mut p.borrow_mut(), 0, &z_ab, 0);
                json_string_reset(&mut jx);
            } else {
                json_bad_path_error(ctx, &z_path);
                break 'body;
            }
            let n_blob = p.borrow().n_blob;
            if j < n_blob {
                if argc == 2 {
                    if (flags & JSON_JSON) != 0 {
                        json_string_init(&mut jx, Some(ctx.clone()));
                        json_translate_blob_to_text(&mut p.borrow_mut(), j, &mut jx);
                        json_return_string(&mut jx, None, None);
                        json_string_reset(&mut jx);
                        debug_assert!((flags & JSON_BLOB) == 0);
                        api::result_subtype(&mut ctx.borrow_mut(), JSON_SUBTYPE);
                    } else {
                        json_return_from_blob(&mut p.borrow_mut(), j, ctx, 0);
                        if (flags & (JSON_SQL | JSON_BLOB)) == 0
                            && (p.borrow().a_blob[j as usize] & 0x0f) >= JSONB_ARRAY
                        {
                            api::result_subtype(&mut ctx.borrow_mut(), JSON_SUBTYPE);
                        }
                    }
                } else {
                    json_append_separator(&mut jx);
                    json_translate_blob_to_text(&mut p.borrow_mut(), j, &mut jx);
                }
            } else if j == JSON_LOOKUP_NOTFOUND {
                if argc == 2 {
                    break 'body; // Devolve NULL se não achou
                } else {
                    json_append_separator(&mut jx);
                    json_append_raw_nz(&mut jx, b"null", 4);
                }
            } else if j == JSON_LOOKUP_ERROR {
                api::result_error(&mut ctx.borrow_mut(), b"malformed JSON", -1);
                break 'body;
            } else {
                json_bad_path_error(ctx, &z_path);
                break 'body;
            }
            i += 1;
        }
        if argc > 2 {
            json_append_char(&mut jx, b']');
            json_return_string(&mut jx, None, None);
            if (flags & JSON_BLOB) == 0 {
                api::result_subtype(&mut ctx.borrow_mut(), JSON_SUBTYPE);
            }
        }
    }
    // json_extract_error:
    json_string_reset(&mut jx);
    json_parse_free(Some(p));
}


// ---- part_010.rs ----

// Códigos de retorno de json_merge_patch().
pub const JSON_MERGE_OK: i32 = 0; // sucesso
pub const JSON_MERGE_BADTARGET: i32 = 1; // blob TARGET malformado
pub const JSON_MERGE_BADPATCH: i32 = 2; // blob PATCH malformado
pub const JSON_MERGE_OOM: i32 = 3; // falta de memória

/// MergePatch da RFC 7396 para dois blobs JSONB.
///
/// `p_target` é o alvo e `p_patch` é o remendo. O alvo é atualizado no lugar.
/// O remendo é somente leitura.
///
/// O algoritmo original da RFC 7396 é este:
///
/// ```text
///   define MergePatch(Target, Patch):
///     if Patch is an Object:
///       if Target is not an Object:
///         Target = {} # Ignore the contents and set it to an empty Object
///     for each Name/Value pair in Patch:
///         if Value is null:
///           if Name exists in Target:
///             remove the Name/Value pair from Target
///         else:
///           Target[Name] = MergePatch(Target[Name], Value)
///       return Target
///     else:
///       return Patch
/// ```
///
/// Aqui está um algoritmo equivalente, reestruturado para mostrar a
/// implementação de fato:
///
/// ```text
/// 01   define MergePatch(Target, Patch):
/// 02      if Patch is not an Object:
/// 03         return Patch
/// 04      else: // if Patch is an Object
/// 05         if Target is not an Object:
/// 06            Target = {}
/// 07      for each Name/Value pair in Patch:
/// 08         if Name exists in Target:
/// 09            if Value is null:
/// 10               remove the Name/Value pair from Target
/// 11            else
/// 12               Target[name] = MergePatch(Target[Name], Value)
/// 13         else if Value is not NULL:
/// 14            if Value is not an Object:
/// 15               Target[name] = Value
/// 16            else:
/// 17               Target[name] = MergePatch('{}',value)
/// 18      return Target
/// ```
///
/// Os números de linha acima são citados nos comentários da implementação.
fn json_merge_patch(
    p_target: &mut JsonParse, // O parser JSON que contém o TARGET
    i_target: u32,            // Índice do TARGET em p_target.a_blob[]
    p_patch: &JsonParse,      // O PATCH
    i_patch: u32,             // Índice do PATCH em p_patch.a_blob[]
) -> i32 {
    let mut x: u8; // Tipo de um único nó
    let mut n: u32; // Valores de retorno de jsonb_payload_size()
    let mut sz: u32 = 0;
    let mut i_t_cursor: u32; // Posição do cursor ao varrer o objeto alvo
    let i_t_start: u32; // Primeiro rótulo do objeto alvo
    let i_t_end_be: u32; // Primeiro byte após o fim do alvo, original, antes da edição
    let mut i_t_end: u32; // Primeiro byte após o fim do alvo, atual
    let mut e_t_label: u8; // Tipo de nó do rótulo do alvo
    let mut i_t_label: u32 = 0; // Índice do rótulo
    let mut n_t_label: u32 = 0; // Tamanho do cabeçalho do rótulo do alvo, em bytes
    let mut sz_t_label: u32 = 0; // Tamanho do payload do rótulo do alvo
    let mut i_t_value: u32 = 0; // Índice do valor do alvo
    let mut n_t_value: u32 = 0; // Tamanho do cabeçalho do valor do alvo
    let mut sz_t_value: u32 = 0; // Tamanho do payload do valor do alvo

    let mut i_p_cursor: u32; // Posição do cursor ao varrer o remendo
    let i_p_end: u32; // Primeiro byte após o fim do remendo
    let mut e_p_label: u8; // Tipo de nó do rótulo do remendo
    let mut i_p_label: u32; // Início do rótulo do remendo
    let mut n_p_label: u32; // Tamanho do cabeçalho do rótulo do remendo
    let mut sz_p_label: u32 = 0; // Tamanho do payload do rótulo do remendo
    let mut i_p_value: u32; // Início do valor do remendo
    let mut n_p_value: u32; // Tamanho do cabeçalho do valor do remendo
    let mut sz_p_value: u32 = 0; // Tamanho do payload do valor do remendo

    debug_assert!(i_target < p_target.n_blob);
    debug_assert!(i_patch < p_patch.n_blob);
    x = p_patch.a_blob[i_patch as usize] & 0x0f;
    if x != JSONB_OBJECT {
        // Algoritmo, linha 02
        let sz_patch: u32; // Tamanho total do remendo, cabeçalho mais payload
        let sz_target: u32; // Tamanho total do alvo, cabeçalho mais payload
        n = jsonb_payload_size(p_patch, i_patch, &mut sz);
        sz_patch = n.wrapping_add(sz);
        sz = 0;
        n = jsonb_payload_size(p_target, i_target, &mut sz);
        sz_target = n.wrapping_add(sz);
        json_blob_edit(
            p_target,
            i_target,
            sz_target,
            Some(&p_patch.a_blob[i_patch as usize..(i_patch + sz_patch) as usize]),
            sz_patch,
        );
        return if p_target.oom != 0 { JSON_MERGE_OOM } else { JSON_MERGE_OK }; // Linha 03
    }
    x = p_target.a_blob[i_target as usize] & 0x0f;
    if x != JSONB_OBJECT {
        // Algoritmo, linha 05
        n = jsonb_payload_size(p_target, i_target, &mut sz);
        json_blob_edit(p_target, i_target.wrapping_add(n), sz, None, 0);
        x = p_target.a_blob[i_target as usize];
        p_target.a_blob[i_target as usize] = (x & 0xf0) | JSONB_OBJECT;
    }
    n = jsonb_payload_size(p_patch, i_patch, &mut sz);
    if n == 0 {
        return JSON_MERGE_BADPATCH;
    }
    i_p_cursor = i_patch.wrapping_add(n);
    i_p_end = i_p_cursor.wrapping_add(sz);
    n = jsonb_payload_size(p_target, i_target, &mut sz);
    if n == 0 {
        return JSON_MERGE_BADTARGET;
    }
    i_t_start = i_target.wrapping_add(n);
    i_t_end_be = i_t_start.wrapping_add(sz);

    while i_p_cursor < i_p_end {
        // Algoritmo, linha 07
        i_p_label = i_p_cursor;
        e_p_label = p_patch.a_blob[i_p_cursor as usize] & 0x0f;
        if e_p_label < JSONB_TEXT || e_p_label > JSONB_TEXTRAW {
            return JSON_MERGE_BADPATCH;
        }
        n_p_label = jsonb_payload_size(p_patch, i_p_cursor, &mut sz_p_label);
        if n_p_label == 0 {
            return JSON_MERGE_BADPATCH;
        }
        i_p_value = i_p_cursor.wrapping_add(n_p_label).wrapping_add(sz_p_label);
        if i_p_value >= i_p_end {
            return JSON_MERGE_BADPATCH;
        }
        n_p_value = jsonb_payload_size(p_patch, i_p_value, &mut sz_p_value);
        if n_p_value == 0 {
            return JSON_MERGE_BADPATCH;
        }
        i_p_cursor = i_p_value.wrapping_add(n_p_value).wrapping_add(sz_p_value);
        if i_p_cursor > i_p_end {
            return JSON_MERGE_BADPATCH;
        }

        i_t_cursor = i_t_start;
        i_t_end = i_t_end_be.wrapping_add(p_target.delta as u32);
        while i_t_cursor < i_t_end {
            let is_equal: i32; // verdadeiro se os rótulos do remendo e do alvo casam
            i_t_label = i_t_cursor;
            e_t_label = p_target.a_blob[i_t_cursor as usize] & 0x0f;
            if e_t_label < JSONB_TEXT || e_t_label > JSONB_TEXTRAW {
                return JSON_MERGE_BADTARGET;
            }
            n_t_label = jsonb_payload_size(p_target, i_t_cursor, &mut sz_t_label);
            if n_t_label == 0 {
                return JSON_MERGE_BADTARGET;
            }
            i_t_value = i_t_label.wrapping_add(n_t_label).wrapping_add(sz_t_label);
            if i_t_value >= i_t_end {
                return JSON_MERGE_BADTARGET;
            }
            n_t_value = jsonb_payload_size(p_target, i_t_value, &mut sz_t_value);
            if n_t_value == 0 {
                return JSON_MERGE_BADTARGET;
            }
            if i_t_value.wrapping_add(n_t_value).wrapping_add(sz_t_value) > i_t_end {
                return JSON_MERGE_BADTARGET;
            }
            let p_label_start = (i_p_label + n_p_label) as usize;
            let t_label_start = (i_t_label + n_t_label) as usize;
            is_equal = json_label_compare(
                &p_patch.a_blob[p_label_start..p_label_start + sz_p_label as usize],
                sz_p_label,
                (e_p_label == JSONB_TEXT || e_p_label == JSONB_TEXTRAW) as i32,
                &p_target.a_blob[t_label_start..t_label_start + sz_t_label as usize],
                sz_t_label,
                (e_t_label == JSONB_TEXT || e_t_label == JSONB_TEXTRAW) as i32,
            );
            if is_equal != 0 {
                break;
            }
            i_t_cursor = i_t_value.wrapping_add(n_t_value).wrapping_add(sz_t_value);
        }
        x = p_patch.a_blob[i_p_value as usize] & 0x0f;
        if i_t_cursor < i_t_end {
            // Um casamento foi encontrado. Algoritmo, linha 08
            if x == 0 {
                // O valor do remendo é NULL. Algoritmo, linha 09
                json_blob_edit(
                    p_target,
                    i_t_label,
                    n_t_label
                        .wrapping_add(sz_t_label)
                        .wrapping_add(n_t_value)
                        .wrapping_add(sz_t_value),
                    None,
                    0,
                );
                // Não há OOM numa edição que só apaga
                if p_target.oom != 0 {
                    return JSON_MERGE_OOM;
                }
            } else {
                // Algoritmo, linha 12
                let rc: i32;
                let saved_delta: i32 = p_target.delta;
                p_target.delta = 0;
                rc = json_merge_patch(p_target, i_t_value, p_patch, i_p_value);
                if rc != 0 {
                    return rc;
                }
                p_target.delta = p_target.delta.wrapping_add(saved_delta);
            }
        } else if x > 0 {
            // Algoritmo, linha 13
            // Sem casamento e o valor do remendo não é NULL
            let sz_new: u32 = sz_p_label.wrapping_add(n_p_label);
            if (p_patch.a_blob[i_p_value as usize] & 0x0f) != JSONB_OBJECT {
                // Linha 14
                json_blob_edit(
                    p_target,
                    i_t_end,
                    0,
                    None,
                    sz_p_value.wrapping_add(n_p_value).wrapping_add(sz_new),
                );
                if p_target.oom != 0 {
                    return JSON_MERGE_OOM;
                }
                let dst = i_t_end as usize;
                let src_label = i_p_label as usize;
                p_target.a_blob[dst..dst + sz_new as usize]
                    .copy_from_slice(&p_patch.a_blob[src_label..src_label + sz_new as usize]);
                let dst_value = (i_t_end + sz_new) as usize;
                let src_value = i_p_value as usize;
                let n_value = (sz_p_value + n_p_value) as usize;
                p_target.a_blob[dst_value..dst_value + n_value]
                    .copy_from_slice(&p_patch.a_blob[src_value..src_value + n_value]);
            } else {
                let rc: i32;
                let saved_delta: i32;
                json_blob_edit(p_target, i_t_end, 0, None, sz_new.wrapping_add(1));
                if p_target.oom != 0 {
                    return JSON_MERGE_OOM;
                }
                let dst = i_t_end as usize;
                let src_label = i_p_label as usize;
                p_target.a_blob[dst..dst + sz_new as usize]
                    .copy_from_slice(&p_patch.a_blob[src_label..src_label + sz_new as usize]);
                p_target.a_blob[(i_t_end + sz_new) as usize] = 0x00;
                saved_delta = p_target.delta;
                p_target.delta = 0;
                rc = json_merge_patch(p_target, i_t_end.wrapping_add(sz_new), p_patch, i_p_value);
                if rc != 0 {
                    return rc;
                }
                p_target.delta = p_target.delta.wrapping_add(saved_delta);
            }
        }
    }
    if p_target.delta != 0 {
        json_after_edit_size_adjust(p_target, i_target);
    }
    if p_target.oom != 0 { JSON_MERGE_OOM } else { JSON_MERGE_OK }
}

/// Implementação da função json_mergepatch(JSON1,JSON2). Devolve um objeto
/// JSON que é o resultado de rodar o algoritmo MergePatch() da RFC 7396 nos
/// dois argumentos.
fn json_patch_func(ctx: &Rc<RefCell<sqlite3_context>>, argc: i32, argv: &[MemRef]) {
    let p_target: Option<JsonParseRef>; // O TARGET
    let p_patch: Option<JsonParseRef>; // O PATCH
    let rc: i32; // Código de resultado

    debug_assert!(argc == 2);
    let _ = argc;
    p_target = json_parse_func_arg(ctx, &argv[0], JSON_EDITABLE);
    let p_target = match p_target {
        Some(p) => p,
        None => return,
    };
    p_patch = json_parse_func_arg(ctx, &argv[1], 0);
    if let Some(p_patch) = p_patch {
        rc = {
            let mut t = p_target.borrow_mut();
            let pa = p_patch.borrow();
            json_merge_patch(&mut t, 0, &pa, 0)
        };
        if rc == JSON_MERGE_OK {
            json_return_parse(ctx, &p_target);
        } else if rc == JSON_MERGE_OOM {
            api::result_error_nomem(&mut ctx.borrow_mut());
        } else {
            api::result_error(&mut ctx.borrow_mut(), b"malformed JSON", -1);
        }
        json_parse_free(Some(p_patch));
    }
    json_parse_free(Some(p_target));
}

/// Implementação da função json_object(NAME,VALUE,...). Devolve um objeto
/// JSON que contém todos os pares nome/valor dados nos argumentos. Se algum
/// nome não é string, ou se algum valor é um BLOB, gera erro.
fn json_object_func(ctx: &Rc<RefCell<sqlite3_context>>, argc: i32, argv: &[MemRef]) {
    let mut i: i32;
    let mut jx = JsonString {
        p_ctx: None,
        z_buf: Vec::new(),
        n_alloc: 0,
        n_used: 0,
        b_static: 0,
        e_err: 0,
        z_space: [0u8; 100],
    };

    if argc & 1 != 0 {
        api::result_error(
            &mut ctx.borrow_mut(),
            b"json_object() requires an even number of arguments",
            -1,
        );
        return;
    }
    json_string_init(&mut jx, Some(ctx.clone()));
    json_append_char(&mut jx, b'{');
    i = 0;
    while i < argc {
        if api::value_type(&mut argv[i as usize].borrow_mut()) != SQLITE_TEXT {
            api::result_error(&mut ctx.borrow_mut(), b"json_object() labels must be TEXT", -1);
            json_string_reset(&mut jx);
            return;
        }
        json_append_separator(&mut jx);
        let z_label: Option<Vec<u8>>;
        let n_label: u32;
        {
            let mut m = argv[i as usize].borrow_mut();
            z_label = api::value_text(&mut m).map(|s| s.to_vec());
            n_label = api::value_bytes(&mut m) as u32;
        }
        json_append_string(&mut jx, z_label.as_deref(), n_label);
        json_append_char(&mut jx, b':');
        json_append_sql_value(&mut jx, &mut argv[(i + 1) as usize].borrow_mut());
        i += 2;
    }
    json_append_char(&mut jx, b'}');
    json_return_string(&mut jx, None, None);
    api::result_subtype(&mut ctx.borrow_mut(), JSON_SUBTYPE);
}

/// json_remove(JSON, PATH, ...)
///
/// Remove os elementos nomeados de JSON e devolve o resultado. Argumentos
/// JSON ou PATH malformados resultam em erro.
fn json_remove_func(ctx: &Rc<RefCell<sqlite3_context>>, argc: i32, argv: &[MemRef]) {
    let p: JsonParseRef; // O parse
    let mut z_path: Vec<u8> = Vec::new(); // Caminho do elemento a remover
    let mut i: i32; // Contador de laço
    let mut rc: u32; // Código de retorno da sub-rotina
    let mut path_error = false; // vai para json_remove_patherror

    if argc < 1 {
        return;
    }
    p = match json_parse_func_arg(ctx, &argv[0], if argc > 1 { JSON_EDITABLE } else { 0 }) {
        Some(p) => p,
        None => return,
    };
    'json_remove_done: {
        i = 1;
        while i < argc {
            let z_text: Option<Vec<u8>> =
                api::value_text(&mut argv[i as usize].borrow_mut()).map(|s| s.to_vec());
            match z_text {
                None => break 'json_remove_done,
                Some(z) => z_path = z,
            }
            if z_path.first().copied().unwrap_or(0) != b'$' {
                path_error = true; // goto json_remove_patherror
                break 'json_remove_done;
            }
            if z_path.get(1).copied().unwrap_or(0) == 0 {
                // json_remove(j,'$') devolve NULL
                break 'json_remove_done;
            }
            {
                let mut pm = p.borrow_mut();
                pm.e_edit = JEDIT_DEL;
                pm.delta = 0;
                rc = json_lookup_step(&mut pm, 0, &z_path[1..], 0);
            }
            if json_lookup_iserror(rc) {
                if rc == JSON_LOOKUP_NOTFOUND {
                    i += 1;
                    continue; // Sem efeito
                } else if rc == JSON_LOOKUP_PATHERROR {
                    json_bad_path_error(ctx, &z_path);
                } else {
                    api::result_error(&mut ctx.borrow_mut(), b"malformed JSON", -1);
                }
                break 'json_remove_done;
            }
            i += 1;
        }
        json_return_parse(ctx, &p);
        json_parse_free(Some(p));
        return;
    }
    if path_error {
        // json_remove_patherror:
        json_bad_path_error(ctx, &z_path);
    }
    // json_remove_done:
    json_parse_free(Some(p));
}

/// json_replace(JSON, PATH, VALUE, ...)
///
/// Substitui o valor em PATH por VALUE. Se PATH ainda não existe, a rotina
/// não faz nada. Se JSON ou PATH está malformado, gera erro.
fn json_replace_func(ctx: &Rc<RefCell<sqlite3_context>>, argc: i32, argv: &[MemRef]) {
    if argc < 1 {
        return;
    }
    if (argc & 1) == 0 {
        json_wrong_num_args(&mut ctx.borrow_mut(), b"replace");
        return;
    }
    json_insert_into_blob(ctx, argc, argv, JEDIT_REPL);
}

/// json_set(JSON, PATH, VALUE, ...)
///
/// Põe VALUE em PATH. Cria o PATH se ele ainda não existe. Sobrescreve os
/// valores que já existem. Se JSON ou PATH está malformado, gera erro.
///
/// json_insert(JSON, PATH, VALUE, ...)
///
/// Cria PATH e o inicializa com VALUE. Se PATH já existe, a rotina não faz
/// nada. Se JSON ou PATH está malformado, gera erro.
fn json_set_func(ctx: &Rc<RefCell<sqlite3_context>>, argc: i32, argv: &[MemRef]) {
    let flags: i32 = ptr_to_int(api::user_data(&ctx.borrow())) as i32;
    let b_is_set: i32 = ((flags & JSON_ISSET) != 0) as i32;

    if argc < 1 {
        return;
    }
    if (argc & 1) == 0 {
        json_wrong_num_args(
            &mut ctx.borrow_mut(),
            if b_is_set != 0 { &b"set"[..] } else { &b"insert"[..] },
        );
        return;
    }
    json_insert_into_blob(ctx, argc, argv, if b_is_set != 0 { JEDIT_SET } else { JEDIT_INS });
}


// ---- part_011.rs ----

// Convenções assumidas nesta parte (as funções SQL do JSON recebem o contexto como
// `Rc<RefCell<sqlite3_context>>`, porque `JsonString.p_ctx` guarda uma cópia dele, como em
// `json_string_init`, e os argumentos como `&[MemRef]`, como em func.c; o `argc` do C é
// `argv.len()`):
//   - `json_parse_func_arg(&mut sqlite3_context, &mut Mem, u32) -> Option<JsonParseRef>`
//   - `json_bad_path_error(&mut sqlite3_context, &[u8])`
//   - `json_lookup_step(&mut JsonParse, u32, &[u8], u32) -> u32`
//   - `json_lookup_iserror(u32) -> bool` e as constantes `JSON_LOOKUP_*`
//   - `json_convert_text_to_blob(&mut JsonParse, Option<&mut sqlite3_context>) -> i32`
//   - `JsonPretty { p_parse: JsonParseRef, p_out: Rc<RefCell<JsonString>>, z_indent: Vec<u8>,
//      sz_indent: u32, n_indent: u32 }` e `json_translate_blob_to_pretty_text(&mut JsonPretty, u32) -> u32`
//   - `api::aggregate_context::<T>(&mut sqlite3_context, usize) -> Option<Rc<RefCell<T>>>`, com o
//     bloco zerado do C representado por `z_buf` vazio em `JsonString`.

/// json_type(JSON)
/// json_type(JSON, PATH)
///
/// Devolve o "tipo" de nível superior de uma string JSON. json_type() gera um
/// erro se o JSON ou o PATH não estiverem bem formados.
fn json_type_func(ctx: &Rc<RefCell<sqlite3_context>>, argv: &[MemRef]) {
    let argc = argv.len();
    let p = json_parse_func_arg(ctx, &argv[0], 0);
    let p = match p {
        Some(p) => p,
        None => return,
    };
    'json_type_done: {
        let i: u32;
        if argc == 2 {
            let z_path: Vec<u8> = match api::value_text(&mut argv[1].borrow_mut()) {
                Some(z) => z.to_vec(),
                None => break 'json_type_done,
            };
            if z_path.first().copied().unwrap_or(0) != b'$' {
                json_bad_path_error(ctx, &z_path);
                break 'json_type_done;
            }
            i = json_lookup_step(&mut p.borrow_mut(), 0, &z_path[1..], 0);
            if json_lookup_iserror(i) {
                if i == JSON_LOOKUP_NOTFOUND {
                    // nada a fazer
                } else if i == JSON_LOOKUP_PATHERROR {
                    json_bad_path_error(ctx, &z_path);
                } else {
                    api::result_error(&mut ctx.borrow_mut(), b"malformed JSON", -1);
                }
                break 'json_type_done;
            }
        } else {
            i = 0;
        }
        let tipo = JSONB_TYPE[(p.borrow().a_blob[i as usize] & 0x0f) as usize];
        api::result_text(&mut ctx.borrow_mut(), Some(tipo), -1, SQLITE_STATIC);
    }
    json_parse_free(Some(p));
}

/// json_pretty(JSON)
/// json_pretty(JSON, INDENT)
///
/// Devolve o texto do JSON de entrada formatado para leitura. Se o argumento
/// não for JSON válido, devolve NULL.
///
/// O argumento INDENT é o texto usado na indentação. Se omitido, vale quatro
/// espaços (o mesmo do PostgreSQL).
fn json_pretty_func(ctx: &Rc<RefCell<sqlite3_context>>, argv: &[MemRef]) {
    let argc = argv.len();
    let p_parse = json_parse_func_arg(ctx, &argv[0], 0);
    let p_parse = match p_parse {
        Some(p) => p,
        None => return,
    };
    // A string de saída (`s` no C).
    let mut s = json_string_blank();
    json_string_init(&mut s, Some(ctx.clone()));
    let z_indent_arg: Option<Vec<u8>> = if argc == 1 {
        None
    } else {
        api::value_text(&mut argv[1].borrow_mut())
    };
    let (z_indent, sz_indent): (Vec<u8>, u32) = match z_indent_arg {
        None => (b"    ".to_vec(), 4),
        Some(mut z) => {
            // strlen(): o texto termina no primeiro NUL.
            if let Some(n) = z.iter().position(|&b| b == 0) {
                z.truncate(n);
            }
            let n = z.len() as u32;
            (z, n)
        }
    };
    {
        let pp = p_parse.borrow();
        let mut x = JsonPretty {
            p_parse: &pp,
            p_out: &mut s,
            z_indent: &z_indent,
            sz_indent,
            n_indent: 0,
        };
        json_translate_blob_to_pretty_text(&mut x, 0);
    }
    json_return_string(&mut s, None, None);
    json_parse_free(Some(p_parse));
}

/// json_valid(JSON)
/// json_valid(JSON, FLAGS)
///
/// Verifica se o argumento JSON está bem formado. O argumento FLAGS codifica
/// as restrições sobre o que significa "bem formado":
///
///     0x01      Texto JSON canônico da RFC-8259
///     0x02      Texto JSON com extensões opcionais do JSON-5
///     0x04      Superficialmente parece JSONB
///     0x08      JSONB estritamente bem formado
///
/// Se FLAGS for omitido, vale 1. Valores úteis de FLAGS:
///
///    1          JSON canônico estrito
///    2          Texto JSON talvez com extensões do JSON-5
///    4          Superficialmente parece JSONB
///    5          JSON canônico ou JSONB superficial
///    6          JSON-5 ou JSONB superficial
///    8          JSONB estrito
///    9          JSON canônico ou JSONB estrito
///    10         JSON-5 ou JSONB estrito
///
/// Outras combinações são redundantes. Todo texto JSON canônico também é
/// JSON-5 bem formado, então os valores 2 e 3 são iguais. Do mesmo modo, o que
/// passa na validação estrita de JSONB passa na superficial, então 12 a 15 são
/// iguais a 8 a 11.
///
/// A rotina roda em tempo linear para validar texto e na validação estrita de
/// JSONB. A validação superficial de JSONB é de tempo constante, supondo o
/// BLOB já em memória.
///
/// Só os quatro bits baixos de FLAGS são usados hoje. Os bits altos são
/// reservados; a implementação gera erro se algum outro bit estiver ligado.
///
/// Valores devolvidos:
///
///   *   Erro se FLAGS estiver fora do intervalo de 1 a 15.
///   *   NULL se a entrada for NULL.
///   *   1 se a entrada estiver bem formada.
///   *   0 se a entrada não estiver bem formada.
fn json_valid_func(ctx: &Rc<RefCell<sqlite3_context>>, argv: &[MemRef]) {
    let argc = argv.len();
    let mut flags: u8 = 1;
    let mut res: u8 = 0;
    if argc == 2 {
        let f: i64 = api::value_int64(&mut argv[1].borrow_mut());
        if f < 1 || f > 15 {
            api::result_error(
                &mut ctx.borrow_mut(),
                b"FLAGS parameter to json_valid() must be between 1 and 15",
                -1,
            );
            return;
        }
        flags = (f & 0x0f) as u8;
    }
    let tipo = api::value_type(&mut argv[0].borrow_mut());
    if tipo == SQLITE_NULL {
        // SQLITE_LEGACY_JSON_VALID não está definido: nada é devolvido.
        return;
    }
    // O caso SQLITE_BLOB cai no `default` (interpreta como texto) quando o BLOB
    // não parece JSONB.
    let mut como_texto = true;
    if tipo == SQLITE_BLOB && json_func_arg_might_be_binary(&mut argv[0].borrow_mut()) != 0 {
        if flags & 0x04 != 0 {
            // Só verificação superficial, feita pela chamada a
            // json_func_arg_might_be_binary() acima.
            res = 1;
        } else if flags & 0x08 != 0 {
            // Verificação estrita. Traduz BLOB->TEXTO->BLOB; se não houver
            // erros, vale como "verificação estrita".
            let mut px = JsonParse::default();
            px.a_blob = api::value_blob(&mut argv[0].borrow_mut())
                .map(|b| b.to_vec())
                .unwrap_or_default();
            px.n_blob = api::value_bytes(&mut argv[0].borrow_mut()) as u32;
            let i_err = jsonb_validity_check(&px, 0, px.n_blob, 1);
            res = (i_err == 0) as u8;
        }
        como_texto = false;
    }
    if como_texto && (flags & 0x3) != 0 {
        let p = json_parse_func_arg(ctx, &argv[0], JSON_KEEPERROR);
        match p {
            Some(p) => {
                {
                    let pp = p.borrow();
                    if pp.oom != 0 {
                        api::result_error_nomem(&mut ctx.borrow_mut());
                    } else if pp.n_err != 0 {
                        // nada a fazer
                    } else if (flags & 0x02) != 0 || pp.has_nonstd == 0 {
                        res = 1;
                    }
                }
                json_parse_free(Some(p));
            }
            None => {
                api::result_error_nomem(&mut ctx.borrow_mut());
            }
        }
    }
    api::result_int(&mut ctx.borrow_mut(), res as i32);
}

/// json_error_position(JSON)
///
/// Se o argumento for NULL, devolve NULL.
///
/// Se for BLOB, faz uma verificação completa de validade e devolve um valor
/// diferente de zero se ela falhar. O valor devolvido é o deslocamento
/// aproximado, contado a partir de 1, do byte do elemento que contém o
/// primeiro erro.
///
/// Caso contrário interpreta o argumento como TEXTO (mesmo que seja numérico) e
/// devolve a posição do caractere, contada a partir de 1, em que o parser
/// reconheceu que a entrada não é JSON válido, ou 0 se o texto parecer correto.
/// Extensões do JSON-5 são aceitas.
fn json_error_func(ctx: &Rc<RefCell<sqlite3_context>>, argv: &[MemRef]) {
    let mut i_err_pos: i64 = 0; // posição do erro a devolver
    let mut s = JsonParse::default();
    debug_assert!(argv.len() == 1);
    s.db = Some(api::context_db_handle(&ctx.borrow()));
    if json_func_arg_might_be_binary(&mut argv[0].borrow_mut()) != 0 {
        s.a_blob = api::value_blob(&mut argv[0].borrow_mut())
            .map(|b| b.to_vec())
            .unwrap_or_default();
        s.n_blob = api::value_bytes(&mut argv[0].borrow_mut()) as u32;
        i_err_pos = jsonb_validity_check(&s, 0, s.n_blob, 1) as i64;
    } else {
        s.z_json = match api::value_text(&mut argv[0].borrow_mut()) {
            Some(z) => z.to_vec(),
            None => return, // entrada NULL ou OOM
        };
        s.n_json = api::value_bytes(&mut argv[0].borrow_mut());
        if json_convert_text_to_blob(&mut s, None) != 0 {
            if s.oom != 0 {
                i_err_pos = -1;
            } else {
                // Converte o deslocamento em bytes s.i_err em deslocamento em caracteres
                debug_assert!(!s.z_json.is_empty()); // porque s.oom é falso
                let mut k: u32 = 0;
                while k < s.i_err {
                    // z_json[k] além do fim do slice vale 0, como a string do C terminada em NUL
                    let c = s.z_json.get(k as usize).copied().unwrap_or(0);
                    if c == 0 {
                        break;
                    }
                    if (c & 0xc0) != 0x80 {
                        i_err_pos += 1;
                    }
                    k += 1;
                }
                i_err_pos += 1;
            }
        }
    }
    json_parse_reset(&mut s);
    if i_err_pos < 0 {
        api::result_error_nomem(&mut ctx.borrow_mut());
    } else {
        api::result_int64(&mut ctx.borrow_mut(), i_err_pos);
    }
}

// ****************************************************************************
// Implementações das funções SQL de agregação
// ****************************************************************************

/// json_group_array(VALUE)
///
/// Devolve um array JSON composto por todos os valores da agregação.
fn json_array_step(ctx: &Rc<RefCell<sqlite3_context>>, argv: &[MemRef]) {
    let p_str = api::aggregate_context::<JsonString>(
        &mut ctx.borrow_mut(),
        core::mem::size_of::<JsonString>(),
    );
    if let Some(p_str) = p_str {
        let mut s = p_str.borrow_mut();
        if s.z_buf.is_empty() {
            json_string_init(&mut s, Some(ctx.clone()));
            json_append_char(&mut s, b'[');
        } else if s.n_used > 1 {
            json_append_char(&mut s, b',');
        }
        s.p_ctx = Some(ctx.clone());
        json_append_sql_value(&mut s, &mut argv[0].borrow_mut());
    }
}

fn json_array_compute(ctx: &Rc<RefCell<sqlite3_context>>, is_final: bool) {
    let p_str = api::aggregate_context::<JsonString>(&mut ctx.borrow_mut(), 0);
    if let Some(p_str) = p_str {
        let mut s = p_str.borrow_mut();
        s.p_ctx = Some(ctx.clone());
        json_append_char(&mut s, b']');
        let flags = ptr_to_int(api::user_data(&ctx.borrow())) as i32;
        if s.e_err != 0 {
            json_return_string(&mut s, None, None);
            return;
        } else if flags & JSON_BLOB != 0 {
            json_return_string_as_blob(&mut s);
            if is_final {
                if s.b_static == 0 {
                    // sqlite3RCStrUnref(pStr->zBuf): em Rust o buffer é do Vec.
                    s.z_buf = Vec::new();
                }
            } else {
                json_string_trim_one_char(&mut s);
            }
            return;
        } else if is_final {
            // No C o destrutor é sqlite3RCStrUnref (ou SQLITE_TRANSIENT se a
            // string é estática). Aqui o Destructor só tem Static e Transient:
            // o resultado sempre copia o texto e o buffer continua com a JsonString.
            api::result_text(
                &mut ctx.borrow_mut(),
                Some(&s.z_buf[..s.n_used as usize]),
                s.n_used as i32,
                SQLITE_TRANSIENT,
            );
            s.b_static = 1;
        } else {
            api::result_text(
                &mut ctx.borrow_mut(),
                Some(&s.z_buf[..s.n_used as usize]),
                s.n_used as i32,
                SQLITE_TRANSIENT,
            );
            json_string_trim_one_char(&mut s);
        }
    } else {
        api::result_text(&mut ctx.borrow_mut(), Some(b"[]"), 2, SQLITE_STATIC);
    }
    api::result_subtype(&mut ctx.borrow_mut(), JSON_SUBTYPE);
}

fn json_array_value(ctx: &Rc<RefCell<sqlite3_context>>) {
    json_array_compute(ctx, false);
}

fn json_array_final(ctx: &Rc<RefCell<sqlite3_context>>) {
    json_array_compute(ctx, true);
}

/// Este método serve tanto para json_group_array() quanto para
/// json_group_object(). Remove o primeiro elemento do grupo procurando a
/// primeira vírgula (",") que não está dentro de uma string e apagando todo o
/// texto até ela.
fn json_group_inverse(ctx: &Rc<RefCell<sqlite3_context>>, _argv: &[MemRef]) {
    let mut in_str = false;
    let mut n_nest: i32 = 0;
    let p_str = api::aggregate_context::<JsonString>(&mut ctx.borrow_mut(), 0);
    // jsonArrayStep() ou jsonObjectStep() sempre já inicializaram o acumulador.
    let p_str = match p_str {
        Some(p) => p,
        None => return,
    };
    let mut s = p_str.borrow_mut();
    let mut i: u32 = 1;
    while (i as u64) < s.n_used {
        let c = s.z_buf[i as usize];
        if !(c != b',' || in_str || n_nest != 0) {
            break;
        }
        if c == b'"' {
            in_str = !in_str;
        } else if c == b'\\' {
            i += 1;
        } else if !in_str {
            if c == b'{' || c == b'[' {
                n_nest += 1;
            }
            if c == b'}' || c == b']' {
                n_nest -= 1;
            }
        }
        i += 1;
    }
    if (i as u64) < s.n_used {
        s.n_used -= i as u64;
        let n = s.n_used as usize;
        let origem = i as usize + 1;
        s.z_buf.copy_within(origem..origem + n - 1, 1);
        s.z_buf[n] = 0;
    } else {
        s.n_used = 1;
    }
}


// ---- part_012.rs ----

// Notas do modelo adotado nesta parte (ver CONVENTIONS.md):
// - O agregado `json_group_object()` guarda o `JsonString` no contexto de
//   agregação (`api::aggregate_context`), que devolve um `Rc<RefCell<JsonString>>`.
//   O estado recém-zerado do C (`zBuf==0`) corresponde a `z_buf` vazio.
// - O contexto da função é um `Sqlite3ContextRef`; cada chamada a `api::*` pega o
//   empréstimo só pelo tempo da própria chamada.
// - `sqlite3_vtab` e `sqlite3_vtab_cursor` são o "base class" do C: aqui ficam no
//   campo `base`, e os objetos viram `Box` (dono único), entregues e devolvidos
//   pelos métodos do módulo virtual.

/// Passo do agregado `json_group_obj(NAME,VALUE)`.
///
/// Devolve um objeto JSON composto de todos os nomes e valores do agregado.
fn json_object_step(ctx: &Sqlite3ContextRef, _argc: i32, argv: &[MemRef]) {
    let p_str = api::aggregate_context::<JsonString>(
        &mut ctx.borrow_mut(),
        std::mem::size_of::<JsonString>(),
    );
    if let Some(p_str) = p_str {
        let mut s = p_str.borrow_mut();
        if s.z_buf.is_empty() {
            json_string_init(&mut s, Some(ctx.clone()));
            json_append_char(&mut s, b'{');
        } else if s.n_used > 1 {
            json_append_char(&mut s, b',');
        }
        s.p_ctx = Some(ctx.clone());
        let z: Option<Vec<u8>> = api::value_text(&mut argv[0].borrow_mut()).map(|t| t.to_vec());
        let n: u32 = match &z {
            Some(t) => strlen30(t) as u32,
            None => 0,
        };
        json_append_string(&mut s, z.as_deref(), n);
        json_append_char(&mut s, b':');
        json_append_sql_value(&mut s, &mut argv[1].borrow_mut());
    }
}

/// Calcula o resultado de `json_group_obj()`: o do passo final (`is_final`
/// verdadeiro) ou o do valor de janela (`is_final` falso).
fn json_object_compute(ctx: &Sqlite3ContextRef, is_final: i32) {
    let p_str = api::aggregate_context::<JsonString>(&mut ctx.borrow_mut(), 0);
    if let Some(p_str) = p_str {
        let mut s = p_str.borrow_mut();
        json_append_char(&mut s, b'}');
        s.p_ctx = Some(ctx.clone());
        let flags = ptr_to_int(api::user_data(&ctx.borrow())) as i32;
        if s.e_err != 0 {
            json_return_string(&mut s, None, None);
            return;
        } else if flags & JSON_BLOB != 0 {
            json_return_string_as_blob(&mut s);
            if is_final != 0 {
                if s.b_static == 0 {
                    rc_str_unref(&s.z_buf);
                }
            } else {
                json_string_trim_one_char(&mut s);
            }
            return;
        } else if is_final != 0 {
            let n = s.n_used as usize;
            if s.b_static != 0 {
                api::result_text(&mut ctx.borrow_mut(), &s.z_buf[..n], n as i32, SQLITE_TRANSIENT);
            } else {
                api::result_text(&mut ctx.borrow_mut(), &s.z_buf[..n], n as i32, rc_str_unref);
            }
            s.b_static = 1;
        } else {
            let n = s.n_used as usize;
            api::result_text(&mut ctx.borrow_mut(), &s.z_buf[..n], n as i32, SQLITE_TRANSIENT);
            json_string_trim_one_char(&mut s);
        }
    } else {
        api::result_text(&mut ctx.borrow_mut(), b"{}", 2, SQLITE_STATIC);
    }
    api::result_subtype(&mut ctx.borrow_mut(), JSON_SUBTYPE);
}

/// Valor de janela de `json_group_obj()`.
fn json_object_value(ctx: &Sqlite3ContextRef) {
    json_object_compute(ctx, 0);
}

/// Passo final de `json_group_obj()`.
fn json_object_final(ctx: &Sqlite3ContextRef) {
    json_object_compute(ctx, 1);
}

// ****************************************************************************
// A tabela virtual json_each
// ****************************************************************************

/// Elemento pai na pilha de aninhamento do cursor de `json_tree()`.
#[derive(Default, Clone, Copy)]
pub struct JsonParent {
    pub i_head: u32,  // início do objeto ou array
    pub i_value: u32, // início do valor
    pub i_end: u32,   // primeiro byte depois do fim
    pub n_path: u32,  // comprimento do caminho
    pub i_key: i64,   // chave para JSONB_ARRAY
}

/// Cursor de `json_each()` e `json_tree()`.
pub struct JsonEachCursor {
    pub base: sqlite3_vtab_cursor, // classe base
    pub i_rowid: u32,              // o rowid
    pub i: u32,                    // índice em s_parse.a_blob[] da linha atual
    pub i_end: u32,                // fim quando i alcança ou passa deste valor
    pub n_root: u32,               // tamanho do caminho da raiz em bytes
    pub e_type: u8,                // tipo do contêiner do elemento i
    pub b_recursive: u8,           // verdadeiro para json_tree(), falso para json_each()
    pub n_parent: u32,             // profundidade de aninhamento atual
    pub n_parent_alloc: u32,       // espaço alocado em a_parent[]
    pub a_parent: Vec<JsonParent>, // elementos pai de i
    pub db: Option<Sqlite3Ref>,    // conexão com o banco
    pub path: JsonString,          // caminho atual
    pub s_parse: JsonParse,        // análise do JSON de entrada
}

impl Default for JsonEachCursor {
    /// Estado zerado do `sqlite3DbMallocZero`.
    fn default() -> Self {
        JsonEachCursor {
            base: sqlite3_vtab_cursor::default(),
            i_rowid: 0,
            i: 0,
            i_end: 0,
            n_root: 0,
            e_type: 0,
            b_recursive: 0,
            n_parent: 0,
            n_parent_alloc: 0,
            a_parent: Vec::new(),
            db: None,
            path: JsonString {
                p_ctx: None,
                z_buf: Vec::new(),
                n_alloc: 0,
                n_used: 0,
                b_static: 0,
                e_err: 0,
                z_space: [0u8; 100],
            },
            s_parse: JsonParse::default(),
        }
    }
}

/// Conexão da tabela virtual json_each.
#[derive(Default)]
pub struct JsonEachConnection {
    pub base: sqlite3_vtab,     // classe base
    pub db: Option<Sqlite3Ref>, // conexão com o banco
}

// Números das colunas
pub const JEACH_KEY: i32 = 0;
pub const JEACH_VALUE: i32 = 1;
pub const JEACH_TYPE: i32 = 2;
pub const JEACH_ATOM: i32 = 3;
pub const JEACH_ID: i32 = 4;
pub const JEACH_PARENT: i32 = 5;
pub const JEACH_FULLKEY: i32 = 6;
pub const JEACH_PATH: i32 = 7;
// O método x_best_index presume que as colunas JSON e ROOT são as duas últimas
// da tabela. Se isso mudar, atualize x_best_index.
pub const JEACH_JSON: i32 = 8;
pub const JEACH_ROOT: i32 = 9;

/// Construtor da tabela virtual json_each.
fn json_each_connect(
    db: &Sqlite3Ref,
    _p_aux: Option<Rc<dyn std::any::Any>>,
    _argv: &[&[u8]],
    pp_vtab: &mut Option<Box<JsonEachConnection>>,
    _pz_err: &mut Option<Vec<u8>>,
) -> i32 {
    let rc = api::declare_vtab(
        db,
        b"CREATE TABLE x(key,value,type,atom,id,parent,fullkey,path,json HIDDEN,root HIDDEN)",
    );
    if rc == SQLITE_OK {
        // sqlite3DbMallocZero: a alocação em Rust não falha, então o ramo
        // `pNew==0 -> SQLITE_NOMEM` não existe.
        let mut p_new = Box::new(JsonEachConnection::default());
        api::vtab_config(db, SQLITE_VTAB_INNOCUOUS, &[]);
        p_new.db = Some(db.clone());
        *pp_vtab = Some(p_new);
    }
    rc
}

/// Destrutor da tabela virtual json_each.
fn json_each_disconnect(p_vtab: Box<JsonEachConnection>) -> i32 {
    // sqlite3DbFree(p->db, pVtab): o Box é liberado ao sair do escopo.
    drop(p_vtab);
    SQLITE_OK
}

/// Construtor de um objeto JsonEachCursor para json_each().
fn json_each_open_each(p: &JsonEachConnection, pp_cursor: &mut Option<Box<JsonEachCursor>>) -> i32 {
    // sqlite3DbMallocZero: a alocação em Rust não falha, então o ramo
    // `pCur==0 -> SQLITE_NOMEM` não existe.
    let mut p_cur = Box::new(JsonEachCursor::default());
    p_cur.db = p.db.clone();
    json_string_zero(&mut p_cur.path);
    *pp_cursor = Some(p_cur);
    SQLITE_OK
}

/// Construtor de um objeto JsonEachCursor para json_tree().
fn json_each_open_tree(p: &JsonEachConnection, pp_cursor: &mut Option<Box<JsonEachCursor>>) -> i32 {
    let rc = json_each_open_each(p, pp_cursor);
    if rc == SQLITE_OK {
        if let Some(p_cur) = pp_cursor.as_mut() {
            p_cur.b_recursive = 1;
        }
    }
    rc
}

/// Devolve um JsonEachCursor ao estado original. Libera toda a memória retida.
fn json_each_cursor_reset(p: &mut JsonEachCursor) {
    json_parse_reset(&mut p.s_parse);
    json_string_reset(&mut p.path);
    p.a_parent = Vec::new();
    p.i_rowid = 0;
    p.i = 0;
    p.n_parent = 0;
    p.n_parent_alloc = 0;
    p.i_end = 0;
    p.e_type = 0;
}

/// Destrutor de um objeto jsonEachCursor.
fn json_each_close(mut cur: Box<JsonEachCursor>) -> i32 {
    json_each_cursor_reset(&mut cur);
    // sqlite3DbFree(p->db, cur): o Box é liberado ao sair do escopo.
    drop(cur);
    SQLITE_OK
}

/// Devolve verdadeiro se o objeto jsonEachCursor avançou além do fim do objeto
/// JSON.
fn json_each_eof(cur: &mut Sqlite3VtabCursor) -> i32 {
    let p: &mut JsonEachCursor = json_each_cursor_mut(cur);
    (p.i >= p.i_end) as i32
}

/// Se o cursor aponta atualmente para o rótulo de uma entrada de objeto,
/// devolve o índice do valor. Em todos os outros casos devolve a posição
/// atual, que é o valor.
fn json_skip_label(p: &JsonEachCursor) -> i32 {
    if p.e_type == JSONB_OBJECT {
        let mut sz: u32 = 0;
        let n = jsonb_payload_size(&p.s_parse, p.i, &mut sz);
        p.i.wrapping_add(n).wrapping_add(sz) as i32
    } else {
        p.i as i32
    }
}

/// Acrescenta o nome do caminho do elemento atual.
fn json_append_path_name(p: &mut JsonEachCursor) {
    debug_assert!(p.n_parent > 0);
    debug_assert!(p.e_type == JSONB_ARRAY || p.e_type == JSONB_OBJECT);
    if p.e_type == JSONB_ARRAY {
        let key = p.a_parent[(p.n_parent - 1) as usize].i_key;
        json_printf(30, &mut p.path, b"[%lld]", &[Value::Int64(key)]);
    } else {
        let mut sz: u32 = 0;
        let mut need_quote = false;
        let n = jsonb_payload_size(&p.s_parse, p.i, &mut sz);
        let k = p.i.wrapping_add(n) as usize;
        let z: Vec<u8> = p.s_parse.a_blob[k..k + sz as usize].to_vec();
        if sz == 0 || !isalpha(z[0]) {
            need_quote = true;
        } else {
            let mut i: u32 = 0;
            while i < sz {
                if !isalnum(z[i as usize]) {
                    need_quote = true;
                    break;
                }
                i += 1;
            }
        }
        if need_quote {
            json_printf(
                sz.wrapping_add(4) as i32,
                &mut p.path,
                b".\"%.*s\"",
                &[Value::Int(sz as i32), Value::Text(z)],
            );
        } else {
            json_printf(
                sz.wrapping_add(2) as i32,
                &mut p.path,
                b".%.*s",
                &[Value::Int(sz as i32), Value::Text(z)],
            );
        }
    }
}

/// Avança o cursor para o próximo elemento de json_tree().
fn json_each_next(cur: &mut Sqlite3VtabCursor) -> i32 {
    let p: &mut JsonEachCursor = json_each_cursor_mut(cur);
    let mut rc = SQLITE_OK;
    if p.b_recursive != 0 {
        let mut level_change: u8 = 0;
        let mut sz: u32 = 0;
        let i = json_skip_label(p) as u32;
        let x: u8 = p.s_parse.a_blob[i as usize] & 0x0f;
        let n = jsonb_payload_size(&p.s_parse, i, &mut sz);
        if x == JSONB_OBJECT || x == JSONB_ARRAY {
            if p.n_parent >= p.n_parent_alloc {
                let n_new: u64 = p.n_parent_alloc.wrapping_mul(2).wrapping_add(3) as u64;
                let cur = p.a_parent.len();
                if (n_new as usize) > cur && p.a_parent.try_reserve_exact(n_new as usize - cur).is_err() {
                    return SQLITE_NOMEM;
                }
                p.a_parent.resize(n_new as usize, JsonParent::default());
                p.n_parent_alloc = n_new as u32;
            }
            level_change = 1;
            let idx = p.n_parent as usize;
            p.a_parent[idx].i_head = p.i;
            p.a_parent[idx].i_value = i;
            p.a_parent[idx].i_end = i.wrapping_add(n).wrapping_add(sz);
            p.a_parent[idx].i_key = -1;
            p.a_parent[idx].n_path = p.path.n_used as u32;
            if p.e_type != 0 && p.n_parent != 0 {
                json_append_path_name(p);
                if p.path.e_err != 0 {
                    rc = SQLITE_NOMEM;
                }
            }
            p.n_parent += 1;
            p.i = i.wrapping_add(n);
        } else {
            p.i = i.wrapping_add(n).wrapping_add(sz);
        }
        while p.n_parent > 0 && p.i >= p.a_parent[(p.n_parent - 1) as usize].i_end {
            p.n_parent -= 1;
            p.path.n_used = p.a_parent[p.n_parent as usize].n_path as u64;
            level_change = 1;
        }
        if level_change != 0 {
            if p.n_parent > 0 {
                let i_val = p.a_parent[(p.n_parent - 1) as usize].i_value;
                p.e_type = p.s_parse.a_blob[i_val as usize] & 0x0f;
            } else {
                p.e_type = 0;
            }
        }
    } else {
        let mut sz: u32 = 0;
        let i = json_skip_label(p) as u32;
        let n = jsonb_payload_size(&p.s_parse, i, &mut sz);
        p.i = i.wrapping_add(n).wrapping_add(sz);
    }
    if p.e_type == JSONB_ARRAY && p.n_parent != 0 {
        let idx = (p.n_parent - 1) as usize;
        p.a_parent[idx].i_key += 1;
    }
    p.i_rowid += 1;
    rc
}

/// Comprimento do caminho para rowid==0 no modo b_recursive.
fn json_each_path_length(p: &mut JsonEachCursor) -> i32 {
    let mut n: u32 = p.path.n_used as u32;
    if p.i_rowid == 0 && p.b_recursive != 0 && n >= 2 {
        while n > 1 {
            n -= 1;
            let c = p.path.z_buf[n as usize];
            if c == b'[' || c == b'.' {
                let c_saved = c;
                p.path.z_buf[n as usize] = 0;
                debug_assert!(p.s_parse.e_edit == 0);
                // z+1 termina em NUL na posição n (o NUL entra no recorte).
                let x = json_lookup_step(
                    &mut p.s_parse,
                    0,
                    &p.path.z_buf[1..(n as usize + 1)],
                    0,
                );
                p.path.z_buf[n as usize] = c_saved;
                if json_lookup_is_error(x) {
                    continue;
                }
                let mut sz: u32 = 0;
                if x.wrapping_add(jsonb_payload_size(&p.s_parse, x, &mut sz)) == p.i {
                    break;
                }
            }
        }
    }
    n as i32
}


// ---- part_013.rs ----

// Nota de integração: os métodos do módulo recebem `&mut Sqlite3VtabCursor` (assinatura de
// `Sqlite3Module`). O `(JsonEachCursor*)cur` do C é feito por `json_each_cursor_mut()`, que devolve
// o `JsonEachCursor` que contém o `base` recebido (a ponte fica com o tech lead, no mod.rs).
//
// Em `JsonParse` o `z_json` é um `Vec<u8>`: o `zJson==0` do C (entrada em JSONB) corresponde a
// `z_json` vazio. Texto vazio nunca chega a esse ponto, porque falha na conversão para JSONB
// ("malformed JSON"), então a equivalência é exata.

/// Devolve o valor de uma coluna.
fn json_each_column(
    cur: &mut Sqlite3VtabCursor, // O cursor
    ctx: &mut Sqlite3Context,    // Primeiro argumento de result_...()
    i_column: i32,               // Qual coluna devolver
) -> i32 {
    let p: &mut JsonEachCursor = json_each_cursor_mut(cur);
    match i_column {
        JEACH_KEY => {
            'key: {
                if p.n_parent == 0 {
                    let n: u32;
                    let j: u32;
                    if p.n_root == 1 {
                        break 'key;
                    }
                    j = json_each_path_length(p);
                    n = p.n_root - j;
                    if n == 0 {
                        break 'key;
                    } else if p.path.z_buf[j as usize] == b'[' {
                        let mut x: i64 = 0;
                        atoi64(
                            &p.path.z_buf[(j + 1) as usize..],
                            &mut x,
                            (n - 1) as usize,
                            SQLITE_UTF8 as i32,
                        );
                        api::result_int64(ctx, x);
                    } else if p.path.z_buf[(j + 1) as usize] == b'"' {
                        let a = (j + 2) as usize;
                        let len = n.wrapping_sub(3);
                        api::result_text(
                            ctx,
                            Some(&p.path.z_buf[a..a + len as usize]),
                            len as i32,
                            SQLITE_TRANSIENT,
                        );
                    } else {
                        let a = (j + 1) as usize;
                        let len = n - 1;
                        api::result_text(
                            ctx,
                            Some(&p.path.z_buf[a..a + len as usize]),
                            len as i32,
                            SQLITE_TRANSIENT,
                        );
                    }
                    break 'key;
                }
                if p.e_type == JSONB_OBJECT {
                    json_return_from_blob(&p.s_parse, p.i, ctx, 1);
                } else {
                    debug_assert!(p.e_type == JSONB_ARRAY);
                    api::result_int64(ctx, p.a_parent[(p.n_parent - 1) as usize].i_key);
                }
            }
        }
        JEACH_VALUE => {
            let i = json_skip_label(p);
            json_return_from_blob(&p.s_parse, i, ctx, 1);
            if (p.s_parse.a_blob[i as usize] & 0x0f) >= JSONB_ARRAY {
                api::result_subtype(ctx, JSON_SUBTYPE);
            }
        }
        JEACH_TYPE => {
            let i = json_skip_label(p);
            let e_type: u8 = p.s_parse.a_blob[i as usize] & 0x0f;
            api::result_text(ctx, Some(JSONB_TYPE[e_type as usize]), -1, SQLITE_STATIC);
        }
        JEACH_ATOM => {
            let i = json_skip_label(p);
            if (p.s_parse.a_blob[i as usize] & 0x0f) < JSONB_ARRAY {
                json_return_from_blob(&p.s_parse, i, ctx, 1);
            }
        }
        JEACH_ID => {
            api::result_int64(ctx, p.i as i64);
        }
        JEACH_PARENT => {
            if p.n_parent > 0 && p.b_recursive != 0 {
                api::result_int64(ctx, p.a_parent[(p.n_parent - 1) as usize].i_head as i64);
            }
        }
        JEACH_FULLKEY => {
            let n_base: u64 = p.path.n_used;
            if p.n_parent != 0 {
                json_append_path_name(p);
            }
            api::result_text64(
                ctx,
                Some(&p.path.z_buf[..p.path.n_used as usize]),
                p.path.n_used,
                SQLITE_TRANSIENT,
                SQLITE_UTF8 as u8,
            );
            p.path.n_used = n_base;
        }
        JEACH_PATH => {
            let n = json_each_path_length(p);
            api::result_text64(
                ctx,
                Some(&p.path.z_buf[..n as usize]),
                n as u64,
                SQLITE_TRANSIENT,
                SQLITE_UTF8 as u8,
            );
        }
        JEACH_JSON => {
            if p.s_parse.z_json.is_empty() {
                api::result_blob(
                    ctx,
                    Some(&p.s_parse.a_blob[..p.s_parse.n_blob as usize]),
                    p.s_parse.n_blob as i32,
                    SQLITE_TRANSIENT,
                );
            } else {
                api::result_text(ctx, Some(&p.s_parse.z_json[..]), -1, SQLITE_TRANSIENT);
            }
        }
        _ => {
            api::result_text(
                ctx,
                Some(&p.path.z_buf[..p.n_root as usize]),
                p.n_root as i32,
                SQLITE_STATIC,
            );
        }
    }
    SQLITE_OK
}

/// Devolve o valor atual do rowid.
fn json_each_rowid(cur: &mut Sqlite3VtabCursor, p_rowid: &mut i64) -> i32 {
    let p: &mut JsonEachCursor = json_each_cursor_mut(cur);
    *p_rowid = p.i_rowid as i64;
    SQLITE_OK
}

/// A estratégia de consulta é procurar uma restrição de igualdade na coluna
/// json. Sem essa restrição a tabela não funciona. `idx_num` vale 1 se a
/// restrição é achada, 3 se a restrição e `z_root` são achados, e 0 nos demais
/// casos.
fn json_each_best_index(_tab: &mut Sqlite3Vtab, p_idx_info: &mut Sqlite3IndexInfo) -> i32 {
    let mut i: i32; // Contador de laço ou índice calculado
    let mut a_idx: [i32; 2]; // Índice das restrições de JSON e ROOT
    let mut unusable_mask: i32 = 0; // Máscara das restrições inutilizáveis de JSON e ROOT
    let mut idx_mask: i32 = 0; // Máscara das restrições == utilizáveis de JSON e ROOT

    // Esta implementação supõe que JSON e ROOT são as duas últimas colunas da tabela.
    debug_assert!(JEACH_ROOT == JEACH_JSON + 1);
    a_idx = [-1, -1];
    if let Some(ref constraints) = p_idx_info.a_constraint {
        i = 0;
        while i < p_idx_info.n_constraint {
            let p_constraint = &constraints[i as usize];
            let i_col: i32;
            let i_mask: i32;
            if p_constraint.i_column < JEACH_JSON {
                i += 1;
                continue;
            }
            i_col = p_constraint.i_column - JEACH_JSON;
            debug_assert!(i_col == 0 || i_col == 1);
            i_mask = 1 << i_col;
            if p_constraint.usable == 0 {
                unusable_mask |= i_mask;
            } else if p_constraint.op == SQLITE_INDEX_CONSTRAINT_EQ as u8 {
                a_idx[i_col as usize] = i;
                idx_mask |= i_mask;
            }
            i += 1;
        }
    }
    if p_idx_info.n_order_by > 0 {
        if let Some(ref order_by) = p_idx_info.a_order_by {
            if order_by[0].i_column < 0 && order_by[0].desc == 0 {
                p_idx_info.order_by_consumed = 1;
            }
        }
    }

    if (unusable_mask & !idx_mask) != 0 {
        // Se há restrições inutilizáveis em JSON ou ROOT, rejeita o plano inteiro.
        return SQLITE_CONSTRAINT;
    }
    if a_idx[0] < 0 {
        // Sem entrada JSON. Deixa estimated_cost no valor enorme com que foi
        // inicializado, para desencorajar o planejador de escolher este plano.
        p_idx_info.idx_num = 0;
    } else {
        p_idx_info.estimated_cost = 1.0;
        i = a_idx[0];
        if let Some(ref mut usage) = p_idx_info.a_constraint_usage {
            usage[i as usize].argv_index = 1;
            usage[i as usize].omit = 1;
        }
        if a_idx[1] < 0 {
            p_idx_info.idx_num = 1; // Só JSON informado. Plano 1
        } else {
            i = a_idx[1];
            if let Some(ref mut usage) = p_idx_info.a_constraint_usage {
                usage[i as usize].argv_index = 2;
                usage[i as usize].omit = 1;
            }
            p_idx_info.idx_num = 3; // JSON e ROOT informados. Plano 3
        }
    }
    SQLITE_OK
}

/// Inicia uma busca numa nova string JSON.
fn json_each_filter(
    cur: &mut Sqlite3VtabCursor,
    idx_num: i32,
    _idx_str: Option<&[u8]>,
    argv: &mut [Sqlite3Value],
) -> i32 {
    let p: &mut JsonEachCursor = json_each_cursor_mut(cur);
    let mut z_root: Vec<u8>;
    let mut i: u32;
    let n: u32;
    let mut sz: u32;

    json_each_cursor_reset(p);
    if idx_num == 0 {
        return SQLITE_OK;
    }
    p.s_parse = JsonParse::default();
    p.s_parse.n_jp_ref = 1;
    p.s_parse.db = p.db.clone();
    'json_each_malformed_input: {
        if json_func_arg_might_be_binary(&mut argv[0]) {
            p.s_parse.n_blob = api::value_bytes(&mut argv[0]) as u32;
            p.s_parse.a_blob = api::value_blob(&mut argv[0]).map(|s| s.to_vec()).unwrap_or_default();
        } else {
            // `zJson==0` só vale para NULL: texto vazio segue adiante e falha na
            // conversão para JSONB ("malformed JSON"), como no C.
            match api::value_text(&mut argv[0]) {
                Some(s) => p.s_parse.z_json = s.to_vec(),
                None => {
                    p.i = 0;
                    p.i_end = 0;
                    return SQLITE_OK;
                }
            }
            p.s_parse.n_json = api::value_bytes(&mut argv[0]);
            if json_convert_text_to_blob(&mut p.s_parse, None) != 0 {
                if p.s_parse.oom != 0 {
                    return SQLITE_NOMEM;
                }
                break 'json_each_malformed_input;
            }
        }
        if idx_num == 3 {
            z_root = match api::value_text(&mut argv[1]) {
                Some(s) => s.to_vec(),
                None => return SQLITE_OK,
            };
            if z_root.first().copied().unwrap_or(0) != b'$' {
                if let Some(vt) = p.base.p_vtab.as_ref() {
                    vt.borrow_mut().z_err_msg = json_bad_path_error(None, &z_root);
                }
                json_each_cursor_reset(p);
                let has_err = p
                    .base
                    .p_vtab
                    .as_ref()
                    .map(|vt| vt.borrow().z_err_msg.is_some())
                    .unwrap_or(false);
                return if has_err { SQLITE_ERROR } else { SQLITE_NOMEM };
            }
            p.n_root = strlen30_nn(&z_root) as u32;
            if z_root.get(1).copied().unwrap_or(0) == 0 {
                p.i = 0;
                i = 0;
                p.e_type = 0;
            } else {
                // O C passa `zRoot+1` terminado em NUL: o recorte leva o NUL no fim.
                let mut z_path: Vec<u8> = z_root[1..].to_vec();
                z_path.push(0);
                i = json_lookup_step(&mut p.s_parse, 0, &z_path, 0);
                if json_lookup_iserror(i) {
                    if i == JSON_LOOKUP_NOTFOUND {
                        p.i = 0;
                        p.e_type = 0;
                        p.i_end = 0;
                        return SQLITE_OK;
                    }
                    if let Some(vt) = p.base.p_vtab.as_ref() {
                        vt.borrow_mut().z_err_msg = json_bad_path_error(None, &z_root);
                    }
                    json_each_cursor_reset(p);
                    let has_err = p
                        .base
                        .p_vtab
                        .as_ref()
                        .map(|vt| vt.borrow().z_err_msg.is_some())
                        .unwrap_or(false);
                    return if has_err { SQLITE_ERROR } else { SQLITE_NOMEM };
                }
                if p.s_parse.i_label != 0 {
                    p.i = p.s_parse.i_label;
                    p.e_type = JSONB_OBJECT;
                } else {
                    p.i = i;
                    p.e_type = JSONB_ARRAY;
                }
            }
            let n_root = p.n_root;
            json_append_raw(&mut p.path, &z_root, n_root);
        } else {
            p.i = 0;
            i = 0;
            p.e_type = 0;
            p.n_root = 1;
            json_append_raw(&mut p.path, b"$", 1);
        }
        p.n_parent = 0;
        sz = 0;
        n = jsonb_payload_size(&p.s_parse, i, &mut sz);
        p.i_end = i.wrapping_add(n).wrapping_add(sz);
        if (p.s_parse.a_blob[i as usize] & 0x0f) >= JSONB_ARRAY && p.b_recursive == 0 {
            p.i = i.wrapping_add(n);
            p.e_type = p.s_parse.a_blob[i as usize] & 0x0f;
            p.a_parent = vec![JsonParent::default()];
            p.n_parent = 1;
            p.n_parent_alloc = 1;
            p.a_parent[0].i_key = 0;
            p.a_parent[0].i_end = p.i_end;
            p.a_parent[0].i_head = p.i;
            p.a_parent[0].i_value = i;
        }
        return SQLITE_OK;
    }

    // json_each_malformed_input:
    if let Some(vt) = p.base.p_vtab.as_ref() {
        vt.borrow_mut().z_err_msg = Some(b"malformed JSON".to_vec());
    }
    json_each_cursor_reset(p);
    let has_err = p
        .base
        .p_vtab
        .as_ref()
        .map(|vt| vt.borrow().z_err_msg.is_some())
        .unwrap_or(false);
    if has_err {
        SQLITE_ERROR
    } else {
        SQLITE_NOMEM
    }
}

/// Os métodos da tabela virtual json_each.
pub fn json_each_module() -> Rc<Sqlite3Module> {
    Rc::new(Sqlite3Module {
        i_version: 0,
        x_create: None,
        x_connect: Some(json_each_connect),    // xConnect
        x_best_index: Some(json_each_best_index), // xBestIndex
        x_disconnect: Some(json_each_disconnect), // xDisconnect
        x_destroy: None,
        x_open: Some(json_each_open_each),     // xOpen: abre um cursor
        x_close: Some(json_each_close),        // xClose
        x_filter: Some(json_each_filter),      // xFilter: configura as restrições da varredura
        x_next: Some(json_each_next),          // xNext: avança um cursor
        x_eof: Some(json_each_eof),            // xEof: verifica o fim da varredura
        x_column: Some(json_each_column),      // xColumn: lê dados
        x_rowid: Some(json_each_rowid),        // xRowid: lê dados
        x_update: None,
        x_begin: None,
        x_sync: None,
        x_commit: None,
        x_rollback: None,
        x_find_function: None,
        x_rename: None,
        x_savepoint: None,
        x_release: None,
        x_rollback_to: None,
        x_shadow_name: None,
        x_integrity: None,
    })
}

/// Os métodos da tabela virtual json_tree.
pub fn json_tree_module() -> Rc<Sqlite3Module> {
    Rc::new(Sqlite3Module {
        i_version: 0,
        x_create: None,
        x_connect: Some(json_each_connect),    // xConnect
        x_best_index: Some(json_each_best_index), // xBestIndex
        x_disconnect: Some(json_each_disconnect), // xDisconnect
        x_destroy: None,
        x_open: Some(json_each_open_tree),     // xOpen: abre um cursor
        x_close: Some(json_each_close),        // xClose
        x_filter: Some(json_each_filter),      // xFilter: configura as restrições da varredura
        x_next: Some(json_each_next),          // xNext: avança um cursor
        x_eof: Some(json_each_eof),            // xEof: verifica o fim da varredura
        x_column: Some(json_each_column),      // xColumn: lê dados
        x_rowid: Some(json_each_rowid),        // xRowid: lê dados
        x_update: None,
        x_begin: None,
        x_sync: None,
        x_commit: None,
        x_rollback: None,
        x_find_function: None,
        x_rename: None,
        x_savepoint: None,
        x_release: None,
        x_rollback_to: None,
        x_shadow_name: None,
        x_integrity: None,
    })
}

/// Registra as funções JSON.
pub fn register_json_functions() {
    //  result_subtype() ----,  ,--- value_subtype()
    //                       |  |
    //     Usa o cache ----, |  | ,---- Devolve JSONB
    //                     | |  | |
    //   Número de args -, | |  | | ,--- Flags
    let a_json_func: Vec<FuncDef> = vec![
        jfunction("json", 1, 1, 1, 0, 0, 0, Rc::new(json_remove_func)),
        jfunction("jsonb", 1, 1, 0, 0, 1, 0, Rc::new(json_remove_func)),
        jfunction("json_array", -1, 0, 1, 1, 0, 0, Rc::new(json_array_func)),
        jfunction("jsonb_array", -1, 0, 1, 1, 1, 0, Rc::new(json_array_func)),
        jfunction("json_array_length", 1, 1, 0, 0, 0, 0, Rc::new(json_array_length_func)),
        jfunction("json_array_length", 2, 1, 0, 0, 0, 0, Rc::new(json_array_length_func)),
        jfunction("json_error_position", 1, 1, 0, 0, 0, 0, Rc::new(json_error_func)),
        jfunction("json_extract", -1, 1, 1, 0, 0, 0, Rc::new(json_extract_func)),
        jfunction("jsonb_extract", -1, 1, 0, 0, 1, 0, Rc::new(json_extract_func)),
        jfunction("->", 2, 1, 1, 0, 0, JSON_JSON as isize, Rc::new(json_extract_func)),
        jfunction("->>", 2, 1, 0, 0, 0, JSON_SQL as isize, Rc::new(json_extract_func)),
        jfunction("json_insert", -1, 1, 1, 1, 0, 0, Rc::new(json_set_func)),
        jfunction("jsonb_insert", -1, 1, 0, 1, 1, 0, Rc::new(json_set_func)),
        jfunction("json_object", -1, 0, 1, 1, 0, 0, Rc::new(json_object_func)),
        jfunction("jsonb_object", -1, 0, 1, 1, 1, 0, Rc::new(json_object_func)),
        jfunction("json_patch", 2, 1, 1, 0, 0, 0, Rc::new(json_patch_func)),
        jfunction("jsonb_patch", 2, 1, 0, 0, 1, 0, Rc::new(json_patch_func)),
        jfunction("json_pretty", 1, 1, 0, 0, 0, 0, Rc::new(json_pretty_func)),
        jfunction("json_pretty", 2, 1, 0, 0, 0, 0, Rc::new(json_pretty_func)),
        jfunction("json_quote", 1, 0, 1, 1, 0, 0, Rc::new(json_quote_func)),
        jfunction("json_remove", -1, 1, 1, 0, 0, 0, Rc::new(json_remove_func)),
        jfunction("jsonb_remove", -1, 1, 0, 0, 1, 0, Rc::new(json_remove_func)),
        jfunction("json_replace", -1, 1, 1, 1, 0, 0, Rc::new(json_replace_func)),
        jfunction("jsonb_replace", -1, 1, 0, 1, 1, 0, Rc::new(json_replace_func)),
        jfunction("json_set", -1, 1, 1, 1, 0, JSON_ISSET as isize, Rc::new(json_set_func)),
        jfunction("jsonb_set", -1, 1, 0, 1, 1, JSON_ISSET as isize, Rc::new(json_set_func)),
        jfunction("json_type", 1, 1, 0, 0, 0, 0, Rc::new(json_type_func)),
        jfunction("json_type", 2, 1, 0, 0, 0, 0, Rc::new(json_type_func)),
        jfunction("json_valid", 1, 1, 0, 0, 0, 0, Rc::new(json_valid_func)),
        jfunction("json_valid", 2, 1, 0, 0, 0, 0, Rc::new(json_valid_func)),
        waggregate(
            "json_group_array",
            1,
            0,
            0,
            Rc::new(json_array_step),
            Rc::new(json_array_final),
            Some(Rc::new(json_array_value)),
            Some(Rc::new(json_group_inverse)),
            SQLITE_SUBTYPE | SQLITE_RESULT_SUBTYPE | SQLITE_UTF8 | SQLITE_DETERMINISTIC,
        ),
        waggregate(
            "jsonb_group_array",
            1,
            JSON_BLOB as isize,
            0,
            Rc::new(json_array_step),
            Rc::new(json_array_final),
            Some(Rc::new(json_array_value)),
            Some(Rc::new(json_group_inverse)),
            SQLITE_SUBTYPE | SQLITE_RESULT_SUBTYPE | SQLITE_UTF8 | SQLITE_DETERMINISTIC,
        ),
        waggregate(
            "json_group_object",
            2,
            0,
            0,
            Rc::new(json_object_step),
            Rc::new(json_object_final),
            Some(Rc::new(json_object_value)),
            Some(Rc::new(json_group_inverse)),
            SQLITE_SUBTYPE | SQLITE_RESULT_SUBTYPE | SQLITE_UTF8 | SQLITE_DETERMINISTIC,
        ),
        waggregate(
            "jsonb_group_object",
            2,
            JSON_BLOB as isize,
            0,
            Rc::new(json_object_step),
            Rc::new(json_object_final),
            Some(Rc::new(json_object_value)),
            Some(Rc::new(json_group_inverse)),
            SQLITE_SUBTYPE | SQLITE_RESULT_SUBTYPE | SQLITE_UTF8 | SQLITE_DETERMINISTIC,
        ),
    ];
    insert_builtin_funcs(&a_json_func);
}


// ---- part_014.rs ----

/// Registra as funções de tabela JSON (`json_each` e `json_tree`).
pub fn json_table_functions(db: &Sqlite3Ref) -> i32 {
    let mut rc = SQLITE_OK;
    // Tabela estática `aMod[]` do C: nome da função e o módulo virtual correspondente.
    let a_mod: [(&[u8], Rc<Sqlite3Module>); 2] = [
        (b"json_each", json_each_module()),
        (b"json_tree", json_tree_module()),
    ];
    let mut i: usize = 0;
    while i < a_mod.len() && rc == SQLITE_OK {
        rc = api::create_module(db, a_mod[i].0, a_mod[i].1.clone(), None);
        i += 1;
    }
    rc
}

