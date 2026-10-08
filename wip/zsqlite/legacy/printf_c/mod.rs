// Mesclado das partes traduzidas de printf_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Tradução de printf.c (trecho 000): constantes de conversão, tabela de
// conversões, argumentos variádicos modelados em Rust e funções auxiliares.
//
// O código original data dos anos 1980 e é de domínio público. Contém rotinas
// estilo printf, com extensões específicas do SQLite.
//
// Nomes: `sqlite3Xxx` perde o prefixo (`str_accum_set_error`), e `sqlite3_xxx`
// perde o `sqlite3_` (`str_append`, `str_reset`, `str_vappendf`).

// Tipos de conversão conforme definido pela enumeração do código original.
pub const ET_RADIX: u8 = 0; // Inteiros não decimais. %x %o
pub const ET_FLOAT: u8 = 1; // Ponto flutuante. %f
pub const ET_EXP: u8 = 2; // Notação exponencial. %e e %E
pub const ET_GENERIC: u8 = 3; // Ponto flutuante ou exponencial, conforme o expoente. %g
pub const ET_SIZE: u8 = 4; // Devolve o número de caracteres processados até agora. %n
pub const ET_STRING: u8 = 5; // Strings. %s
pub const ET_DYNSTRING: u8 = 6; // Strings alocadas dinamicamente. %z
pub const ET_PERCENT: u8 = 7; // Símbolo de porcentagem. %%
pub const ET_CHARX: u8 = 8; // Caracteres. %c
// O resto são extensões, normalmente ausentes do printf()
pub const ET_SQLESCAPE: u8 = 9; // Strings com '\'' dobrado. %q
pub const ET_SQLESCAPE2: u8 = 10; // Strings com '\'' dobrado e entre '', ponteiro nulo vira NULL do SQL. %Q
pub const ET_TOKEN: u8 = 11; // Um ponteiro para uma estrutura Token
pub const ET_SRCITEM: u8 = 12; // Um ponteiro para um SrcItem
pub const ET_POINTER: u8 = 13; // A conversão %p
pub const ET_SQLESCAPE3: u8 = 14; // %w -> Strings com '"' dobrado
pub const ET_ORDINAL: u8 = 15; // %r -> 1st, 2nd, 3rd, 4th, etc. Somente em inglês
pub const ET_DECIMAL: u8 = 16; // %d ou %u, mas não %x, %o

pub const ET_INVALID: u8 = 17; // Qualquer conversão não reconhecida

/// Um "etByte" é um valor sem sinal de 8 bits.
pub type EtByte = u8;

/// Cada caractere de conversão embutido (ex: o 'd' em "%d") é descrito
/// por uma instância desta estrutura.
#[derive(Clone, Copy)]
pub struct EtInfo {
    /// A letra de código do campo de formato
    pub fmt_type: u8,
    /// A base para a conversão radix
    pub base: EtByte,
    /// Uma ou mais das constantes FLAG_ abaixo
    pub flags: EtByte,
    /// Paradigma de conversão
    pub ty: EtByte,
    /// Deslocamento em A_DIGITS da string de dígitos
    pub charset: EtByte,
    /// Deslocamento em A_PREFIX da string de prefixo
    pub prefix: EtByte,
}

// Valores permitidos para EtInfo.flags
pub const FLAG_SIGNED: u8 = 1; // Verdadeiro se o valor a converter tem sinal
pub const FLAG_STRING: u8 = 4; // Permite precisão infinita

