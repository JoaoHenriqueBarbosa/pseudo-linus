//! `fts3_tokenizer.c`: o registro de tokenizadores do FTS3 (a tabela hash `Fts3Hash` de nomes), a
//! função SQL `fts3_tokenizer()` que o lê e escreve, a análise dos argumentos da cláusula
//! `tokenize=` (`sqlite3Fts3NextToken`, `sqlite3Fts3InitTokenizer`) e `sqlite3Fts3IsIdChar`.
//!
//! # O registro (`Fts3HashWrapper`) e a função `fts3_tokenizer()`
//!
//! O `pHash` do C (um `Fts3HashWrapper` com a hash e uma contagem de referências) é o `aux` dos
//! módulos `fts3`, `fts4` e `fts3tokenize` e o dado de usuário das duas funções `fts3_tokenizer`.
//! Aqui é um `Rc<Fts3HashWrapper>` (a contagem de referências é a do `Rc`, o `hashDestroy` é o
//! `Drop`). Ele é alterado em tempo de execução pela forma de dois argumentos da função SQL, então
//! é o único ponto do FTS3 com `RefCell`: o compartilhamento é o do C (o mesmo `pHash` visto de
//! quatro lugares) e nenhuma referência atravessa uma chamada que reentre no registro.
//!
//! No C o valor guardado na hash é o ponteiro para o `sqlite3_tokenizer_module`, e a função SQL
//! troca esse ponteiro (8 bytes) como BLOB: `fts3_tokenizer('simple')` devolve o ponteiro e
//! `fts3_tokenizer('meu', fts3_tokenizer('simple'))` registra o mesmo módulo com outro nome. Sem
//! ponteiros, o "ponteiro" do blob é um handle de 8 bytes (a posição do módulo no arena de módulos
//! do registro, mais 1; 0 é o ponteiro nulo): `Fts3HashWrapper::modules` guarda todo módulo já
//! registrado, mesmo depois de removido da hash (os módulos do C são estáticos e o ponteiro
//! continua válido). Um blob de 8 bytes que não é handle de módulo nenhum é rejeitado com
//! `argument type mismatch` (o C o aceitaria e travaria no primeiro uso). O valor do handle de
//! `simple` não coincide com o endereço do C, que também varia de uma execução para outra.

use std::cell::RefCell;
use std::rc::Rc;

use crate::build::text_arg;
use crate::connection::{Connection, Context, UserData};
use crate::consts::{SQLITE_DBCONFIG_ENABLE_FTS3_TOKENIZER, SQLITE_DIRECTONLY, SQLITE_ERROR, SQLITE_UTF8};
use crate::main::{create_function_api, db_config, DbConfigArg};
use crate::mem::{Mem, StrDtor};
use crate::printf::{mprintf, PrintfArg};
use crate::util::at;
use crate::vdbeapi::{context_db_handle, result_blob, result_error, text_of, user_data, value_blob, value_bytes, value_frombind};

use super::hash::{Fts3Hash, FTS3_HASH_STRING};
use super::int::{fts3_dequote, Fts3Tokenizer, Fts3TokenizerModule};

/// O estado do registro: a hash nome para handle e o arena dos módulos que os handles nomeiam.
struct Fts3HashWrapperInner {
    /// `hash`: a chave é o nome seguido de um NUL (o `nKey` do C inclui o terminador) e o dado é
    /// o handle do módulo.
    hash: Fts3Hash<u64>,
    /// Os módulos; o handle `h` nomeia `modules[h-1]`.
    modules: Vec<Rc<dyn Fts3TokenizerModule>>,
}

/// `Fts3HashWrapper`: o registro de tokenizadores de uma conexão (ver o cabeçalho do módulo).
pub struct Fts3HashWrapper {
    inner: RefCell<Fts3HashWrapperInner>,
}

impl Default for Fts3HashWrapper {
    fn default() -> Self {
        Self::new()
    }
}

/// A chave da hash para o nome `name`: o nome seguido de NUL (`nName = strlen(name)+1`).
fn hash_key(name: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(name.len() + 1);
    key.extend_from_slice(name);
    key.push(0);
    key
}

