//! Tradução de `printf.c`: rotinas estilo printf com as extensões do SQLite.
//!
//! O código original data dos anos 1980 e é de domínio público. O acumulador de
//! texto (`StrAccum`, o `sqlite3_str` do C) é autocontido: não guarda ponteiro
//! para `sqlite3`; o limite de tamanho vem em `mx_alloc` e os efeitos que o C
//! faria sobre a conexão (`sqlite3ErrorToParser`, `errByteOffset`) ficam
//! registrados em campos públicos para o integrador aplicar.
//!
//! Nomes: `sqlite3_str_xxx` vira método de `StrAccum` (`append`, `reset`, ...),
//! `sqlite3StrAccumXxx` perde o prefixo (`set_error`, `str_accum_enlarge`).
//! Texto é sempre `&[u8]`/`Vec<u8>`; um "char*" do C termina no primeiro NUL.

use crate::consts::{
    SQLITE_MAX_LENGTH, SQLITE_NOMEM, SQLITE_PRINTF_INTERNAL, SQLITE_PRINTF_MALLOCED,
    SQLITE_PRINTF_SQLFUNC, SQLITE_TOOBIG,
};
use crate::util::{fp_decode, FpDecode};
use std::rc::Rc;

// Tipos de conversão, como na enumeração do C.
const ET_RADIX: u8 = 0; // inteiros não decimais: %x %o
const ET_FLOAT: u8 = 1; // ponto flutuante: %f
const ET_EXP: u8 = 2; // notação exponencial: %e e %E
const ET_GENERIC: u8 = 3; // ponto flutuante ou exponencial conforme o expoente: %g
const ET_SIZE: u8 = 4; // número de caracteres processados até agora: %n
const ET_STRING: u8 = 5; // strings: %s
const ET_DYNSTRING: u8 = 6; // strings alocadas dinamicamente: %z
const ET_PERCENT: u8 = 7; // o símbolo de porcentagem: %%
const ET_CHARX: u8 = 8; // caracteres: %c
// O resto são extensões, normalmente ausentes do printf()
const ET_SQLESCAPE: u8 = 9; // strings com '\'' dobrado: %q
const ET_SQLESCAPE2: u8 = 10; // '\'' dobrado e entre '', NULL vira NULL do SQL: %Q
const ET_TOKEN: u8 = 11; // um ponteiro para Token
const ET_SRCITEM: u8 = 12; // um ponteiro para SrcItem
const ET_POINTER: u8 = 13; // a conversão %p
const ET_SQLESCAPE3: u8 = 14; // %w: strings com '"' dobrado
const ET_ORDINAL: u8 = 15; // %r: 1st, 2nd, 3rd, 4th... só em inglês
const ET_DECIMAL: u8 = 16; // %d ou %u, mas não %x, %o
const ET_INVALID: u8 = 17; // qualquer conversão não reconhecida

/// Tamanho do buffer de conversão (`SQLITE_PRINT_BUF_SIZE`, padrão 70).
pub const SQLITE_PRINT_BUF_SIZE: u32 = 70;
const ET_BUFSIZE: i32 = SQLITE_PRINT_BUF_SIZE as i32;

/// Limite rígido da precisão das conversões de ponto flutuante
/// (`SQLITE_FP_PRECISION_LIMIT`, ativo porque `SQLITE_PRINTF_PRECISION_LIMIT`
/// não é definido no Debian).
const SQLITE_FP_PRECISION_LIMIT: i32 = 100000000;

/// Informação de cada caractere de conversão embutido (o `et_info` do C).
struct EtInfo {
    fmt_type: u8, // a letra do campo de formato
    base: u8,     // a base da conversão radix
    flags: u8,    // uma ou mais constantes FLAG_*
    ty: u8,       // paradigma da conversão
    charset: u8,  // deslocamento em A_DIGITS da string de dígitos
    prefix: u8,   // deslocamento em A_PREFIX da string de prefixo
}

const FLAG_SIGNED: u8 = 1; // o valor a converter tem sinal
#[allow(dead_code)]
const FLAG_STRING: u8 = 4; // permite precisão infinita

const A_DIGITS: &[u8] = b"0123456789ABCDEF0123456789abcdef";
const A_PREFIX: &[u8] = b"-x0\x00X0";

/// A tabela é pesquisada linearmente, então os tipos mais usados vêm primeiro.
const FMT_INFO: [EtInfo; 23] = [
    EtInfo { fmt_type: b'd', base: 10, flags: 1, ty: ET_DECIMAL, charset: 0, prefix: 0 },
    EtInfo { fmt_type: b's', base: 0, flags: 4, ty: ET_STRING, charset: 0, prefix: 0 },
    EtInfo { fmt_type: b'g', base: 0, flags: 1, ty: ET_GENERIC, charset: 30, prefix: 0 },
    EtInfo { fmt_type: b'z', base: 0, flags: 4, ty: ET_DYNSTRING, charset: 0, prefix: 0 },
    EtInfo { fmt_type: b'q', base: 0, flags: 4, ty: ET_SQLESCAPE, charset: 0, prefix: 0 },
    EtInfo { fmt_type: b'Q', base: 0, flags: 4, ty: ET_SQLESCAPE2, charset: 0, prefix: 0 },
    EtInfo { fmt_type: b'w', base: 0, flags: 4, ty: ET_SQLESCAPE3, charset: 0, prefix: 0 },
    EtInfo { fmt_type: b'c', base: 0, flags: 0, ty: ET_CHARX, charset: 0, prefix: 0 },
    EtInfo { fmt_type: b'o', base: 8, flags: 0, ty: ET_RADIX, charset: 0, prefix: 2 },
    EtInfo { fmt_type: b'u', base: 10, flags: 0, ty: ET_DECIMAL, charset: 0, prefix: 0 },
    EtInfo { fmt_type: b'x', base: 16, flags: 0, ty: ET_RADIX, charset: 16, prefix: 1 },
    EtInfo { fmt_type: b'X', base: 16, flags: 0, ty: ET_RADIX, charset: 0, prefix: 4 },
    EtInfo { fmt_type: b'f', base: 0, flags: 1, ty: ET_FLOAT, charset: 0, prefix: 0 },
    EtInfo { fmt_type: b'e', base: 0, flags: 1, ty: ET_EXP, charset: 30, prefix: 0 },
    EtInfo { fmt_type: b'E', base: 0, flags: 1, ty: ET_EXP, charset: 14, prefix: 0 },
    EtInfo { fmt_type: b'G', base: 0, flags: 1, ty: ET_GENERIC, charset: 14, prefix: 0 },
    EtInfo { fmt_type: b'i', base: 10, flags: 1, ty: ET_DECIMAL, charset: 0, prefix: 0 },
    EtInfo { fmt_type: b'n', base: 0, flags: 0, ty: ET_SIZE, charset: 0, prefix: 0 },
    EtInfo { fmt_type: b'%', base: 0, flags: 0, ty: ET_PERCENT, charset: 0, prefix: 0 },
    EtInfo { fmt_type: b'p', base: 16, flags: 0, ty: ET_POINTER, charset: 0, prefix: 1 },
    // Todo o resto é não documentado e de uso interno
    EtInfo { fmt_type: b'T', base: 0, flags: 0, ty: ET_TOKEN, charset: 0, prefix: 0 },
    EtInfo { fmt_type: b'S', base: 0, flags: 0, ty: ET_SRCITEM, charset: 0, prefix: 0 },
    EtInfo { fmt_type: b'r', base: 10, flags: 1, ty: ET_ORDINAL, charset: 0, prefix: 0 },
];