/// A tabela a seguir é pesquisada linearmente, então convém pôr os tipos de
/// conversão mais usados primeiro.
pub const A_DIGITS: &[u8] = b"0123456789ABCDEF0123456789abcdef";
pub const A_PREFIX: &[u8] = b"-x0\x00X0";
pub const FMT_INFO: &[EtInfo] = &[
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
    // Sem SQLITE_OMIT_FLOATING_POINT: as entradas de ponto flutuante existem
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
//    %S    Recebe um ponteiro para SrcItem. Mostra o nome ou banco.nome
//    %!S   Como %S, mas prefere o zName ao zAlias

/// Um argumento da lista variádica (`va_list`) do C. Cada variante corresponde
/// a um tipo que o `va_arg` do original lê.
pub enum VaArg<'a> {
    /// `int`
    Int(i32),
    /// `unsigned int`
    UInt(u32),
    /// `long int` e `i64` (no Linux x86_64 os dois têm 64 bits)
    Long(i64),
    /// `unsigned long int` e `u64`
    ULong(u64),
    /// `double`
    Double(f64),
    /// `char*`. `None` é o ponteiro nulo. O texto termina no primeiro byte NUL, se houver.
    Text(Option<Vec<u8>>),
    /// `int*` do %n: recebe o número de caracteres emitidos até agora
    Size(&'a mut i32),
    /// `Expr*` do %#T
    Expr(&'a Expr),
    /// `Token*` do %T. O segundo campo é o deslocamento em bytes do token dentro do texto
    /// de `Parse.zTail`, já resolvido por quem monta o argumento (a conferência
    /// `SQLITE_WITHIN` do C), ou `None` se o token está fora desse texto.
    Token(Option<&'a Token>, Option<usize>),
    /// `SrcItem*` do %S
    SrcItem(&'a SrcItem),
}

/// A `va_list` do C: a fila de argumentos e, para `SQLITE_PRINTF_SQLFUNC`, o
/// objeto `PrintfArguments` que o C lê como primeiro `va_arg`.
#[derive(Default)]
pub struct VaList<'a> {
    pub args: std::collections::VecDeque<VaArg<'a>>,
    pub arg_list: Option<PrintfArguments>,
}

impl Default for PrintfArguments {
    fn default() -> Self {
        PrintfArguments { n_arg: 0, n_used: 0, ap_arg: Vec::new() }
    }
}

impl<'a> VaList<'a> {
    /// Cria uma lista vazia.
    pub fn new() -> Self {
        VaList { args: std::collections::VecDeque::new(), arg_list: None }
    }

    /// `va_arg(ap,int)`
    pub fn int(&mut self) -> i32 {
        match self.args.pop_front() {
            Some(VaArg::Int(v)) => v,
            Some(VaArg::UInt(v)) => v as i32,
            Some(VaArg::Long(v)) => v as i32,
            Some(VaArg::ULong(v)) => v as i32,
            _ => 0,
        }
    }

    /// `va_arg(ap,unsigned int)`
    pub fn uint(&mut self) -> u32 {
        self.int() as u32
    }

    /// `va_arg(ap,i64)` e `va_arg(ap,long int)`
    pub fn long(&mut self) -> i64 {
        match self.args.pop_front() {
            Some(VaArg::Int(v)) => v as i64,
            Some(VaArg::UInt(v)) => v as i64,
            Some(VaArg::Long(v)) => v,
            Some(VaArg::ULong(v)) => v as i64,
            _ => 0,
        }
    }

    /// `va_arg(ap,u64)` e `va_arg(ap,unsigned long int)`
    pub fn ulong(&mut self) -> u64 {
        self.long() as u64
    }

    /// `va_arg(ap,double)`
    pub fn double(&mut self) -> f64 {
        match self.args.pop_front() {
            Some(VaArg::Double(v)) => v,
            _ => 0.0,
        }
    }

    /// `va_arg(ap,char*)`
    pub fn text(&mut self) -> Option<Vec<u8>> {
        match self.args.pop_front() {
            Some(VaArg::Text(v)) => v,
            _ => None,
        }
    }
}

/// Define o objeto StrAccum para um modo de erro.
pub fn str_accum_set_error(p: &mut StrAccum, e_error: u8) {
    debug_assert!(e_error as i32 == SQLITE_NOMEM || e_error as i32 == SQLITE_TOOBIG);
    p.acc_error = e_error;
    if p.mx_alloc != 0 {
        str_reset(p);
    }
    if e_error as i32 == SQLITE_TOOBIG {
        if let Some(db_ref) = p.db.clone() {
            error_to_parser(Some(&mut db_ref.borrow_mut()), e_error as i32);
        }
    }
}

/// Argumentos extras de um objeto PrintfArguments.
pub fn get_int_arg(p: &mut PrintfArguments) -> i64 {
    if p.n_arg <= p.n_used {
        return 0;
    }
    let arg = p.ap_arg[p.n_used as usize].clone();
    p.n_used += 1;
    let v = value_int64(&arg.borrow());
    v
}

pub fn get_double_arg(p: &mut PrintfArguments) -> f64 {
    if p.n_arg <= p.n_used {
        return 0.0;
    }
    let arg = p.ap_arg[p.n_used as usize].clone();
    p.n_used += 1;
    let v = value_double(&arg.borrow());
    v
}

pub fn get_text_arg(p: &mut PrintfArguments) -> Option<Vec<u8>> {
    if p.n_arg <= p.n_used {
        return None;
    }
    let arg = p.ap_arg[p.n_used as usize].clone();
    p.n_used += 1;
    let v = value_text(&mut arg.borrow_mut());
    v
}

/// Aloca memória para um buffer temporário necessário à renderização do printf.
///
/// Se o tamanho pedido para o buffer temporário for maior que o tamanho do
/// buffer de saída em pAccum, causa um erro SQLITE_TOOBIG. A checagem de
/// tamanho vem antes da alocação para impedir que um SQL malicioso peça
/// alocações enormes pelo campo de precisão ou largura da função printf().
pub fn printf_temp_buf(p_accum: &mut StrAccum, n: i64) -> Option<Vec<u8>> {
    if p_accum.acc_error != 0 {
        return None;
    }
    if n > p_accum.n_alloc as i64 && n > p_accum.mx_alloc as i64 {
        str_accum_set_error(p_accum, SQLITE_TOOBIG as u8);
        return None;
    }
    let mut z: Vec<u8> = Vec::new();
    if z.try_reserve_exact(n as usize).is_err() {
        str_accum_set_error(p_accum, SQLITE_NOMEM as u8);
        return None;
    }
    Some(z)
}

// Em máquinas com pouca pilha, SQLITE_PRINT_BUF_SIZE pode ser redefinido para
// algo menor. O padrão vale aqui.
pub const SQLITE_PRINT_BUF_SIZE: i32 = 70;
pub const ET_BUFSIZE: i32 = SQLITE_PRINT_BUF_SIZE; // Tamanho do buffer de saída

// Limite duro da precisão das conversões de ponto flutuante. Como
// SQLITE_PRINTF_PRECISION_LIMIT não é definido, o limite vale só para float.
pub const SQLITE_FP_PRECISION_LIMIT: i32 = 100000000;


// ---- part_001.rs ----

// Tradução de printf.c (trecho 001): `sqlite3_str_vappendf`, o coração do printf.
//
// A função C abrange os trechos 000 e 001 do fatiamento; ela vive inteira aqui.
// Modelo: o texto da conversão que o C deixa em `bufpt` (apontando para `buf`,
// para a memória temporária ou para o argumento) vira o `Vec<u8>` `out`, com
// `length` bytes válidos. O `fmt` é lido por `at(i)`, que devolve 0 depois do
// fim, como o byte terminador do C.

/// Renderiza a string dada por `fmt` no objeto StrAccum.
pub fn str_vappendf(p_accum: &mut StrAccum, fmt: &[u8], ap: &mut VaList<'_>) {
    let at = |i: usize| -> u8 { fmt.get(i).copied().unwrap_or(0) };
    let mut c: u8; // Próximo caractere da string de formato
    let mut precision: i32; // Precisão do campo atual
    let mut length: i32; // Comprimento do campo
    let mut width: i32; // Largura do campo atual
    let mut flag_leftjustify: bool; // Verdadeiro se há a flag "-"
    let mut flag_prefix: u8; // '+' ou ' ' ou 0 como prefixo
    let mut flag_alternateform: bool; // Verdadeiro se há a flag "#"
    let mut flag_altform2: bool; // Verdadeiro se há a flag "!"
    let mut flag_zeropad: bool; // Verdadeiro se a constante de largura começa com zero
    let mut flag_long: u8; // 1 para a flag "l", 2 para "ll", 0 por padrão
    let mut done: bool; // Flag de fim do laço
    let mut c_thousand: u8; // Separador de milhares para %d e %u
    let mut xtype: u8; // Paradigma da conversão
    let b_arg_list: bool; // Verdadeiro para SQLITE_PRINTF_SQLFUNC
    let mut prefix: u8; // Caractere de prefixo. '+' ou '-' ou ' ' ou 0
    let mut longvalue: u64; // Valor para tipos inteiros
    let mut infop: &EtInfo; // A estrutura de informação apropriada
    let mut exp: i32; // Expoente dos números reais
    let mut e2: i32;
    let mut flag_rtz: bool; // Verdadeiro se os zeros finais devem sair
    let mut flag_dp: bool; // Verdadeiro se o ponto decimal deve aparecer

    let mut p_arg_list: PrintfArguments = PrintfArguments::default(); // Argumentos do SQLITE_PRINTF_SQLFUNC
    let mut buf: [u8; ET_BUFSIZE as usize]; // Buffer de conversão
    let mut out: Vec<u8>; // O texto da conversão (o "bufpt" do C)

    // pAccum nunca começa com um buffer vazio obtido de malloc(). Esta
    // pré-condição é exigida pela otimização do mprintf("%z...").
    debug_assert!(p_accum.n_char > 0 || (p_accum.printf_flags & SQLITE_PRINTF_MALLOCED) == 0);

    if (p_accum.printf_flags & SQLITE_PRINTF_SQLFUNC) != 0 {
        p_arg_list = ap.arg_list.take().unwrap_or_default();
        b_arg_list = true;
    } else {
        b_arg_list = false;
    }
    let mut i: usize = 0; // posição em fmt (o ponteiro `fmt` do C)
    loop {
        c = at(i);
        if c == 0 {
            break;
        }
        if c != b'%' {
            let start = i;
            while at(i) != 0 && at(i) != b'%' {
                i += 1;
            }
            str_append(p_accum, &fmt[start..i], (i - start) as i32);
            if at(i) == 0 {
                break;
            }
        }
        i += 1;
        c = at(i);
        if c == 0 {
            str_append(p_accum, b"%", 1);
            break;
        }
        // Descobre quais flags estão presentes
        flag_leftjustify = false;
        flag_prefix = 0;
        c_thousand = 0;
        flag_alternateform = false;
        flag_altform2 = false;
        flag_zeropad = false;
        done = false;
        width = 0;
        flag_long = 0;
        precision = -1;
        loop {
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
                    i += 1;
                    c = at(i);
                    if c == b'l' {
                        i += 1;
                        c = at(i);
                        flag_long = 2;
                    }
                    done = true;
                }
                b'1'..=b'9' => {
                    let mut wx: u32 = (c - b'0') as u32;
                    loop {
                        i += 1;
                        c = at(i);
                        if c.is_ascii_digit() {
                            wx = wx.wrapping_mul(10).wrapping_add((c - b'0') as u32);
                        } else {
                            break;
                        }
                    }
                    width = (wx & 0x7fffffff) as i32;
                    if c != b'.' && c != b'l' {
                        done = true;
                    } else {
                        i -= 1;
                    }
                }
                b'*' => {
                    if b_arg_list {
                        width = get_int_arg(&mut p_arg_list) as i32;
                    } else {
                        width = ap.int();
                    }
                    if width < 0 {
                        flag_leftjustify = true;
                        width = if width >= -2147483647 { -width } else { 0 };
                    }
                    c = at(i + 1);
                    if c != b'.' && c != b'l' {
                        i += 1;
                        c = at(i);
                        done = true;
                    }
                }
                b'.' => {
                    i += 1;
                    c = at(i);
                    if c == b'*' {
                        if b_arg_list {
                            precision = get_int_arg(&mut p_arg_list) as i32;
                        } else {
                            precision = ap.int();
                        }
                        if precision < 0 {
                            precision = if precision >= -2147483647 { -precision } else { -1 };
                        }
                        i += 1;
                        c = at(i);
                    } else {
                        let mut px: u32 = 0;
                        while c.is_ascii_digit() {
                            px = px.wrapping_mul(10).wrapping_add((c - b'0') as u32);
                            i += 1;
                            c = at(i);
                        }
                        precision = (px & 0x7fffffff) as i32;
                    }
                    if c == b'l' {
                        i -= 1;
                    } else {
                        done = true;
                    }
                }
                _ => done = true,
            }
            if done {
                break;
            }
            i += 1;
            c = at(i);
            if c == 0 {
                break;
            }
        }

        // Busca a entrada de informação do campo
        infop = &FMT_INFO[0];
        xtype = ET_INVALID;
        for entry in FMT_INFO.iter() {
            if c == entry.fmt_type {
                infop = entry;
                xtype = infop.ty;
                break;
            }
        }

        // Neste ponto as variáveis estão inicializadas assim:
        //
        //   flag_alternateform  VERDADEIRO se há um '#'.
        //   flag_altform2       VERDADEIRO se há um '!'.
        //   flag_prefix         '+' ou ' ' ou zero
        //   flag_leftjustify    VERDADEIRO se há um '-' ou se a largura foi negativa.
        //   flag_zeropad        VERDADEIRO se a largura começou com 0.
        //   flag_long           1 para "l", 2 para "ll"
        //   width               A largura do campo. Sempre não negativa. O padrão é zero.
        //   precision           A precisão. O padrão é -1.
        //   xtype               A classe da conversão.
        //   infop               A estrutura de informação apropriada.
        debug_assert!(width >= 0);
        debug_assert!(precision >= -1);
        out = Vec::new();
        length = 0;
        // Resolve o `goto adjust_width_for_utf8` do C: `adjust` fica verdadeiro nos
        // ramos que saltam para o rótulo (c, s, z, q, Q, w).
        let mut adjust = false;
        match xtype {
            ET_POINTER | ET_ORDINAL | ET_RADIX | ET_DECIMAL => {
                if xtype == ET_POINTER {
                    flag_long = 2; // sizeof(char*)==sizeof(i64)
                }
                if xtype != ET_DECIMAL {
                    c_thousand = 0;
                }
                if (infop.flags & FLAG_SIGNED) != 0 {
                    let v: i64 = if b_arg_list {
                        get_int_arg(&mut p_arg_list)
                    } else if flag_long != 0 {
                        ap.long()
                    } else {
                        ap.int() as i64
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
                        get_int_arg(&mut p_arg_list) as u64
                    } else if flag_long != 0 {
                        ap.ulong()
                    } else {
                        ap.uint() as u64
                    };
                    prefix = 0;
                }
                if longvalue == 0 {
                    flag_alternateform = false;
                }
                let pfx = (prefix != 0) as i32;
                if flag_zeropad && precision < width - pfx {
                    precision = width - pfx;
                }
                if precision >= ET_BUFSIZE - 10 - ET_BUFSIZE / 3 {
                    let mut n: u64 = precision as u64 + 10;
                    if c_thousand != 0 {
                        n += (precision / 3) as u64;
                    }
                    // Só a checagem de tamanho importa: o texto é montado em `rev`
                    if printf_temp_buf(p_accum, n as i64).is_none() {
                        return;
                    }
                }
                // O C escreve de trás para frente; `rev` guarda o texto invertido
                let mut rev: Vec<u8> = Vec::new();
                if xtype == ET_ORDINAL {
                    const Z_ORD: &[u8] = b"thstndrd";
                    let mut x = (longvalue % 10) as usize;
                    if x >= 4 || (longvalue / 10) % 10 == 1 {
                        x = 0;
                    }
                    rev.push(Z_ORD[x * 2 + 1]);
                    rev.push(Z_ORD[x * 2]);
                }
                {
                    let cset = &A_DIGITS[infop.charset as usize..];
                    let base = infop.base as u64;
                    loop {
                        // Converte para ascii
                        rev.push(cset[(longvalue % base) as usize]);
                        longvalue /= base;
                        if longvalue == 0 {
                            break;
                        }
                    }
                }
                length = rev.len() as i32;
                while precision > length {
                    rev.push(b'0'); // Preenche com zeros
                    length += 1;
                }
                if c_thousand != 0 {
                    // Insere "," a cada três dígitos, contando da direita
                    let total = rev.len();
                    let mut with_commas: Vec<u8> = Vec::with_capacity(total + total / 3);
                    for (k, &ch) in rev.iter().enumerate() {
                        // `rev` está invertido: a posição k é a k-ésima da direita
                        if k > 0 && k % 3 == 0 {
                            with_commas.push(c_thousand);
                        }
                        with_commas.push(ch);
                    }
                    rev = with_commas;
                }
                if prefix != 0 {
                    rev.push(prefix); // Acrescenta o sinal
                }
                if flag_alternateform && infop.prefix != 0 {
                    // Acrescenta "0" ou "0x"
                    let mut k = infop.prefix as usize;
                    while A_PREFIX[k] != 0 {
                        rev.push(A_PREFIX[k]);
                        k += 1;
                    }
                }
                rev.reverse();
                length = rev.len() as i32;
                out = rev;
            }
            ET_FLOAT | ET_EXP | ET_GENERIC => 'float: {
                let realvalue: f64 = if b_arg_list {
                    get_double_arg(&mut p_arg_list)
                } else {
                    ap.double()
                };
                if precision < 0 {
                    precision = 6; // Define a precisão padrão
                }
                if precision > SQLITE_FP_PRECISION_LIMIT {
                    precision = SQLITE_FP_PRECISION_LIMIT;
                }
                let i_round: i32 = if xtype == ET_FLOAT {
                    -precision
                } else if xtype == ET_GENERIC {
                    if precision == 0 {
                        precision = 1;
                    }
                    precision
                } else {
                    precision + 1
                };
                let mut s = FpDecode { sign: 0, is_special: 0, n: 0, i_dp: 0, z: 0, z_buf: [0u8; 24] };
                fp_decode(&mut s, realvalue, i_round, if flag_altform2 { 26 } else { 16 });
                if s.is_special != 0 {
                    if s.is_special == 2 {
                        out = if flag_zeropad { b"null".to_vec() } else { b"NaN".to_vec() };
                        length = out.len() as i32;
                        break 'float;
                    } else if flag_zeropad {
                        s.z_buf[s.z] = b'9';
                        s.i_dp = 1000;
                        s.n = 1;
                    } else {
                        let mut inf: [u8; 4] = *b"-Inf";
                        let mut start = 0usize;
                        if s.sign == b'-' {
                            // sem efeito
                        } else if flag_prefix != 0 {
                            inf[0] = flag_prefix;
                        } else {
                            start = 1;
                        }
                        out = inf[start..].to_vec();
                        length = out.len() as i32;
                        break 'float;
                    }
                }
                if s.sign == b'-' {
                    prefix = b'-';
                } else {
                    prefix = flag_prefix;
                }

                exp = s.i_dp - 1;

                // Se o tipo do campo é etGENERIC, converte para etEXP ou etFLOAT,
                // conforme o caso.
                if xtype == ET_GENERIC {
                    debug_assert!(precision > 0);
                    precision -= 1;
                    flag_rtz = !flag_alternateform;
                    if exp < -4 || exp > precision {
                        xtype = ET_EXP;
                    } else {
                        precision -= exp;
                        xtype = ET_FLOAT;
                    }
                } else {
                    flag_rtz = flag_altform2;
                }
                if xtype == ET_EXP {
                    e2 = 0;
                } else {
                    e2 = s.i_dp - 1;
                }
                {
                    // Tamanho de um buffer temporário necessário
                    let mut sz_buf_needed: i64 = (e2.max(0) as i64) + precision as i64 + width as i64 + 15;
                    if c_thousand != 0 && e2 > 0 {
                        sz_buf_needed += ((e2 + 2) / 3) as i64;
                    }
                    if sz_buf_needed > ET_BUFSIZE as i64 && printf_temp_buf(p_accum, sz_buf_needed).is_none() {
                        return;
                    }
                }
                flag_dp = precision > 0 || flag_alternateform || flag_altform2;
                // O sinal na frente do número
                if prefix != 0 {
                    out.push(prefix);
                }
                // Dígitos antes do ponto decimal
                let mut j: i32 = 0;
                if e2 < 0 {
                    out.push(b'0');
                } else {
                    while e2 >= 0 {
                        if j < s.n {
                            out.push(s.z_buf[s.z + j as usize]);
                            j += 1;
                        } else {
                            out.push(b'0');
                        }
                        if c_thousand != 0 && (e2 % 3) == 0 && e2 > 1 {
                            out.push(b',');
                        }
                        e2 -= 1;
                    }
                }
                // O ponto decimal
                if flag_dp {
                    out.push(b'.');
                }
                // Dígitos "0" depois do ponto decimal e antes do primeiro dígito
                // significativo do número
                e2 += 1;
                while e2 < 0 && precision > 0 {
                    out.push(b'0');
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
                    if j < s.n {
                        out.push(s.z_buf[s.z + j as usize]);
                        j += 1;
                    } else {
                        out.push(b'0');
                    }
                }
                // Remove os zeros finais e o "." se nenhum dígito vier depois dele
                if flag_rtz && flag_dp {
                    while out.last() == Some(&b'0') {
                        out.pop();
                    }
                    debug_assert!(!out.is_empty());
                    if out.last() == Some(&b'.') {
                        if flag_altform2 {
                            out.push(b'0');
                        } else {
                            out.pop();
                        }
                    }
                }
                // Acrescenta o sufixo "eNNN"
                if xtype == ET_EXP {
                    exp = s.i_dp - 1;
                    out.push(A_DIGITS[infop.charset as usize]);
                    if exp < 0 {
                        out.push(b'-');
                        exp = -exp;
                    } else {
                        out.push(b'+');
                    }
                    if exp >= 100 {
                        out.push((exp / 100) as u8 + b'0'); // dígito das centenas
                        exp %= 100;
                    }
                    out.push((exp / 10) as u8 + b'0'); // dígito das dezenas
                    out.push((exp % 10) as u8 + b'0'); // dígito das unidades
                }

                // O número convertido está em `out`. Note que o número está na ordem
                // usual, não invertido como nas conversões de inteiros.
                length = out.len() as i32;

                // Caso especial: acrescenta zeros à esquerda se flag_zeropad está
                // ligada e o campo não é justificado à esquerda
                if flag_zeropad && !flag_leftjustify && length < width {
                    let n_pad = (width - length) as usize;
                    let at_pos = (prefix != 0) as usize;
                    out.splice(at_pos..at_pos, std::iter::repeat(b'0').take(n_pad));
                    length = width;
                }
            }
            ET_SIZE => {
                if !b_arg_list {
                    if let Some(VaArg::Size(dst)) = ap.args.pop_front() {
                        *dst = p_accum.n_char as i32;
                    }
                }
                length = 0;
                width = 0;
            }
            ET_PERCENT => {
                out = vec![b'%'];
                length = 1;
            }
            ET_CHARX => {
                buf = [0u8; ET_BUFSIZE as usize];
                if b_arg_list {
                    let text = get_text_arg(&mut p_arg_list);
                    length = 1;
                    if let Some(text) = text {
                        let mut k = 0usize;
                        let tb = |idx: usize| -> u8 { text.get(idx).copied().unwrap_or(0) };
                        buf[0] = tb(k);
                        c = buf[0];
                        k += 1;
                        if (c & 0xc0) == 0xc0 {
                            while length < 4 && (tb(k) & 0xc0) == 0x80 {
                                buf[length as usize] = tb(k);
                                length += 1;
                                k += 1;
                            }
                        }
                    } else {
                        buf[0] = 0;
                    }
                } else {
                    let ch: u32 = ap.uint();
                    if ch < 0x00080 {
                        buf[0] = (ch & 0xff) as u8;
                        length = 1;
                    } else if ch < 0x00800 {
                        buf[0] = 0xc0 + ((ch >> 6) & 0x1f) as u8;
                        buf[1] = 0x80 + (ch & 0x3f) as u8;
                        length = 2;
                    } else if ch < 0x10000 {
                        buf[0] = 0xe0 + ((ch >> 12) & 0x0f) as u8;
                        buf[1] = 0x80 + ((ch >> 6) & 0x3f) as u8;
                        buf[2] = 0x80 + (ch & 0x3f) as u8;
                        length = 3;
                    } else {
                        buf[0] = 0xf0 + ((ch >> 18) & 0x07) as u8;
                        buf[1] = 0x80 + ((ch >> 12) & 0x3f) as u8;
                        buf[2] = 0x80 + ((ch >> 6) & 0x3f) as u8;
                        buf[3] = 0x80 + (ch & 0x3f) as u8;
                        length = 4;
                    }
                }
                if precision > 1 {
                    let mut n_prior: i64 = 1;
                    width -= precision - 1;
                    if width > 1 && !flag_leftjustify {
                        str_appendchar(p_accum, width - 1, b' ');
                        width = 0;
                    }
                    str_append(p_accum, &buf[..length as usize], length);
                    precision -= 1;
                    while precision > 1 {
                        if n_prior > (precision - 1) as i64 {
                            n_prior = (precision - 1) as i64;
                        }
                        let n_copy_bytes: i64 = length as i64 * n_prior;
                        if n_copy_bytes + p_accum.n_char as i64 >= p_accum.n_alloc as i64 {
                            str_accum_enlarge(p_accum, n_copy_bytes);
                        }
                        if p_accum.acc_error != 0 {
                            break;
                        }
                        let end = p_accum.n_char as usize;
                        let chunk: Vec<u8> = p_accum.z_text[end - n_copy_bytes as usize..end].to_vec();
                        str_append(p_accum, &chunk, n_copy_bytes as i32);
                        precision -= n_prior as i32;
                        n_prior *= 2;
                    }
                }
                out = buf[..length as usize].to_vec();
                flag_altform2 = true;
                adjust = true;
            }
            ET_STRING | ET_DYNSTRING => 'string: {
                let text: Option<Vec<u8>> = if b_arg_list {
                    xtype = ET_STRING;
                    get_text_arg(&mut p_arg_list)
                } else {
                    ap.text()
                };
                // O ponteiro nulo do C vira "", a string vazia, e não passa pela
                // otimização do %z
                let is_null = text.is_none();
                let mut text = text.unwrap_or_default();
                if let Some(nul) = text.iter().position(|&b| b == 0) {
                    text.truncate(nul);
                }
                if !is_null
                    && xtype == ET_DYNSTRING
                    && p_accum.n_char == 0
                    && p_accum.mx_alloc != 0
                    && width == 0
                    && precision < 0
                    && p_accum.acc_error == 0
                {
                    // Otimização especial para sqlite3_mprintf("%z..."): estende uma
                    // alocação existente em vez de criar uma nova.
                    debug_assert!((p_accum.printf_flags & SQLITE_PRINTF_MALLOCED) == 0);
                    p_accum.n_alloc = (text.len() + 1) as u32;
                    p_accum.n_char = 0x7fffffff & (text.len() as u32);
                    p_accum.z_text = text;
                    p_accum.printf_flags |= SQLITE_PRINTF_MALLOCED;
                    length = 0;
                    break 'string;
                }
                if precision >= 0 {
                    if flag_altform2 {
                        // Define length como o número de bytes necessário para mostrar
                        // `precision` caracteres
                        let mut z: usize = 0;
                        while precision > 0 && z < text.len() {
                            precision -= 1;
                            let b = text[z];
                            z += 1;
                            if b >= 0xc0 {
                                while z < text.len() && (text[z] & 0xc0) == 0x80 {
                                    z += 1;
                                }
                            }
                        }
                        length = z as i32;
                    } else {
                        length = 0;
                        while length < precision && (length as usize) < text.len() {
                            length += 1;
                        }
                    }
                } else {
                    length = 0x7fffffff & (text.len() as i32);
                }
                out = text;
                adjust = true;
            }
            ET_SQLESCAPE | ET_SQLESCAPE2 | ET_SQLESCAPE3 => {
                let q: u8 = if xtype == ET_SQLESCAPE3 { b'"' } else { b'\'' }; // Caractere de aspas
                let escarg_opt: Option<Vec<u8>> = if b_arg_list {
                    get_text_arg(&mut p_arg_list)
                } else {
                    ap.text()
                };
                let isnull = escarg_opt.is_none();
                let mut escarg: Vec<u8> = match escarg_opt {
                    Some(t) => t,
                    None => {
                        if xtype == ET_SQLESCAPE2 {
                            b"NULL".to_vec()
                        } else {
                            b"(NULL)".to_vec()
                        }
                    }
                };
                if let Some(nul) = escarg.iter().position(|&b| b == 0) {
                    escarg.truncate(nul);
                }
                let ea = |idx: usize| -> u8 { escarg.get(idx).copied().unwrap_or(0) };
                // Para %q, %Q e %w, a precisão é o número de bytes (ou de caracteres
                // se a flag ! está presente) a usar da entrada. Por causa das aspas
                // inseridas, o número de caracteres de saída pode passar da precisão.
                let mut k: i64 = precision as i64;
                let mut n: i64 = 0;
                let mut ii: usize = 0;
                while k != 0 {
                    let ch = ea(ii);
                    if ch == 0 {
                        break;
                    }
                    if ch == q {
                        n += 1;
                    }
                    if flag_altform2 && (ch & 0xc0) == 0xc0 {
                        while (ea(ii + 1) & 0xc0) == 0x80 {
                            ii += 1;
                        }
                    }
                    ii += 1;
                    k -= 1;
                }
                let need_quote = !isnull && xtype == ET_SQLESCAPE2;
                n += ii as i64 + 3;
                if n > ET_BUFSIZE as i64 && printf_temp_buf(p_accum, n).is_none() {
                    return;
                }
                if need_quote {
                    out.push(q);
                }
                for idx in 0..ii {
                    let ch = ea(idx);
                    out.push(ch);
                    if ch == q {
                        out.push(ch);
                    }
                }
                if need_quote {
                    out.push(q);
                }
                length = out.len() as i32;
                adjust = true;
            }
            ET_TOKEN => {
                if (p_accum.printf_flags & SQLITE_PRINTF_INTERNAL) == 0 {
                    return;
                }
                if flag_alternateform {
                    // %#T significa um ponteiro Expr que usa Expr.u.zToken
                    if let Some(VaArg::Expr(p_expr)) = ap.args.pop_front() {
                        if !expr_has_property(p_expr, EP_INT_VALUE) {
                            if let Some(z_token) = &p_expr.u.z_token {
                                str_appendall(p_accum, z_token);
                            }
                            if let Some(db_ref) = p_accum.db.clone() {
                                record_error_offset_of_expr(&mut db_ref.borrow_mut(), Some(p_expr));
                            }
                        }
                    }
                } else {
                    // %T significa um ponteiro Token
                    debug_assert!(!b_arg_list);
                    if let Some(VaArg::Token(Some(p_token), z_off)) = ap.args.pop_front() {
                        if p_token.n != 0 {
                            str_append(p_accum, &p_token.z, p_token.n as i32);
                            if let Some(db_ref) = p_accum.db.clone() {
                                record_error_byte_offset(&mut db_ref.borrow_mut(), z_off);
                            }
                        }
                    }
                }
                length = 0;
                width = 0;
            }
            ET_SRCITEM => {
                if (p_accum.printf_flags & SQLITE_PRINTF_INTERNAL) == 0 {
                    return;
                }
                debug_assert!(!b_arg_list);
                if let Some(VaArg::SrcItem(p_item)) = ap.args.pop_front() {
                    if !p_item.z_alias.is_empty() && !flag_altform2 {
                        str_appendall(p_accum, &p_item.z_alias);
                    } else if !p_item.z_name.is_empty() {
                        if !p_item.z_database.is_empty() {
                            str_appendall(p_accum, &p_item.z_database);
                            str_append(p_accum, b".", 1);
                        }
                        str_appendall(p_accum, &p_item.z_name);
                    } else if !p_item.z_alias.is_empty() {
                        str_appendall(p_accum, &p_item.z_alias);
                    } else {
                        let p_sel = p_item.p_select.as_ref();
                        debug_assert!(p_sel.is_some()); // Por causa do tag-20240424-1
                        if let Some(p_sel) = p_sel {
                            if (p_sel.sel_flags & SF_NESTEDFROM) != 0 {
                                let msg = format_join_label(p_sel.sel_id);
                                str_appendall(p_accum, &msg);
                            } else if (p_sel.sel_flags & SF_MULTIVALUE) != 0 {
                                debug_assert!(p_item.fg.is_tab_func == 0 && p_item.fg.is_indexed_by == 0);
                                let n_row = match &p_item.u1 {
                                    SrcItemU1::NRow(n) => *n,
                                    _ => 0,
                                };
                                let mut msg = decimal_u32(n_row);
                                msg.extend_from_slice(b"-ROW VALUES CLAUSE");
                                str_appendall(p_accum, &msg);
                            } else {
                                let mut msg = b"(subquery-".to_vec();
                                msg.extend_from_slice(&decimal_u32(p_sel.sel_id));
                                msg.push(b')');
                                str_appendall(p_accum, &msg);
                            }
                        }
                    }
                }
                length = 0;
                width = 0;
            }
            _ => {
                debug_assert!(xtype == ET_INVALID);
                return;
            }
        } // Fim do switch sobre o tipo de formato

        if adjust && flag_altform2 && width > 0 {
            // Ajusta a largura pelos bytes extras dos caracteres UTF-8
            let mut ii: i32 = length - 1;
            while ii >= 0 {
                let b = out[ii as usize];
                ii -= 1;
                if (b & 0xc0) == 0x80 {
                    width += 1;
                }
            }
        }
        // O texto da conversão está em "out" e tem "length" bytes. A largura do
        // campo é "width". Faz a saída. Length e width estão em bytes, não em
        // caracteres, neste ponto. Se a flag "!" estava presente nas conversões de
        // string, indicando largura e precisão em caracteres, os valores já foram
        // traduzidos antes de chegar aqui.
        width -= length;
        if width > 0 {
            if !flag_leftjustify {
                str_appendchar(p_accum, width, b' ');
            }
            str_append(p_accum, &out[..length as usize], length);
            if flag_leftjustify {
                str_appendchar(p_accum, width, b' ');
            }
        } else {
            str_append(p_accum, &out[..length as usize], length);
        }
        i += 1; // o `++fmt` do laço for do C
    } // Fim do laço sobre a string de formato
} // Fim da função