impl Fts3HashWrapper {
    /// O registro vazio (`sqlite3Fts3HashInit(&pHash->hash, FTS3_HASH_STRING, 1)`). O
    /// `sqlite3Fts3Init` registra nele `simple`, `porter` e `unicode61` com
    /// [`Fts3HashWrapper::insert_module`].
    pub fn new() -> Self {
        Fts3HashWrapper {
            inner: RefCell::new(Fts3HashWrapperInner {
                hash: Fts3Hash::new(FTS3_HASH_STRING),
                modules: Vec::new(),
            }),
        }
    }

    /// `sqlite3Fts3HashInsert(&pHash->hash, name, nName+1, module)`: registra `module` com o
    /// nome `name` (sem o NUL), substituindo o que houver.
    pub fn insert_module(&self, name: &[u8], module: Rc<dyn Fts3TokenizerModule>) {
        let mut inner = self.inner.borrow_mut();
        inner.modules.push(module);
        let handle = inner.modules.len() as u64;
        inner.hash.insert(&hash_key(name), Some(handle));
    }

    /// `sqlite3Fts3HashFind(&pHash->hash, name, nName+1)` seguido da leitura do módulo: o módulo
    /// registrado com o nome `name` (sem o NUL).
    pub fn find_module(&self, name: &[u8]) -> Option<Rc<dyn Fts3TokenizerModule>> {
        let inner = self.inner.borrow();
        let handle = *inner.hash.find(&hash_key(name))?;
        inner.modules.get((handle - 1) as usize).cloned()
    }

    /// O dado guardado para a chave `key` (nome com NUL): o handle, ou 0 se não há.
    fn find_handle(&self, key: &[u8]) -> u64 {
        self.inner.borrow().hash.find(key).copied().unwrap_or(0)
    }

    /// `sqlite3Fts3HashInsert(pHash, zName, nName, pPtr)` da forma de dois argumentos de
    /// `fts3_tokenizer()`: guarda `handle` na chave `key` (handle 0 apaga a entrada) e devolve o
    /// que havia (0 se nada). `None` se `handle` não nomeia módulo algum.
    fn insert_handle(&self, key: &[u8], handle: u64) -> Option<u64> {
        let mut inner = self.inner.borrow_mut();
        if handle > inner.modules.len() as u64 {
            return None;
        }
        let data = if handle == 0 { None } else { Some(handle) };
        Some(inner.hash.insert(key, data).unwrap_or(0))
    }
}

/// `fts3TokenizerEnabled`: a forma de dois argumentos de `fts3_tokenizer()` foi ativada por
/// `sqlite3_db_config(db, SQLITE_DBCONFIG_ENABLE_FTS3_TOKENIZER, 1, 0)`? (Com a opção
/// `ENABLE_FTS3_TOKENIZER` do Debian, vem ligada por padrão.)
fn fts3_tokenizer_enabled(ctx: &mut Context<'_>) -> bool {
    let mut is_enabled = 0;
    db_config(
        context_db_handle(ctx),
        SQLITE_DBCONFIG_ENABLE_FTS3_TOKENIZER,
        DbConfigArg::Flag(-1, Some(&mut is_enabled)),
    );
    is_enabled != 0
}