// Notas:
//
//    %S    recebe um ponteiro para SrcItem. Mostra o nome ou banco.nome
//    %!S   como %S, mas prefere o zName ao zAlias

/// Um argumento variádico do formato (o que o `va_arg` do C leria). A leitura é
/// tolerante: um argumento ausente ou de tipo diferente do pedido vale 0 / NULL
/// (no C seria comportamento indefinido).
#[derive(Clone, Debug)]
pub enum PrintfArg {
    /// `int`, `unsigned`, `long`, `long long`, `i64`, `u64`: o conversor decide
    /// o corte (`as i32`/`as u32`) como o `va_arg` do C faria.
    Int(i64),
    /// `double`
    Double(f64),
    /// `char*`. `None` é o ponteiro nulo; o texto vale até o primeiro NUL.
    Text(Option<Vec<u8>>),
    /// `unsigned int` do `%c` (ponto de código Unicode)
    Char(u32),
    /// `int*` do `%n`. O C grava `nChar` no endereço recebido; em Rust o
    /// argumento só é consumido (a gravação não é observável sem o ponteiro).
    Size,
    /// `Token*` do `%T`. `None` é o ponteiro nulo.
    Token(Option<PrintfToken>),
    /// `Expr*` do `%#T`. `None` vale para ponteiro nulo ou `EP_IntValue`
    /// (os dois casos que o C protege com `ALWAYS`).
    ExprToken(Option<PrintfExprToken>),
    /// `SrcItem*` do `%S`
    SrcItem(PrintfSrcItem),
}

/// O `Token` do `%T`: `z` já cortado em `n` bytes.
#[derive(Clone, Debug)]
pub struct PrintfToken {
    /// Os `n` bytes do token (`n == z.len()`; `n == 0` não emite nada).
    pub z: Vec<u8>,
    /// Deslocamento do início do token dentro de `Parse.zTail`, já resolvido
    /// por quem monta o argumento (a conferência `SQLITE_WITHIN` do
    /// `sqlite3RecordErrorByteOffset`), ou `None` se está fora desse texto.
    pub tail_offset: Option<i32>,
}

/// A parte do `Expr` que o `%#T` consome.
#[derive(Clone, Debug)]
pub struct PrintfExprToken {
    /// `Expr.u.zToken`
    pub z_token: Vec<u8>,
    /// Resultado de `sqlite3RecordErrorOffsetOfExpr` já calculado (o primeiro
    /// `iOfst > 0` descendo por `pLeft`), ou `None` se a descida terminou em nulo.
    pub err_offset: Option<i32>,
}

/// Quando um `SrcItem` não tem alias nem nome, o C usa o `Select` do item.
#[derive(Clone, Copy, Debug)]
pub enum PrintfSrcAnon {
    /// `SF_NestedFrom`: `(join-%u)` com `pSel->selId`
    NestedFrom { sel_id: u32 },
    /// `SF_MultiValue`: `%u-ROW VALUES CLAUSE` com `pItem->u1.nRow`
    MultiValue { n_row: u32 },
    /// os demais: `(subquery-%u)` com `pSel->selId`
    Subquery { sel_id: u32 },
}

/// Os campos do `SrcItem` que o `%S` lê (montados por quem chama).
#[derive(Clone, Debug)]
pub struct PrintfSrcItem {
    pub z_alias: Option<Vec<u8>>,
    pub z_name: Option<Vec<u8>>,
    pub z_database: Option<Vec<u8>>,
    /// Só é usado quando `z_alias` e `z_name` são ambos nulos.
    pub anon: PrintfSrcAnon,
}

/// Um valor SQL como argumento do `printf()` da linguagem SQL
/// (`sqlite3_value_int64/double/text` do `PrintfArguments`).
pub trait PrintfValue {
    fn value_int64(&mut self) -> i64;
    fn value_double(&mut self) -> f64;
    fn value_text(&mut self) -> Option<Vec<u8>>;
}

/// O `PrintfArguments` do C: argumentos de função SQL para `SQLITE_PRINTF_SQLFUNC`.
pub struct PrintfArguments<'a> {
    /// `nUsed`
    pub n_used: usize,
    /// `apArg` (`nArg` é `ap_arg.len()`)
    pub ap_arg: Vec<&'a mut dyn PrintfValue>,
}

/// A `va_list` do C: lista simples ou, com `SQLITE_PRINTF_SQLFUNC`, o
/// `PrintfArguments*` que o C lê como primeiro `va_arg`.
pub enum PrintfArgs<'a, 'b> {
    Plain(&'a [PrintfArg]),
    SqlFunc(&'a mut PrintfArguments<'b>),
}

/// Fonte de argumentos da conversão (a `va_list` mais o `pArgList`).
struct ArgSource<'a, 'b> {
    plain: &'a [PrintfArg],
    pos: usize,
    list: Option<&'a mut PrintfArguments<'b>>,
}

impl<'a, 'b> ArgSource<'a, 'b> {
    fn new(args: PrintfArgs<'a, 'b>) -> Self {
        match args {
            PrintfArgs::Plain(p) => ArgSource { plain: p, pos: 0, list: None },
            PrintfArgs::SqlFunc(l) => ArgSource { plain: &[], pos: 0, list: Some(l) },
        }
    }

    /// Próximo argumento da lista simples (o `va_arg`).
    fn next_arg(&mut self) -> Option<&'a PrintfArg> {
        let a = self.plain.get(self.pos);
        if a.is_some() {
            self.pos += 1;
        }
        a
    }

    fn va_i64(&mut self) -> i64 {
        match self.next_arg() {
            Some(PrintfArg::Int(v)) => *v,
            Some(PrintfArg::Char(c)) => *c as i64,
            Some(PrintfArg::Double(d)) => *d as i64,
            _ => 0,
        }
    }

    fn va_double(&mut self) -> f64 {
        match self.next_arg() {
            Some(PrintfArg::Double(d)) => *d,
            Some(PrintfArg::Int(v)) => *v as f64,
            _ => 0.0,
        }
    }

    fn va_text(&mut self) -> Option<Vec<u8>> {
        match self.next_arg() {
            Some(PrintfArg::Text(t)) => t.clone(),
            _ => None,
        }
    }

    fn va_char(&mut self) -> u32 {
        match self.next_arg() {
            Some(PrintfArg::Char(c)) => *c,
            Some(PrintfArg::Int(v)) => *v as u32,
            _ => 0,
        }
    }

    /// `getIntArg`: extra argument values from a PrintfArguments object
    fn get_int_arg(&mut self) -> i64 {
        match self.list.as_deref_mut() {
            Some(p) if p.ap_arg.len() > p.n_used => {
                let i = p.n_used;
                p.n_used += 1;
                p.ap_arg[i].value_int64()
            }
            _ => 0,
        }
    }

    fn get_double_arg(&mut self) -> f64 {
        match self.list.as_deref_mut() {
            Some(p) if p.ap_arg.len() > p.n_used => {
                let i = p.n_used;
                p.n_used += 1;
                p.ap_arg[i].value_double()
            }
            _ => 0.0,
        }
    }