/// Os `%u` das mensagens internas de `%S`: o inteiro sem sinal em decimal.
fn decimal_u32(v: u32) -> Vec<u8> {
    let mut digits: Vec<u8> = Vec::new();
    let mut x = v;
    loop {
        digits.push(b'0' + (x % 10) as u8);
        x /= 10;
        if x == 0 {
            break;
        }
    }
    digits.reverse();
    digits
}

/// A mensagem "(join-%u)" do %S.
fn format_join_label(sel_id: u32) -> Vec<u8> {
    let mut msg = b"(join-".to_vec();
    msg.extend_from_slice(&decimal_u32(sel_id));
    msg.push(b')');
    msg
}


// ---- part_002.rs ----

// Tradução de printf.c (trecho 002): registro de deslocamento de erro, o
// acumulador de string (StrAccum) e as variantes de mprintf.
//
// Modelo do StrAccum: `z_text` guarda exatamente os `n_char` bytes acumulados
// (`z_text.len() == n_char`), sem o NUL final. `n_alloc` continua sendo a
// capacidade lógica do C (é ela que decide quando ampliar e quando estourar
// em modo de buffer fixo, `mx_alloc == 0`). O `zText` nulo do C é `n_alloc == 0`.

/// A string `z` aponta para o primeiro caractere de um token associado a um erro.
/// Se `db` ainda não tem um deslocamento em bytes de erro registrado, tenta
/// calcular o deslocamento em bytes de `z` e grava em `db`.
///
/// `z_off` é o deslocamento de `z` dentro do texto de `Parse.zTail` (a conferência
/// `SQLITE_WITHIN(z,zText,zEnd)` do C é feita por quem monta o argumento; `None`
/// significa que `z` está fora desse texto).
pub fn record_error_byte_offset(db: &mut Sqlite3, z_off: Option<usize>) {
    if db.err_byte_offset != -2 {
        return;
    }
    if db.p_parse.is_none() {
        return;
    }
    if let Some(off) = z_off {
        db.err_byte_offset = off as i32;
    }
}