/// `fts3TokenizerFunc`: a função SQL de acesso à hash:
///
/// ```text
///   SELECT fts3_tokenizer(<nome>);
///   SELECT fts3_tokenizer(<nome>, <ponteiro>);
/// ```
///
/// Com `<ponteiro>` (um blob com o ponteiro), ele é guardado como o dado de `<nome>`; sem ele,
/// `<nome>` já precisa existir, ou é erro. Nos dois casos o valor devolvido é o blob com o
/// ponteiro guardado em `<nome>` (depois da atualização, se houver).
fn fts3_tokenizer_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let UserData::Ptr(p) = user_data(ctx) else {
        return;
    };
    let Ok(p_hash) = p.downcast::<Fts3HashWrapper>() else {
        return;
    };
    debug_assert!(argv.len() == 1 || argv.len() == 2);

    let z_name: Option<Vec<u8>> = text_of(&argv[0]).map(|z| z.into_owned());
    let mut p_ptr: u64 = 0;

    if argv.len() == 2 {
        if fts3_tokenizer_enabled(ctx) || value_frombind(&argv[1]) {
            let mut arg = argv[1].clone();
            let n = value_bytes(&mut arg);
            let blob: Option<[u8; 8]> = match value_blob(&mut arg) {
                Some(b) if n == 8 && b.len() == 8 => {
                    let mut a = [0u8; 8];
                    a.copy_from_slice(b);
                    Some(a)
                }
                _ => None,
            };
            let (Some(z_name), Some(blob)) = (z_name.as_ref(), blob) else {
                result_error(ctx, b"argument type mismatch", -1);
                return;
            };
            p_ptr = u64::from_ne_bytes(blob);
            match p_hash.insert_handle(&hash_key(z_name), p_ptr) {
                None => {
                    result_error(ctx, b"argument type mismatch", -1);
                    return;
                }
                /* O C compara o ponteiro antigo com o novo para detectar falta de memória (a
                ** inserção devolve o dado novo se falha), e o teste também vale quando o ponteiro
                ** novo é igual ao que já estava lá, ou quando é nulo e não havia nada. */
                Some(p_old) if p_old == p_ptr => {
                    result_error(ctx, b"out of memory", -1);
                }
                Some(_) => {}
            }
        } else {
            result_error(ctx, b"fts3tokenize disabled", -1);
            return;
        }
    } else {
        if let Some(z_name) = z_name.as_ref() {
            p_ptr = p_hash.find_handle(&hash_key(z_name));
        }
        if p_ptr == 0 {
            let z_err = mprintf(b"unknown tokenizer: %s", &[PrintfArg::Text(z_name)]).unwrap_or_default();
            result_error(ctx, &z_err, -1);
            return;
        }
    }
    if fts3_tokenizer_enabled(ctx) || value_frombind(&argv[0]) {
        result_blob(ctx, Some(&p_ptr.to_ne_bytes()), 8, StrDtor::Transient);
    }
}

/// `isFtsIdChar`: os caracteres ASCII que fazem parte de um identificador (letras, dígitos, `$` e
/// `_`).
static IS_FTS_ID_CHAR: [u8; 128] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, /* 0x */
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, /* 1x */
    0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, /* 2x */
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, /* 3x */
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, /* 4x */
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, /* 5x */
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, /* 6x */
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, /* 7x */
];

/// `sqlite3Fts3IsIdChar`: verdadeiro para um byte não ASCII ou um caractere de identificador.
pub fn fts3_is_id_char(c: u8) -> bool {
    (c & 0x80) != 0 || IS_FTS_ID_CHAR[c as usize] != 0
}

/// `sqlite3Fts3NextToken`: o próximo token do texto `z_str` (uma cadeia C: o fim da fatia é o
/// NUL). Um token é uma palavra de identificador, um texto entre aspas (`'`, `"` ou `` ` ``, com
/// a aspa dobrada como escape) ou entre colchetes. Devolve o deslocamento do começo do token e o
/// comprimento dele, ou `None` se não há mais tokens.
pub fn fts3_next_token(z_str: &[u8]) -> Option<(usize, usize)> {
    let mut z1 = 0usize;
    let mut z2: Option<usize> = None;

    /* Procura o começo do próximo token. */
    while z2.is_none() {
        let c = at(z_str, z1);
        match c {
            0 => return None, /* Não há mais tokens aqui. */
            b'\'' | b'"' | b'`' => {
                let mut p = z1;
                loop {
                    p += 1;
                    if at(z_str, p) == 0 {
                        break;
                    }
                    if at(z_str, p) != c {
                        continue;
                    }
                    p += 1;
                    if at(z_str, p) == c {
                        continue;
                    }
                    break;
                }
                z2 = Some(p);
            }
            b'[' => {
                let mut p = z1 + 1;
                while at(z_str, p) != 0 && at(z_str, p) != b']' {
                    p += 1;
                }
                if at(z_str, p) != 0 {
                    p += 1;
                }
                z2 = Some(p);
            }
            _ => {
                if fts3_is_id_char(c) {
                    let mut p = z1 + 1;
                    while fts3_is_id_char(at(z_str, p)) {
                        p += 1;
                    }
                    z2 = Some(p);
                } else {
                    z1 += 1;
                }
            }
        }
    }

    Some((z1, z2.unwrap_or(z1) - z1))
}