    fn get_text_arg(&mut self) -> Option<Vec<u8>> {
        match self.list.as_deref_mut() {
            Some(p) if p.ap_arg.len() > p.n_used => {
                let i = p.n_used;
                p.n_used += 1;
                p.ap_arg[i].value_text()
            }
            _ => None,
        }
    }
}

/// Tamanho do texto C (até o primeiro NUL, ou o fim do slice).
fn c_str_len(z: &[u8]) -> usize {
    z.iter().position(|&b| b == 0).unwrap_or(z.len())
}

/// Byte `i` de um texto terminado em NUL: o fim do slice lê 0.
#[inline]
fn at(z: &[u8], i: usize) -> u8 {
    z.get(i).copied().unwrap_or(0)
}

/// Tamanho que o alocador devolve para um pedido de `n` bytes: o `sqlite3Realloc`
/// arredonda para 8 (`sqlite3MemRoundup`) e o `sqlite3MallocSize` do Debian usa
/// `malloc_usable_size` da glibc (`HAVE_MALLOC_USABLE_SIZE`).
pub(crate) fn malloc_size(n: u64) -> u32 {
    let r = (n + 7) & !7;
    let usable = (((r + 8 + 15) & !15) - 8).max(24);
    usable as u32
}

/// O acumulador de texto (`StrAccum`, também `sqlite3_str`).
///
/// Invariante: `text.len() == n_char`. `n_alloc` é o tamanho LÓGICO do buffer do
/// C (inclui o byte NUL final), que governa quando e quanto o buffer cresce.
pub struct StrAccum {
    /// Os `n_char` bytes acumulados (o `zText` do C, sem o NUL)
    text: Vec<u8>,
    /// `zText != 0`
    has_text: bool,
    /// Tamanho do buffer atual (`nAlloc`)
    pub n_alloc: u32,
    /// Máximo a acumular; 0 significa buffer fixo, sem alocação (`mxAlloc`)
    pub mx_alloc: u32,
    /// Bytes acumulados (`nChar`)
    pub n_char: u32,
    /// 0, `SQLITE_NOMEM` ou `SQLITE_TOOBIG` (`accError`)
    pub acc_error: u8,
    /// `SQLITE_PRINTF_INTERNAL`, `SQLITE_PRINTF_SQLFUNC`, `SQLITE_PRINTF_MALLOCED`
    pub printf_flags: u8,
    /// O `db` do C era não nulo. Usado só para o `sqlite3ErrorToParser` do TOOBIG.
    pub has_db: bool,
    /// Verdadeiro quando um erro TOOBIG exige `sqlite3ErrorToParser(db, SQLITE_TOOBIG)`
    /// do integrador (só acontece com `has_db`).
    pub toobig_to_parser: bool,
    /// Pedido de `db->errByteOffset` vindo de `%T` (`sqlite3RecordErrorByteOffset`):
    /// o integrador grava só se `db->errByteOffset` ainda for -2.
    pub err_byte_offset_token: Option<i32>,
    /// Pedido de `db->errByteOffset` vindo de `%#T`
    /// (`sqlite3RecordErrorOffsetOfExpr`): o integrador grava sempre.
    pub err_byte_offset_expr: Option<i32>,
    /// É o singleton `sqlite3OomStr`
    oom_singleton: bool,
}

impl StrAccum {
    /// `sqlite3StrAccumInit` sem buffer inicial (`zBase == 0`, `n == 0`), como o
    /// `sqlite3_str_new`: `mx_alloc` é o limite (`SQLITE_LIMIT_LENGTH` da conexão
    /// ou `SQLITE_MAX_LENGTH`).
    pub fn new(mx_alloc: u32) -> StrAccum {
        StrAccum {
            text: Vec::new(),
            has_text: false,
            n_alloc: 0,
            mx_alloc,
            n_char: 0,
            acc_error: 0,
            printf_flags: 0,
            has_db: false,
            toobig_to_parser: false,
            err_byte_offset_token: None,
            err_byte_offset_expr: None,
            oom_singleton: false,
        }
    }

    /// `sqlite3StrAccumInit` com um buffer inicial de `n_base` bytes (o `zBase`
    /// da pilha ou o `zBuf` do snprintf). Com `mx_alloc == 0` o buffer nunca cresce.
    pub fn with_base(n_base: u32, mx_alloc: u32) -> StrAccum {
        let mut p = StrAccum::new(mx_alloc);
        p.has_text = true;
        p.n_alloc = n_base;
        p
    }

    /// O singleton `sqlite3OomStr`: não aceita texto e sempre devolve SQLITE_NOMEM.
    pub fn oom() -> StrAccum {
        let mut p = StrAccum::new(0);
        p.acc_error = SQLITE_NOMEM as u8;
        p.oom_singleton = true;
        p
    }

    fn is_malloced(&self) -> bool {
        self.printf_flags & SQLITE_PRINTF_MALLOCED != 0
    }

    /// `sqlite3StrAccumSetError`: põe o acumulador em modo de erro.
    pub fn set_error(&mut self, e_error: u8) {
        debug_assert!(e_error as i32 == SQLITE_NOMEM || e_error as i32 == SQLITE_TOOBIG);
        self.acc_error = e_error;
        if self.mx_alloc != 0 {
            self.reset();
        }
        if e_error as i32 == SQLITE_TOOBIG && self.has_db {
            self.toobig_to_parser = true;
        }
    }

    /// `printfTempBuf`: memória temporária para a renderização. Devolve um
    /// `Vec` vazio com capacidade `n`, ou `None` (com o erro registrado) se o
    /// pedido passa do limite do acumulador ou a alocação falha.
    fn printf_temp_buf(&mut self, n: i64) -> Option<Vec<u8>> {
        if self.acc_error != 0 {
            return None;
        }
        if n > self.n_alloc as i64 && n > self.mx_alloc as i64 {
            self.set_error(SQLITE_TOOBIG as u8);
            return None;
        }
        let mut z: Vec<u8> = Vec::new();
        if z.try_reserve_exact(n as usize).is_err() {
            self.set_error(SQLITE_NOMEM as u8);
            return None;
        }
        Some(z)
    }

    /// `sqlite3StrAccumEnlarge`: aumenta a alocação para aceitar pelo menos `n`
    /// bytes a mais. Devolve quantos bytes o acumulador aceita depois da
    /// tentativa (pode ser zero).
    pub fn str_accum_enlarge(&mut self, n: i64) -> i32 {
        debug_assert!(self.n_char as i64 + n >= self.n_alloc as i64);
        if self.acc_error != 0 {
            return 0;
        }
        if self.mx_alloc == 0 {
            self.set_error(SQLITE_TOOBIG as u8);
            return self.n_alloc.wrapping_sub(self.n_char).wrapping_sub(1) as i32;
        }
        let mut sz_new: i64 = self.n_char as i64 + n + 1;
        if sz_new + self.n_char as i64 <= self.mx_alloc as i64 {
            // Força crescimento exponencial enquanto não estoura, para não
            // chamar esta rotina com frequência
            sz_new += self.n_char as i64;
        }
        if sz_new > self.mx_alloc as i64 {
            self.reset();
            self.set_error(SQLITE_TOOBIG as u8);
            return 0;
        }
        self.n_alloc = sz_new as i32 as u32;
        let want = (self.n_alloc as usize).saturating_sub(self.text.len());
        if self.text.try_reserve(want).is_ok() {
            // zText != 0 || nChar == 0; o conteúdo antigo já está em `text`
            self.has_text = true;
            self.n_alloc = malloc_size(sz_new as u64);
            self.printf_flags |= SQLITE_PRINTF_MALLOCED;
        } else {
            self.reset();
            self.set_error(SQLITE_NOMEM as u8);
            return 0;
        }
        debug_assert!(n >= 0 && n <= 0x7fffffff);
        n as i32
    }