/// Se `p_expr` tem um deslocamento em bytes para o início de um token, registra
/// isso como o deslocamento do erro.
pub fn record_error_offset_of_expr(db: &mut Sqlite3, p_expr: Option<&Expr>) {
    let mut cur = p_expr;
    while let Some(e) = cur {
        // ExprHasProperty com dois bits exige os dois ligados
        if expr_has_property(e, EP_OUTER_ON | EP_INNER_ON) || e.w.i_ofst <= 0 {
            cur = e.p_left.as_deref();
        } else {
            break;
        }
    }
    let e = match cur {
        Some(e) => e,
        None => return,
    };
    db.err_byte_offset = e.w.i_ofst;
}

/// Amplia a alocação de memória de um objeto StrAccum para que ele consiga
/// aceitar pelo menos N bytes de texto a mais.
///
/// Devolve o número de bytes de texto que o StrAccum consegue aceitar depois da
/// tentativa de ampliação. O valor devolvido pode ser zero.
pub fn str_accum_enlarge(p: &mut StrAccum, n: i64) -> i32 {
    debug_assert!(p.n_char as i64 + n >= p.n_alloc as i64); // Só chamada se realmente preciso
    if p.acc_error != 0 {
        return 0;
    }
    if p.mx_alloc == 0 {
        str_accum_set_error(p, SQLITE_TOOBIG as u8);
        return p.n_alloc as i32 - p.n_char as i32 - 1;
    } else {
        let mut sz_new: i64 = p.n_char as i64 + n + 1;
        if sz_new + p.n_char as i64 <= p.mx_alloc as i64 {
            // Força o crescimento exponencial do buffer enquanto não estourar,
            // para não chamar esta rotina com tanta frequência
            sz_new += p.n_char as i64;
        }
        if sz_new > p.mx_alloc as i64 {
            str_reset(p);
            str_accum_set_error(p, SQLITE_TOOBIG as u8);
            return 0;
        } else {
            p.n_alloc = sz_new as u32;
        }
        let extra = (p.n_alloc as usize).saturating_sub(p.z_text.len());
        if p.z_text.try_reserve_exact(extra).is_ok() {
            p.printf_flags |= SQLITE_PRINTF_MALLOCED;
        } else {
            str_reset(p);
            str_accum_set_error(p, SQLITE_NOMEM as u8);
            return 0;
        }
    }
    debug_assert!(n >= 0 && n <= 0x7fffffff);
    n as i32
}