/// `sqlite3Fts3InitTokenizer`: cria o tokenizador descrito por `z_arg`, o texto da cláusula
/// `tokenize=` (o nome do tokenizador e depois os argumentos dele). Em erro deixa a mensagem em
/// `pz_err`.
pub fn fts3_init_tokenizer(
    p_hash: &Fts3HashWrapper,
    z_arg: &[u8],
    pz_err: &mut Option<Vec<u8>>,
) -> Result<Rc<dyn Fts3Tokenizer>, i32> {
    /* `sqlite3_mprintf("%s", zArg)`: a cópia vale até o primeiro NUL. */
    let n_arg = z_arg.iter().position(|&c| c == 0).unwrap_or(z_arg.len());
    let mut z_copy: Vec<u8> = z_arg[..n_arg].to_vec();
    let z_end = z_copy.len();
    z_copy.push(0);

    let (z, n) = fts3_next_token(&z_copy).unwrap_or((0, 0));
    debug_assert!(z + n <= z_end);
    z_copy[z + n] = 0;
    let mut name: Vec<u8> = z_copy[z..z + n].to_vec();
    fts3_dequote(&mut name);

    let Some(m) = p_hash.find_module(&name) else {
        *pz_err = mprintf(b"unknown tokenizer: %s", &[text_arg(&name)]);
        return Err(SQLITE_ERROR);
    };

    let mut a_arg: Vec<Vec<u8>> = Vec::new();
    let mut z = z + n + 1;
    while z < z_end {
        let Some((s, n)) = fts3_next_token(&z_copy[z..]) else {
            break;
        };
        let start = z + s;
        z_copy[start + n] = 0;
        let mut arg: Vec<u8> = z_copy[start..start + n].to_vec();
        fts3_dequote(&mut arg);
        a_arg.push(arg);
        z = start + n + 1;
    }
    match m.create(&a_arg) {
        Ok(tok) => Ok(tok),
        Err(rc) => {
            *pz_err = mprintf(b"unknown tokenizer", &[]);
            Err(rc)
        }
    }
}

/// `sqlite3Fts3InitHashTable`: cria na conexão as duas formas da função SQL `z_name` (1 e 2
/// argumentos), que dão acesso de leitura e escrita ao registro `p_hash`. O `ENABLE_TABLE` do C
/// (a tabela virtual do registro) não é definido.
pub fn fts3_init_hash_table(db: &mut Connection, p_hash: &Rc<Fts3HashWrapper>, z_name: &[u8]) -> i32 {
    let any = SQLITE_UTF8 | SQLITE_DIRECTONLY;
    let mut rc = create_function_api(
        db,
        z_name,
        1,
        any,
        UserData::Ptr(p_hash.clone()),
        Some(fts3_tokenizer_func),
        None,
        None,
        None,
        None,
        None,
    );
    if rc == crate::consts::SQLITE_OK {
        rc = create_function_api(
            db,
            z_name,
            2,
            any,
            UserData::Ptr(p_hash.clone()),
            Some(fts3_tokenizer_func),
            None,
            None,
            None,
            None,
            None,
        );
    }
    rc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(z: &str) -> Vec<String> {
        let z = z.as_bytes();
        let mut out = Vec::new();
        let mut off = 0;
        while let Some((s, n)) = fts3_next_token(&z[off..]) {
            out.push(String::from_utf8(z[off + s..off + s + n].to_vec()).unwrap());
            off += s + n + 1;
            if off > z.len() {
                break;
            }
        }
        out
    }

    #[test]
    fn next_token_shapes() {
        assert_eq!(tok("simple remove_diacritics=1"), vec!["simple", "remove_diacritics", "1"]);
        assert_eq!(tok("'a b' \"c\"\"d\" [e f]"), vec!["'a b'", "\"c\"\"d\"", "[e f]"]);
        assert_eq!(tok("   "), Vec::<String>::new());
    }

    #[test]
    fn id_chars() {
        assert!(fts3_is_id_char(b'a'));
        assert!(fts3_is_id_char(b'$'));
        assert!(fts3_is_id_char(b'_'));
        assert!(fts3_is_id_char(0xC3));
        assert!(!fts3_is_id_char(b'='));
        assert!(!fts3_is_id_char(b' '));
    }

    #[test]
    fn registry_resolves_modules() {
        let h = Fts3HashWrapper::new();
        assert!(h.find_module(b"simple").is_none());
        h.insert_module(b"simple", super::super::tokenizer1::fts3_simple_tokenizer_module());
        assert!(h.find_module(b"simple").is_some());
        let mut err = None;
        let t = fts3_init_tokenizer(&h, b"simple", &mut err);
        assert!(t.is_ok());
        let t = fts3_init_tokenizer(&h, b"nosuch", &mut err);
        assert!(t.is_err());
        assert_eq!(err.as_deref(), Some(b"unknown tokenizer: nosuch".as_slice()));
    }
}