    /// `sqlite3_str_appendchar`: acrescenta `n` cópias do caractere `c`.
    pub fn append_char(&mut self, mut n: i32, c: u8) {
        if self.n_char as i64 + n as i64 >= self.n_alloc as i64 {
            n = self.str_accum_enlarge(n as i64);
            if n <= 0 {
                return;
            }
        }
        while n > 0 {
            n -= 1;
            self.text.push(c);
            self.n_char += 1;
        }
    }

    /// `enlargeAndAppend`: o acumulador não comporta `z.len()` bytes novos, então
    /// cresce e depois acrescenta.
    fn enlarge_and_append(&mut self, z: &[u8]) {
        let n = self.str_accum_enlarge(z.len() as i64);
        if n > 0 {
            let n = n as usize;
            self.text.extend_from_slice(&z[..n]);
            self.n_char += n as u32;
        }
    }

    /// `sqlite3_str_append`: acrescenta os bytes de `z`, aumentando a alocação
    /// quando preciso.
    pub fn append(&mut self, z: &[u8]) {
        let n = z.len();
        debug_assert!(self.acc_error == 0 || self.n_alloc == 0 || self.mx_alloc == 0);
        if self.n_char.wrapping_add(n as u32) >= self.n_alloc {
            self.enlarge_and_append(z);
        } else if n > 0 {
            debug_assert!(self.has_text);
            self.text.extend_from_slice(z);
            self.n_char += n as u32;
        }
    }

    /// `sqlite3_str_appendall`: o texto completo de `z` até o primeiro NUL.
    pub fn append_all(&mut self, z: &[u8]) {
        self.append(&z[..c_str_len(z)]);
    }

    /// `sqlite3StrAccumFinish`: devolve o texto resultante, ou `None` se houve
    /// qualquer erro (ou nada foi acumulado em acumulador sem buffer inicial).
    /// O texto é movido para fora: uma segunda chamada devolve `None`; `n_char`
    /// continua valendo.
    pub fn finish(&mut self) -> Option<Vec<u8>> {
        if self.oom_singleton || !self.has_text {
            return None;
        }
        if self.mx_alloc > 0 && !self.is_malloced() {
            // strAccumFinishRealloc: copia o buffer inicial para memória alocada
            let mut z: Vec<u8> = Vec::new();
            if z.try_reserve_exact(self.text.len() + 1).is_ok() {
                z.extend_from_slice(&self.text);
                self.text = z;
                self.printf_flags |= SQLITE_PRINTF_MALLOCED;
            } else {
                self.set_error(SQLITE_NOMEM as u8);
                self.has_text = false;
                self.text = Vec::new();
                return None;
            }
        }
        self.has_text = false;
        Some(std::mem::take(&mut self.text))
    }

    /// `sqlite3_str_finish`: finaliza uma string criada com `StrAccum::new`.
    pub fn str_finish(mut self) -> Option<Vec<u8>> {
        self.finish()
    }

    /// `sqlite3_str_errcode`
    pub fn errcode(&self) -> i32 {
        self.acc_error as i32
    }

    /// `sqlite3_str_length`: o tamanho atual em bytes
    pub fn length(&self) -> i32 {
        self.n_char as i32
    }

    /// `sqlite3_str_value`: o valor atual, ou `None` se está vazio.
    pub fn value(&self) -> Option<&[u8]> {
        if self.n_char == 0 {
            return None;
        }
        Some(&self.text[..self.n_char as usize])
    }

    /// `sqlite3_str_reset`: reclama toda a memória alocada.
    pub fn reset(&mut self) {
        if self.is_malloced() {
            self.printf_flags &= !SQLITE_PRINTF_MALLOCED;
        }
        self.n_alloc = 0;
        self.n_char = 0;
        self.has_text = false;
        self.text = Vec::new();
    }

    /// Remove os `n` primeiros bytes do texto acumulado (o `memmove` de `groupConcatInverse`):
    /// com `n` maior ou igual a `n_char` o acumulador fica vazio.
    pub fn remove_prefix(&mut self, n: u32) {
        if n >= self.n_char {
            self.n_char = 0;
            self.text.clear();
        } else {
            self.n_char -= n;
            self.text.drain(..n as usize);
        }
    }

    /// Adoção do buffer do `%z` em `sqlite3_mprintf("%z...")`: estende uma
    /// alocação existente em vez de criar outra. `nAlloc` do C é
    /// `sqlite3DbMallocSize(bufpt)`, aqui o tamanho da glibc para `len + 1`.
    fn adopt_dyn_string(&mut self, t: Vec<u8>) {
        debug_assert!(!self.is_malloced());
        self.n_alloc = malloc_size(t.len() as u64 + 1);
        self.n_char = 0x7fffffff & t.len() as u32;
        self.text = t;
        self.has_text = true;
        self.printf_flags |= SQLITE_PRINTF_MALLOCED;
    }

    /// `sqlite3_str_appendf`: o `sqlite3_str_vappendf` com argumentos simples.
    pub fn appendf(&mut self, fmt: &[u8], args: &[PrintfArg]) {
        self.str_vappendf(fmt, PrintfArgs::Plain(args));
    }