/// Anexa N cópias do caractere c ao buffer de string dado.
pub fn str_appendchar(p: &mut StrAccum, n: i32, c: u8) {
    let mut n = n;
    if p.n_char as i64 + n as i64 >= p.n_alloc as i64 {
        n = str_accum_enlarge(p, n as i64);
        if n <= 0 {
            return;
        }
    }
    while n > 0 {
        n -= 1;
        p.z_text.push(c);
        p.n_char += 1;
    }
}

/// O StrAccum "p" não é grande o bastante para aceitar N novos bytes de z[].
/// Então amplia primeiro, depois anexa.
///
/// É uma rotina auxiliar de `str_append()` que faz o trabalho do caso especial
/// (ampliar o buffer) com chamada em cauda, para que `str_append()` use a
/// convenção de chamada rápida.
fn enlarge_and_append(p: &mut StrAccum, z: &[u8], n: i32) {
    let n = str_accum_enlarge(p, n as i64);
    if n > 0 {
        p.z_text.extend_from_slice(&z[..n as usize]);
        p.n_char += n as u32;
    }
}

/// Anexa N bytes de texto de z ao objeto StrAccum. Aumenta o tamanho da
/// alocação do StrAccum se necessário.
pub fn str_append(p: &mut StrAccum, z: &[u8], n: i32) {
    debug_assert!(z.len() >= n as usize);
    debug_assert!(p.n_alloc != 0 || p.n_char == 0 || p.acc_error != 0);
    debug_assert!(n >= 0);
    debug_assert!(p.acc_error == 0 || p.n_alloc == 0 || p.mx_alloc == 0);
    if p.n_char as i64 + n as i64 >= p.n_alloc as i64 {
        enlarge_and_append(p, z, n);
    } else if n != 0 {
        p.n_char += n as u32;
        p.z_text.extend_from_slice(&z[..n as usize]);
    }
}