    /// `sqlite3_str_vappendf`: renderiza `fmt` no acumulador. Um caractere de
    /// conversão inválido (ou um `%T`/`%S` sem `SQLITE_PRINTF_INTERNAL`) encerra
    /// a renderização, como o `return` do C.
    pub fn str_vappendf(&mut self, fmt: &[u8], args: PrintfArgs<'_, '_>) {
        let b_arg_list = self.printf_flags & SQLITE_PRINTF_SQLFUNC != 0;
        let mut src = ArgSource::new(args);
        let mut fi: usize = 0; // posição em fmt
        loop {
            let mut c = at(fmt, fi);
            if c == 0 {
                break;
            }
            if c != b'%' {
                let start = fi;
                while at(fmt, fi) != 0 && at(fmt, fi) != b'%' {
                    fi += 1;
                }
                self.append(&fmt[start..fi]);
                if at(fmt, fi) == 0 {
                    break;
                }
            }
            fi += 1;
            c = at(fmt, fi);
            if c == 0 {
                self.append(b"%");
                break;
            }
            // Descobre quais flags estão presentes
            let mut flag_leftjustify = false; // "-"
            let mut flag_prefix: u8 = 0; // '+' ou ' ' ou 0
            let mut c_thousand: u8 = 0; // separador de milhar de %d e %u
            let mut flag_alternateform = false; // "#"
            let mut flag_altform2 = false; // "!"
            let mut flag_zeropad = false; // largura começa com zero
            let mut flag_long: u8 = 0; // 1 para "l", 2 para "ll"
            let mut width: i32 = 0;
            let mut precision: i32 = -1;
            loop {
                let mut done = false;
                match c {
                    b'-' => flag_leftjustify = true,
                    b'+' => flag_prefix = b'+',
                    b' ' => flag_prefix = b' ',
                    b'#' => flag_alternateform = true,
                    b'!' => flag_altform2 = true,
                    b'0' => flag_zeropad = true,
                    b',' => c_thousand = b',',
                    b'l' => {
                        flag_long = 1;
                        fi += 1;
                        c = at(fmt, fi);
                        if c == b'l' {
                            fi += 1;
                            c = at(fmt, fi);
                            flag_long = 2;
                        }
                        done = true;
                    }
                    b'1'..=b'9' => {
                        let mut wx: u32 = (c - b'0') as u32;
                        loop {
                            fi += 1;
                            c = at(fmt, fi);
                            if !c.is_ascii_digit() {
                                break;
                            }
                            wx = wx.wrapping_mul(10).wrapping_add((c - b'0') as u32);
                        }
                        width = (wx & 0x7fffffff) as i32;
                        if c != b'.' && c != b'l' {
                            done = true;
                        } else {
                            fi -= 1;
                        }
                    }
                    b'*' => {
                        width = if b_arg_list { src.get_int_arg() as i32 } else { src.va_i64() as i32 };
                        if width < 0 {
                            flag_leftjustify = true;
                            width = if width >= -2147483647 { -width } else { 0 };
                        }
                        c = at(fmt, fi + 1);
                        if c != b'.' && c != b'l' {
                            fi += 1;
                            c = at(fmt, fi);
                            done = true;
                        }
                    }
                    b'.' => {
                        fi += 1;
                        c = at(fmt, fi);
                        if c == b'*' {
                            precision =
                                if b_arg_list { src.get_int_arg() as i32 } else { src.va_i64() as i32 };
                            if precision < 0 {
                                precision = if precision >= -2147483647 { -precision } else { -1 };
                            }
                            fi += 1;
                            c = at(fmt, fi);
                        } else {
                            let mut px: u32 = 0;
                            while c.is_ascii_digit() {
                                px = px.wrapping_mul(10).wrapping_add((c - b'0') as u32);
                                fi += 1;
                                c = at(fmt, fi);
                            }
                            precision = (px & 0x7fffffff) as i32;
                        }
                        if c == b'l' {
                            fi -= 1;
                        } else {
                            done = true;
                        }
                    }
                    _ => done = true,
                }
                if done {
                    break;
                }
                fi += 1;
                c = at(fmt, fi);
                if c == 0 {
                    break;
                }
            }

            // Busca a entrada de informação do campo
            let mut infop = &FMT_INFO[0];
            let mut xtype = ET_INVALID;
            for info in FMT_INFO.iter() {
                if c == info.fmt_type {
                    infop = info;
                    xtype = info.ty;
                    break;
                }
            }

            // Neste ponto: flag_alternateform ('#'), flag_altform2 ('!'),
            // flag_prefix, flag_leftjustify (ou largura negativa), flag_zeropad,
            // flag_long, width >= 0, precision >= -1, xtype e infop.
            debug_assert!(width >= 0);
            debug_assert!(precision >= -1);

            // O texto da conversão, em ordem normal, e a largura do campo
            let out: Vec<u8> = match xtype {
                ET_POINTER | ET_ORDINAL | ET_RADIX | ET_DECIMAL => {
                    if xtype == ET_POINTER {
                        flag_long = 2; // sizeof(char*)==sizeof(i64)
                    }
                    if xtype != ET_DECIMAL {
                        c_thousand = 0;
                    }
                    let mut longvalue: u64;
                    let prefix: u8;
                    if infop.flags & FLAG_SIGNED != 0 {
                        let v: i64 = if b_arg_list {
                            src.get_int_arg()
                        } else if flag_long != 0 {
                            src.va_i64()
                        } else {
                            src.va_i64() as i32 as i64
                        };
                        if v < 0 {
                            longvalue = (!v) as u64;
                            longvalue = longvalue.wrapping_add(1);
                            prefix = b'-';
                        } else {
                            longvalue = v as u64;
                            prefix = flag_prefix;
                        }
                    } else {
                        longvalue = if b_arg_list {
                            src.get_int_arg() as u64
                        } else if flag_long != 0 {
                            src.va_i64() as u64
                        } else {
                            src.va_i64() as u32 as u64
                        };
                        prefix = 0;
                    }
                    if longvalue == 0 {
                        flag_alternateform = false;
                    }
                    if flag_zeropad && precision < width - (prefix != 0) as i32 {
                        precision = width - (prefix != 0) as i32;
                    }
                    let n_out: usize;
                    let mut z_out: Vec<u8>;
                    if precision < ET_BUFSIZE - 10 - ET_BUFSIZE / 3 {
                        n_out = ET_BUFSIZE as usize;
                        z_out = vec![0u8; n_out];
                    } else {
                        let mut n: u64 = precision as u64 + 10;
                        if c_thousand != 0 {
                            n += (precision / 3) as u64;
                        }
                        z_out = match self.printf_temp_buf(n as i64) {
                            Some(v) => v,
                            None => return,
                        };
                        n_out = n as i32 as usize;
                        z_out.resize(n_out, 0);
                    }
                    let end = n_out - 1;
                    let mut bufpt: usize = end;
                    if xtype == ET_ORDINAL {
                        const Z_ORD: &[u8] = b"thstndrd";
                        let mut x = (longvalue % 10) as usize;
                        if x >= 4 || (longvalue / 10) % 10 == 1 {
                            x = 0;
                        }
                        bufpt -= 1;
                        z_out[bufpt] = Z_ORD[x * 2 + 1];
                        bufpt -= 1;
                        z_out[bufpt] = Z_ORD[x * 2];
                    }
                    {
                        let cset = &A_DIGITS[infop.charset as usize..];
                        let base = infop.base as u64;
                        loop {
                            // Converte para ascii
                            bufpt -= 1;
                            z_out[bufpt] = cset[(longvalue % base) as usize];
                            longvalue /= base;
                            if longvalue == 0 {
                                break;
                            }
                        }
                    }
                    let mut length = (end - bufpt) as i32;
                    while precision > length {
                        bufpt -= 1;
                        z_out[bufpt] = b'0'; // completa com zeros
                        length += 1;
                    }
                    if c_thousand != 0 {
                        let mut nn = ((length - 1) / 3) as usize; // número de "," a inserir
                        let mut ix = (length - 1) % 3 + 1;
                        bufpt -= nn;
                        let mut idx: usize = 0;
                        while nn > 0 {
                            z_out[bufpt + idx] = z_out[bufpt + idx + nn];
                            ix -= 1;
                            if ix == 0 {
                                idx += 1;
                                z_out[bufpt + idx] = c_thousand;
                                nn -= 1;
                                ix = 3;
                            }
                            idx += 1;
                        }
                    }
                    if prefix != 0 {
                        bufpt -= 1;
                        z_out[bufpt] = prefix; // sinal
                    }
                    if flag_alternateform && infop.prefix != 0 {
                        // acrescenta "0" ou "0x"
                        let mut pi = infop.prefix as usize;
                        while A_PREFIX[pi] != 0 {
                            bufpt -= 1;
                            z_out[bufpt] = A_PREFIX[pi];
                            pi += 1;
                        }
                    }
                    z_out[bufpt..end].to_vec()
                }
                ET_FLOAT | ET_EXP | ET_GENERIC => {
                    let realvalue: f64 =
                        if b_arg_list { src.get_double_arg() } else { src.va_double() };
                    if precision < 0 {
                        precision = 6; // precisão padrão
                    }
                    if precision > SQLITE_FP_PRECISION_LIMIT {
                        precision = SQLITE_FP_PRECISION_LIMIT;
                    }
                    let mut xt = xtype;
                    let i_round: i32 = if xt == ET_FLOAT {
                        -precision
                    } else if xt == ET_GENERIC {
                        if precision == 0 {
                            precision = 1;
                        }
                        precision
                    } else {
                        precision + 1
                    };
                    let mut s = fp_decode(realvalue, i_round, if flag_altform2 { 26 } else { 16 }, true);
                    let mut special: Option<Vec<u8>> = None;
                    if s.is_special != 0 {
                        if s.is_special == 2 {
                            special = Some(if flag_zeropad { b"null".to_vec() } else { b"NaN".to_vec() });
                        } else if flag_zeropad {
                            s.z_buf[s.z_off] = b'9';
                            s.i_dp = 1000;
                            s.n = 1;
                        } else {
                            let mut b = b"-Inf".to_vec();
                            if s.sign == b'-' {
                                // nada a fazer
                            } else if flag_prefix != 0 {
                                b[0] = flag_prefix;
                            } else {
                                b.remove(0);
                            }
                            special = Some(b);
                        }
                    }
                    if let Some(b) = special {
                        b
                    } else {
                        let prefix: u8 = if s.sign == b'-' { b'-' } else { flag_prefix };
                        let mut exp: i32 = s.i_dp - 1;

                        // Se o tipo é etGENERIC, converte para etEXP ou etFLOAT
                        let flag_rtz: bool;
                        if xt == ET_GENERIC {
                            debug_assert!(precision > 0);
                            precision -= 1;
                            flag_rtz = !flag_alternateform;
                            if exp < -4 || exp > precision {
                                xt = ET_EXP;
                            } else {
                                precision -= exp;
                                xt = ET_FLOAT;
                            }
                        } else {
                            flag_rtz = flag_altform2;
                        }
                        let mut e2: i32 = if xt == ET_EXP { 0 } else { s.i_dp - 1 };
                        let mut buf: Vec<u8>;
                        {
                            // tamanho do buffer temporário necessário
                            let mut sz_buf_needed: i64 =
                                e2.max(0) as i64 + precision as i64 + width as i64 + 15;
                            if c_thousand != 0 && e2 > 0 {
                                sz_buf_needed += ((e2 + 2) / 3) as i64;
                            }
                            if sz_buf_needed > ET_BUFSIZE as i64 {
                                buf = match self.printf_temp_buf(sz_buf_needed) {
                                    Some(v) => v,
                                    None => return,
                                };
                            } else {
                                buf = Vec::new();
                            }
                        }
                        let flag_dp = precision > 0 || flag_alternateform || flag_altform2;
                        // O sinal na frente do número
                        if prefix != 0 {
                            buf.push(prefix);
                        }
                        // Dígitos antes do ponto decimal
                        let mut j: i32 = 0;
                        if e2 < 0 {
                            buf.push(b'0');
                        } else {
                            while e2 >= 0 {
                                buf.push(if j < s.n {
                                    j += 1;
                                    s.z_buf[s.z_off + (j - 1) as usize]
                                } else {
                                    b'0'
                                });
                                if c_thousand != 0 && (e2 % 3) == 0 && e2 > 1 {
                                    buf.push(b',');
                                }
                                e2 -= 1;
                            }
                        }
                        // O ponto decimal
                        if flag_dp {
                            buf.push(b'.');
                        }
                        // Zeros depois do ponto mas antes do primeiro dígito significativo
                        e2 += 1;
                        while e2 < 0 && precision > 0 {
                            buf.push(b'0');
                            precision -= 1;
                            e2 += 1;
                        }
                        // Dígitos significativos depois do ponto decimal
                        loop {
                            let more = precision > 0;
                            precision -= 1;
                            if !more {
                                break;
                            }
                            buf.push(if j < s.n {
                                j += 1;
                                s.z_buf[s.z_off + (j - 1) as usize]
                            } else {
                                b'0'
                            });
                        }
                        // Remove zeros à direita e o "." se nenhum dígito o segue
                        if flag_rtz && flag_dp {
                            while buf.last() == Some(&b'0') {
                                buf.pop();
                            }
                            debug_assert!(!buf.is_empty());
                            if buf.last() == Some(&b'.') {
                                if flag_altform2 {
                                    buf.push(b'0');
                                } else {
                                    buf.pop();
                                }
                            }
                        }
                        // Acrescenta o sufixo "eNNN"
                        if xt == ET_EXP {
                            exp = s.i_dp - 1;
                            buf.push(A_DIGITS[infop.charset as usize]);
                            if exp < 0 {
                                buf.push(b'-');
                                exp = -exp;
                            } else {
                                buf.push(b'+');
                            }
                            if exp >= 100 {
                                buf.push((exp / 100) as u8 + b'0'); // casa das centenas
                                exp %= 100;
                            }
                            buf.push((exp / 10) as u8 + b'0'); // casa das dezenas
                            buf.push((exp % 10) as u8 + b'0'); // casa das unidades
                        }

                        // Caso especial: zeros à esquerda se flag_zeropad e não
                        // está justificado à esquerda
                        let length = buf.len() as i32;
                        if flag_zeropad && !flag_leftjustify && length < width {
                            let n_pad = (width - length) as usize;
                            let at_i = (prefix != 0) as usize;
                            buf.splice(at_i..at_i, std::iter::repeat(b'0').take(n_pad));
                        }
                        buf
                    }
                }
                ET_SIZE => {
                    if !b_arg_list {
                        let _ = src.next_arg(); // o int* do %n
                    }
                    width = 0;
                    Vec::new()
                }
                ET_PERCENT => b"%".to_vec(),
                ET_CHARX => {
                    let mut buf: Vec<u8> = Vec::with_capacity(4);
                    if b_arg_list {
                        match src.get_text_arg() {
                            Some(t) => {
                                let mut k = 0usize;
                                let c0 = at(&t, k);
                                k += 1;
                                buf.push(c0);
                                if (c0 & 0xc0) == 0xc0 {
                                    while buf.len() < 4 && (at(&t, k) & 0xc0) == 0x80 {
                                        buf.push(t[k]);
                                        k += 1;
                                    }
                                }
                            }
                            None => buf.push(0),
                        }
                    } else {
                        let ch: u32 = src.va_char();
                        if ch < 0x00080 {
                            buf.push((ch & 0xff) as u8);
                        } else if ch < 0x00800 {
                            buf.push(0xc0 + ((ch >> 6) & 0x1f) as u8);
                            buf.push(0x80 + (ch & 0x3f) as u8);
                        } else if ch < 0x10000 {
                            buf.push(0xe0 + ((ch >> 12) & 0x0f) as u8);
                            buf.push(0x80 + ((ch >> 6) & 0x3f) as u8);
                            buf.push(0x80 + (ch & 0x3f) as u8);
                        } else {
                            buf.push(0xf0 + ((ch >> 18) & 0x07) as u8);
                            buf.push(0x80 + ((ch >> 12) & 0x3f) as u8);
                            buf.push(0x80 + ((ch >> 6) & 0x3f) as u8);
                            buf.push(0x80 + (ch & 0x3f) as u8);
                        }
                    }
                    let length = buf.len() as i64;
                    if precision > 1 {
                        let mut n_prior: i64 = 1;
                        width = width.wrapping_sub(precision - 1);
                        if width > 1 && !flag_leftjustify {
                            self.append_char(width - 1, b' ');
                            width = 0;
                        }
                        self.append(&buf);
                        precision -= 1;
                        while precision > 1 {
                            if n_prior > (precision - 1) as i64 {
                                n_prior = (precision - 1) as i64;
                            }
                            let n_copy_bytes: i64 = length * n_prior;
                            if n_copy_bytes + self.n_char as i64 >= self.n_alloc as i64 {
                                self.str_accum_enlarge(n_copy_bytes);
                            }
                            if self.acc_error != 0 {
                                break;
                            }
                            let start = (self.n_char as i64 - n_copy_bytes) as usize;
                            let tail = self.text[start..self.n_char as usize].to_vec();
                            self.append(&tail);
                            precision = (precision as i64 - n_prior) as i32;
                            n_prior *= 2;
                        }
                    }
                    flag_altform2 = true;
                    width = adjust_width_for_utf8(flag_altform2, width, &buf);
                    buf
                }
                ET_STRING | ET_DYNSTRING => {
                    let mut xt = xtype;
                    let text = if b_arg_list {
                        xt = ET_STRING;
                        src.get_text_arg()
                    } else {
                        src.va_text()
                    };
                    let mut out: Vec<u8> = Vec::new();
                    if let Some(mut t) = text {
                        t.truncate(c_str_len(&t));
                        if xt == ET_DYNSTRING
                            && self.n_char == 0
                            && self.mx_alloc != 0
                            && width == 0
                            && precision < 0
                            && self.acc_error == 0
                        {
                            // Otimização do sqlite3_mprintf("%z..."): estende uma
                            // alocação existente em vez de criar outra
                            self.adopt_dyn_string(t);
                        } else {
                            let length: usize;
                            if precision >= 0 {
                                if flag_altform2 {
                                    // número de bytes para exibir `precision` caracteres
                                    let mut z = 0usize;
                                    while precision > 0 && at(&t, z) != 0 {
                                        precision -= 1;
                                        // SQLITE_SKIP_UTF8
                                        let first = at(&t, z);
                                        z += 1;
                                        if first >= 0xc0 {
                                            while (at(&t, z) & 0xc0) == 0x80 {
                                                z += 1;
                                            }
                                        }
                                    }
                                    length = z;
                                } else {
                                    let mut l = 0usize;
                                    while (l as i64) < precision as i64 && at(&t, l) != 0 {
                                        l += 1;
                                    }
                                    length = l;
                                }
                            } else {
                                length = t.len();
                            }
                            t.truncate(length);
                            width = adjust_width_for_utf8(flag_altform2, width, &t);
                            out = t;
                        }
                    }
                    out
                }
                ET_SQLESCAPE | ET_SQLESCAPE2 | ET_SQLESCAPE3 => {
                    // %q: escapa ' ; %Q: escapa ' e envolve em '...' ; %w: escapa "
                    let q: u8 = if xtype == ET_SQLESCAPE3 { b'"' } else { b'\'' };
                    let arg = if b_arg_list { src.get_text_arg() } else { src.va_text() };
                    let isnull = arg.is_none();
                    let escarg: Vec<u8> = match arg {
                        Some(mut t) => {
                            t.truncate(c_str_len(&t));
                            t
                        }
                        None => {
                            if xtype == ET_SQLESCAPE2 {
                                b"NULL".to_vec()
                            } else {
                                b"(NULL)".to_vec()
                            }
                        }
                    };
                    // Em %q, %Q e %w a precisão é o número de bytes (ou de
                    // caracteres, com a flag !) a usar da entrada.
                    let mut k: i64 = precision as i64;
                    let mut i: usize = 0;
                    let mut n: i64 = 0;
                    while k != 0 {
                        let ch = at(&escarg, i);
                        if ch == 0 {
                            break;
                        }
                        if ch == q {
                            n += 1;
                        }
                        if flag_altform2 && (ch & 0xc0) == 0xc0 {
                            while (at(&escarg, i + 1) & 0xc0) == 0x80 {
                                i += 1;
                            }
                        }
                        i += 1;
                        k -= 1;
                    }
                    let need_quote = !isnull && xtype == ET_SQLESCAPE2;
                    n += i as i64 + 3;
                    let mut buf: Vec<u8> = if n > ET_BUFSIZE as i64 {
                        match self.printf_temp_buf(n) {
                            Some(v) => v,
                            None => return,
                        }
                    } else {
                        Vec::new()
                    };
                    if need_quote {
                        buf.push(q);
                    }
                    for &ch in &escarg[..i] {
                        buf.push(ch);
                        if ch == q {
                            buf.push(ch);
                        }
                    }
                    if need_quote {
                        buf.push(q);
                    }
                    width = adjust_width_for_utf8(flag_altform2, width, &buf);
                    buf
                }
                ET_TOKEN => {
                    if self.printf_flags & SQLITE_PRINTF_INTERNAL == 0 {
                        return;
                    }
                    if flag_alternateform {
                        // %#T é um ponteiro de Expr que usa Expr.u.zToken
                        if let Some(PrintfArg::ExprToken(Some(e))) = src.next_arg() {
                            self.append_all(&e.z_token);
                            if e.err_offset.is_some() {
                                self.err_byte_offset_expr = e.err_offset;
                            }
                        }
                    } else {
                        // %T é um ponteiro de Token
                        debug_assert!(!b_arg_list);
                        if let Some(PrintfArg::Token(Some(t))) = src.next_arg() {
                            if !t.z.is_empty() {
                                self.append(&t.z);
                                if self.err_byte_offset_token.is_none() {
                                    self.err_byte_offset_token = t.tail_offset;
                                }
                            }
                        }
                    }
                    width = 0;
                    Vec::new()
                }
                ET_SRCITEM => {
                    if self.printf_flags & SQLITE_PRINTF_INTERNAL == 0 {
                        return;
                    }
                    debug_assert!(!b_arg_list);
                    if let Some(PrintfArg::SrcItem(item)) = src.next_arg() {
                        if let (Some(alias), false) = (&item.z_alias, flag_altform2) {
                            self.append_all(alias);
                        } else if let Some(name) = &item.z_name {
                            if let Some(db) = &item.z_database {
                                self.append_all(db);
                                self.append(b".");
                            }
                            self.append_all(name);
                        } else if let Some(alias) = &item.z_alias {
                            self.append_all(alias);
                        } else {
                            match item.anon {
                                PrintfSrcAnon::NestedFrom { sel_id } => {
                                    self.appendf(b"(join-%u)", &[PrintfArg::Int(sel_id as i64)]);
                                }
                                PrintfSrcAnon::MultiValue { n_row } => {
                                    self.appendf(
                                        b"%u-ROW VALUES CLAUSE",
                                        &[PrintfArg::Int(n_row as i64)],
                                    );
                                }
                                PrintfSrcAnon::Subquery { sel_id } => {
                                    self.appendf(b"(subquery-%u)", &[PrintfArg::Int(sel_id as i64)]);
                                }
                            }
                        }
                    }
                    width = 0;
                    Vec::new()
                }
                _ => {
                    debug_assert!(xtype == ET_INVALID);
                    return;
                }
            }; // fim do switch sobre o tipo de formato

            // O texto da conversão está em `out`. Largura e comprimento estão em
            // bytes; com a flag "!" nas strings eles já foram traduzidos.
            width = width.wrapping_sub(out.len() as i32);
            if width > 0 {
                if !flag_leftjustify {
                    self.append_char(width, b' ');
                }
                self.append(&out);
                if flag_leftjustify {
                    self.append_char(width, b' ');
                }
            } else {
                self.append(&out);
            }
            fi += 1;
        } // fim do laço sobre a string de formato
    }
}