/// Anexa o texto completo da string terminada em zero z[] à string p.
pub fn str_appendall(p: &mut StrAccum, z: &[u8]) {
    str_append(p, z, strlen30_nn(z));
}

/// Termina uma string garantindo que ela termine em zero. Devolve o texto
/// resultante. Devolve None se qualquer tipo de erro ocorreu.
fn str_accum_finish_realloc(p: &mut StrAccum) -> Option<Vec<u8>> {
    debug_assert!(p.mx_alloc > 0 && !is_malloced(p));
    let mut z_text: Vec<u8> = Vec::new();
    if z_text.try_reserve_exact(p.n_char as usize + 1).is_ok() {
        z_text.extend_from_slice(&p.z_text);
        p.printf_flags |= SQLITE_PRINTF_MALLOCED;
        p.z_text = z_text;
        Some(std::mem::take(&mut p.z_text))
    } else {
        str_accum_set_error(p, SQLITE_NOMEM as u8);
        p.z_text = Vec::new();
        None
    }
}

/// Termina a string. O texto devolvido tem `n_char` bytes (o NUL final do C
/// não é guardado). O acumulador fica sem o texto.
pub fn str_accum_finish(p: &mut StrAccum) -> Option<Vec<u8>> {
    if p.n_alloc != 0 {
        if p.mx_alloc > 0 && !is_malloced(p) {
            return str_accum_finish_realloc(p);
        }
        return Some(std::mem::take(&mut p.z_text));
    }
    None
}