/// `adjust_width_for_utf8`: com a flag "!" a largura conta caracteres, então
/// soma à largura os bytes de continuação UTF-8 do texto.
fn adjust_width_for_utf8(flag_altform2: bool, mut width: i32, bufpt: &[u8]) -> i32 {
    if flag_altform2 && width > 0 {
        for &b in bufpt.iter().rev() {
            if (b & 0xc0) == 0x80 {
                width = width.wrapping_add(1);
            }
        }
    }
    width
}

/// `sqlite3VMPrintf`/`sqlite3MPrintf`: imprime em memória nova com as extensões
/// internas (`%T`, `%S`). `mx_alloc` é `db->aLimit[SQLITE_LIMIT_LENGTH]`.
/// Devolve o texto (ou `None`) e o acumulador já finalizado: se `acc_error` for
/// `SQLITE_NOMEM` o chamador faz `sqlite3OomFault(db)`; `toobig_to_parser` e os
/// `err_byte_offset_*` dizem o que aplicar à conexão.
pub fn vm_printf(mx_alloc: u32, fmt: &[u8], args: &[PrintfArg]) -> (Option<Vec<u8>>, StrAccum) {
    let mut acc = StrAccum::with_base(SQLITE_PRINT_BUF_SIZE, mx_alloc);
    acc.has_db = true;
    acc.printf_flags = SQLITE_PRINTF_INTERNAL;
    acc.appendf(fmt, args);
    let z = acc.finish();
    (z, acc)
}

/// `sqlite3ResultStrAccum`: usa o conteúdo do acumulador como resultado de uma função SQL.
pub fn result_str_accum(ctx: &mut crate::connection::Context<'_>, p: &mut StrAccum) {
    if p.acc_error != 0 {
        crate::vdbeapi::result_error_code(ctx, p.acc_error as i32);
        p.reset();
    } else if p.is_malloced() {
        let n = p.n_char as i32;
        let z = p.finish().unwrap_or_default();
        crate::vdbeapi::result_text(ctx, Some(&z), n, crate::mem::StrDtor::Dynamic);
    } else {
        crate::vdbeapi::result_text(ctx, Some(b""), 0, crate::mem::StrDtor::Static);
        p.reset();
    }
}

/// `sqlite3_mprintf`/`sqlite3_vmprintf`: imprime em memória nova SEM as
/// extensões internas. Devolve `None` em erro.
pub fn mprintf(fmt: &[u8], args: &[PrintfArg]) -> Option<Vec<u8>> {
    let mut acc = StrAccum::with_base(SQLITE_PRINT_BUF_SIZE, SQLITE_MAX_LENGTH as u32);
    acc.appendf(fmt, args);
    acc.finish()
}

/// `sqlite3_snprintf` sem o buffer do chamador: renderiza com no máximo `n - 1`
/// bytes (o limite é fixo, sem alocar) e devolve o texto sem o NUL final.
/// `n <= 0` devolve vazio.
pub fn snprintf(n: i32, fmt: &[u8], args: &[PrintfArg]) -> Vec<u8> {
    if n <= 0 {
        return Vec::new();
    }
    let mut acc = StrAccum::with_base(n as u32, 0);
    acc.appendf(fmt, args);
    acc.finish().unwrap_or_default()
}