/// Usa o conteúdo do StrAccum passado como segundo argumento como resultado de
/// uma função SQL.
pub fn result_str_accum(p_ctx: &mut Sqlite3Context, p: &mut StrAccum) {
    if p.acc_error != 0 {
        result_error_code(p_ctx, p.acc_error as i32);
        str_reset(p);
    } else if is_malloced(p) {
        let n = p.n_char as i32;
        let z = std::mem::take(&mut p.z_text);
        result_text(p_ctx, z, n, Destructor::Dynamic);
    } else {
        result_text(p_ctx, Vec::new(), 0, SQLITE_STATIC);
        str_reset(p);
    }
}

/// Termina uma string criada com `str_new()`.
pub fn str_finish(p: Option<Box<StrAccum>>) -> Option<Vec<u8>> {
    // O singleton sqlite3OomStr do C é representado por None: não aceita texto
    // e sempre devolve SQLITE_NOMEM (ver `str_errcode`). Terminá-lo dá None.
    match p {
        Some(mut p_inner) if !is_oom_str(&p_inner) => str_accum_finish(&mut p_inner),
        _ => None,
    }
}

/// O singleton `sqlite3OomStr` do C: um sqlite3_str que não aceita texto e sempre
/// devolve SQLITE_NOMEM. É reconhecido pelo estado (mx_alloc 0, n_alloc 0 e
/// acc_error NOMEM), que nenhum `str_new` bem sucedido produz.
fn is_oom_str(p: &StrAccum) -> bool {
    p.db.is_none()
        && p.n_alloc == 0
        && p.mx_alloc == 0
        && p.n_char == 0
        && p.acc_error as i32 == SQLITE_NOMEM
        && p.printf_flags == 0
}

/// O objeto devolvido por `str_new()` quando a alocação falha.
pub fn oom_str() -> Box<StrAccum> {
    Box::new(StrAccum {
        db: None,
        z_text: Vec::new(),
        n_alloc: 0,
        mx_alloc: 0,
        n_char: 0,
        acc_error: SQLITE_NOMEM as u8,
        printf_flags: 0,
    })
}

/// Devolve qualquer código de erro associado a p.
pub fn str_errcode(p: Option<&StrAccum>) -> i32 {
    match p {
        Some(p) => p.acc_error as i32,
        None => SQLITE_NOMEM,
    }
}

/// Devolve o comprimento atual de p em bytes.
pub fn str_length(p: Option<&StrAccum>) -> i32 {
    match p {
        Some(p) => p.n_char as i32,
        None => 0,
    }
}

/// Devolve o valor atual de p.
pub fn str_value(p: Option<&StrAccum>) -> Option<Vec<u8>> {
    match p {
        Some(p) if p.n_char != 0 => Some(p.z_text.clone()),
        _ => None,
    }
}

/// Zera uma string StrAccum. Recupera toda a memória alocada.
pub fn str_reset(p: &mut StrAccum) {
    if is_malloced(p) {
        p.printf_flags &= !SQLITE_PRINTF_MALLOCED;
    }
    p.z_text = Vec::new();
    p.n_alloc = 0;
    p.n_char = 0;
}

/// Inicializa um acumulador de string.
///
/// p:      O acumulador a inicializar.
/// db:     A conexão com o banco de dados. Pode ser None. Se não for None, o
///         lookaside é usado e db->mallocFailed é definido conforme o caso.
/// z_base: Um buffer inicial. No C pode ser NULL, e então o buffer inicial é
///         alocado. Aqui só o tamanho `n` importa: o texto é guardado em `z_text`.
/// n:      Tamanho de z_base em bytes. Se as necessidades totais de espaço nunca
///         passarem de n, nenhuma alocação acontece.
/// mx:     Máximo de bytes a acumular. Se mx==0, nenhuma alocação acontece.
pub fn str_accum_init(p: &mut StrAccum, db: Option<Sqlite3Ref>, n: i32, mx: i32) {
    p.z_text = Vec::new();
    p.db = db;
    p.n_alloc = n as u32;
    p.mx_alloc = mx as u32;
    p.n_char = 0;
    p.acc_error = 0;
    p.printf_flags = 0;
}

/// Aloca e inicializa um novo objeto de string dinâmica.
pub fn str_new(db: Option<Sqlite3Ref>) -> Box<StrAccum> {
    let mx = match &db {
        Some(db_ref) => db_ref.borrow().a_limit[SQLITE_LIMIT_LENGTH as usize],
        None => SQLITE_MAX_LENGTH,
    };
    let mut p = Box::new(StrAccum {
        db: None,
        z_text: Vec::new(),
        n_alloc: 0,
        mx_alloc: 0,
        n_char: 0,
        acc_error: 0,
        printf_flags: 0,
    });
    str_accum_init(&mut p, None, 0, mx);
    p
}

/// Imprime em memória obtida de sqliteMalloc(). Usa as extensões internas das
/// conversões %.
pub fn v_m_printf(db: &Sqlite3Ref, z_format: &[u8], ap: &mut VaList<'_>) -> Option<Vec<u8>> {
    let mut acc = StrAccum {
        db: None,
        z_text: Vec::new(),
        n_alloc: 0,
        mx_alloc: 0,
        n_char: 0,
        acc_error: 0,
        printf_flags: 0,
    };
    let mx = db.borrow().a_limit[SQLITE_LIMIT_LENGTH as usize];
    str_accum_init(&mut acc, Some(db.clone()), SQLITE_PRINT_BUF_SIZE, mx);
    acc.printf_flags = SQLITE_PRINTF_INTERNAL;
    str_vappendf(&mut acc, z_format, ap);
    let acc_error = acc.acc_error;
    let z = str_accum_finish(&mut acc);
    if acc_error as i32 == SQLITE_NOMEM {
        oom_fault(&mut db.borrow_mut());
    }
    z
}

/// Imprime em memória obtida de sqliteMalloc(). Usa as extensões internas das
/// conversões %. (A variante com `...` do C é `v_m_printf` com a VaList montada
/// pelo chamador.)
pub fn m_printf(db: &Sqlite3Ref, z_format: &[u8], ap: &mut VaList<'_>) -> Option<Vec<u8>> {
    v_m_printf(db, z_format, ap)
}