/// `sqlite3_snprintf`/`sqlite3_vsnprintf` escrevendo em `z_buf`: grava o texto
/// e o NUL final (`zBuf[acc.nChar] = 0`) e devolve quantos bytes de texto
/// escreveu. `n` é limitado ao tamanho de `z_buf`.
pub fn snprintf_into(n: i32, z_buf: &mut [u8], fmt: &[u8], args: &[PrintfArg]) -> usize {
    let n = n.min(z_buf.len() as i32);
    if n <= 0 {
        return 0;
    }
    let text = snprintf(n, fmt, args);
    z_buf[..text.len()].copy_from_slice(&text);
    z_buf[text.len()] = 0;
    text.len()
}

/// `renderLogMsg`: formata a mensagem de `sqlite3_log()` num buffer fixo de
/// `SQLITE_PRINT_BUF_SIZE*3` bytes, sem alocar. A entrega ao `xLog` da
/// configuração global fica com `sqlite3_log` (ADIADA).
pub fn render_log_msg(fmt: &[u8], args: &[PrintfArg]) -> Vec<u8> {
    let mut acc = StrAccum::with_base(SQLITE_PRINT_BUF_SIZE * 3, 0);
    acc.appendf(fmt, args);
    acc.finish().unwrap_or_default()
}

/// Cadeia de caracteres com contagem de referências (`RCStr` do C): texto
/// compartilhado e imutável enquanto houver mais de um dono. É o caso em que o
/// C documenta compartilhamento (CONVENTIONS, item 6), por isso `Rc`.
/// `sqlite3RCStrRef` é `Clone` e `sqlite3RCStrUnref` é o `Drop`.
#[derive(Clone, Debug)]
pub struct RcStr(Rc<Vec<u8>>);

impl RcStr {
    /// Os bytes da string (sem o NUL final do C).
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Acesso para escrita, só com contagem de referências 1 (o C exige
    /// `nRCRef==1` em `sqlite3RCStrResize` e escreve na string recém-criada).
    pub fn as_mut_vec(&mut self) -> Option<&mut Vec<u8>> {
        Rc::get_mut(&mut self.0)
    }

    /// Número de referências (`nRCRef`).
    pub fn ref_count(&self) -> usize {
        Rc::strong_count(&self.0)
    }
}

/// `sqlite3RCStrNew`: nova string capaz de guardar `n` bytes (sem o NUL),
/// contagem de referências 1, conteúdo zerado. `None` em falta de memória.
pub fn rc_str_new(n: u64) -> Option<RcStr> {
    let mut v: Vec<u8> = Vec::new();
    v.try_reserve_exact(n as usize).ok()?;
    v.resize(n as usize, 0);
    Some(RcStr(Rc::new(v)))
}

/// `sqlite3RCStrResize`: muda o tamanho para `n` bytes (exige contagem 1).
/// Devolve a string redimensionada ou `None` em falta de memória (e a string
/// original é liberada, como no C).
pub fn rc_str_resize(mut z: RcStr, n: u64) -> Option<RcStr> {
    let v = z.as_mut_vec()?;
    let n = n as usize;
    if n > v.len() {
        v.try_reserve_exact(n - v.len()).ok()?;
    }
    v.resize(n, 0);
    Some(z)
}