/// Imprime em memória obtida de sqlite3_malloc(). Omite as extensões internas
/// das conversões %.
pub fn vmprintf(z_format: &[u8], ap: &mut VaList<'_>) -> Option<Vec<u8>> {
    // SQLITE_ENABLE_API_ARMOR não está ligado
    if api::initialize() != 0 {
        return None;
    }
    let mut acc = StrAccum {
        db: None,
        z_text: Vec::new(),
        n_alloc: 0,
        mx_alloc: 0,
        n_char: 0,
        acc_error: 0,
        printf_flags: 0,
    };
    str_accum_init(&mut acc, None, SQLITE_PRINT_BUF_SIZE, SQLITE_MAX_LENGTH);
    str_vappendf(&mut acc, z_format, ap);
    str_accum_finish(&mut acc)
}

/// Imprime em memória obtida de sqlite3_malloc(). Omite as extensões internas
/// das conversões %. (A variante com `...` do C é `vmprintf` com a VaList montada
/// pelo chamador.)
pub fn mprintf(z_format: &[u8], ap: &mut VaList<'_>) -> Option<Vec<u8>> {
    if api::initialize() != 0 {
        return None;
    }
    vmprintf(z_format, ap)
}


// ---- part_003.rs ----

// Tradução de printf.c (trecho 003): snprintf, log e strings com contagem de
// referências.

/// sqlite3_snprintf() funciona como snprintf() exceto que ignora as configurações
/// de localidade atuais. Isso é importante para o SQLite porque não podemos usar
/// "," no lugar de "." como ponto decimal, como algumas localidades especificam.
///
/// Ops: os dois primeiros argumentos de sqlite3_snprintf() estão invertidos em
/// relação ao snprintf() padrão. Infelizmente é tarde demais para mudar isso sem
/// quebrar a compatibilidade, então temos que conviver com o erro.
///
/// sqlite3_vsnprintf() é a versão com varargs. `z_buf` precisa ter pelo menos `n`
/// bytes; o resultado é gravado nele com o NUL final, como no C.
pub fn vsnprintf(n: i32, z_buf: &mut [u8], z_format: &[u8], ap: &mut VaList<'_>) {
    if n <= 0 {
        return;
    }
    // SQLITE_ENABLE_API_ARMOR não está ligado
    let mut acc = StrAccum {
        db: None,
        z_text: Vec::new(),
        n_alloc: 0,
        mx_alloc: 0,
        n_char: 0,
        acc_error: 0,
        printf_flags: 0,
    };
    str_accum_init(&mut acc, None, n, 0);
    str_vappendf(&mut acc, z_format, ap);
    let n_char = acc.n_char as usize;
    z_buf[..n_char].copy_from_slice(&acc.z_text[..n_char]);
    z_buf[n_char] = 0;
}

/// Versão com `...` do `vsnprintf`: o chamador monta a VaList.
pub fn snprintf(n: i32, z_buf: &mut [u8], z_format: &[u8], ap: &mut VaList<'_>) {
    vsnprintf(n, z_buf, z_format, ap);
}

/// Esta é a rotina que de fato formata a mensagem de sqlite3_log(). Ela fica numa
/// rotina separada de sqlite3_log() para não gastar pilha em sistemas de pouca
/// pilha quando o log está desligado.
///
/// sqlite3_log() precisa renderizar num buffer estático. Não pode alocar memória
/// dinamicamente porque pode ser chamada com o mutex do alocador de memória preso.
///
/// sqlite3_str_vappendf() pode pedir alocações *temporárias* para certos
/// caracteres de formato (%q) ou para precisões e larguras muito grandes. É preciso
/// cuidar para que as chamadas a sqlite3_log() feitas com o mutex de memória preso
/// não usem esses mecanismos.
fn render_log_msg(i_err_code: i32, z_format: &[u8], ap: &mut VaList<'_>) {
    let mut acc = StrAccum {
        db: None,
        z_text: Vec::new(),
        n_alloc: 0,
        mx_alloc: 0,
        n_char: 0,
        acc_error: 0,
        printf_flags: 0,
    }; // Acumulador de string. A mensagem completa tem SQLITE_PRINT_BUF_SIZE*3 bytes

    str_accum_init(&mut acc, None, SQLITE_PRINT_BUF_SIZE * 3, 0);
    str_vappendf(&mut acc, z_format, ap);
    let msg = str_accum_finish(&mut acc).unwrap_or_default();
    if let Some(x_log) = global_config_x_log() {
        x_log(i_err_code, &msg);
    }
}

/// Formata e escreve uma mensagem no log se o log está ligado.
pub fn log(i_err_code: i32, z_format: &[u8], ap: &mut VaList<'_>) {
    if global_config_x_log().is_some() {
        render_log_msg(i_err_code, z_format, ap);
    }
}

// sqlite3DebugPrintf() só existe com SQLITE_DEBUG ou SQLITE_HAVE_OS_TRACE, e a
// build do Debian não define nenhum dos dois: a função não existe aqui.

/// Envoltório com argumentos variáveis de str_vappendf(). O bit
/// SQLITE_PRINTF_INTERNAL em printf_flags habilita os formatos internos.
pub fn str_appendf(p: &mut StrAccum, z_format: &[u8], ap: &mut VaList<'_>) {
    str_vappendf(p, z_format, ap);
}

// Armazenamento de string/blob com contagem de referências
//
// No C, a RCStr é um cabeçalho `RCStr { nRCRef }` colado antes dos bytes, e o
// ponteiro circula como um `char*` comum. Aqui a contagem é a do próprio `Rc`
// (`Rc::strong_count`), e os bytes vivem no `Vec<u8>` de dentro. A struct `RCStr`
// de sqliteInt.h fica sem uso.

/// Uma string com contagem de referências (RCStr). Guarda `N + 1` bytes, o último
/// reservado ao NUL do C.
pub type RcStrRef = std::rc::Rc<std::cell::RefCell<Vec<u8>>>;

/// Aumenta em um a contagem de referências da string.
///
/// O parâmetro de entrada é devolvido (como nova referência).
pub fn rc_str_ref(z: &RcStrRef) -> RcStrRef {
    std::rc::Rc::clone(z)
}

/// Diminui em um a contagem de referências. Libera a string quando a contagem
/// chega a zero.
pub fn rc_str_unref(z: RcStrRef) {
    debug_assert!(std::rc::Rc::strong_count(&z) > 0);
    drop(z);
}

/// Cria uma nova string capaz de guardar N bytes de texto, sem contar o byte zero
/// do final. A string não é inicializada (aqui vem zerada).
///
/// A contagem de referências começa em 1. Chame `rc_str_unref()` para liberar a
/// string recém alocada.
///
/// Esta rotina devolve None em caso de OOM.
pub fn rc_str_new(n: u64) -> Option<RcStrRef> {
    let total = (n as usize).checked_add(1)?;
    let mut v: Vec<u8> = Vec::new();
    if v.try_reserve_exact(total).is_err() {
        return None;
    }
    v.resize(total, 0);
    Some(std::rc::Rc::new(std::cell::RefCell::new(v)))
}

/// Muda o tamanho da string para que ela consiga guardar N bytes. A string pode
/// ser realocada, então devolve a nova alocação.
pub fn rc_str_resize(z: RcStrRef, n: u64) -> Option<RcStrRef> {
    debug_assert!(std::rc::Rc::strong_count(&z) == 1);
    let total = match (n as usize).checked_add(1) {
        Some(t) => t,
        None => return None,
    };
    {
        let mut v = z.borrow_mut();
        let extra = total.saturating_sub(v.len());
        if v.try_reserve_exact(extra).is_err() {
            return None; // `z` é liberada ao sair, como o sqlite3_free(p) do C
        }
        v.resize(total, 0);
    }
    Some(z)
}

