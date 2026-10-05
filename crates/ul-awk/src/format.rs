//! `printf`/`sprintf` do gawk 5.2.1 e a conversão de número pra texto (`CONVFMT`/`OFMT`).
//!
//! Contrato com o interpretador (não mude as assinaturas sem combinar com o integrador):
//!
//! - [`FmtArg`] é como o formatador enxerga um argumento: o interpretador implementa a conversão de
//!   valor pra número e pra texto (com `CONVFMT`), o formatador só decide qual das duas usar.
//! - [`format`] devolve os bytes formatados, ou [`FormatError`] com o texto que vem depois de
//!   `fatal: ` na mensagem do gawk (o interpretador acrescenta o prefixo `awk: cmd. line:N: fatal: `).
//! - Avisos (não fatais) vão pra `warnings`, sem prefixo; o interpretador prefixa `awk: cmd. line:N: warning: `.
//!   O gawk 5.2.1 sem `--lint` não emite aviso nenhum ao formatar (todos os avisos de `format_tree`
//!   são de lint), então o vetor só é repassado.
//! - [`num_to_str`] é a regra do gawk pra número virar texto: valor inteiro sai como inteiro exato,
//!   o resto passa pelo formato (`CONVFMT` ou `OFMT`, quem chama escolhe).
//! - Extra ao contrato: [`try_num_to_str`] é a mesma conversão, mas devolve o erro fatal que o gawk
//!   daria com um `CONVFMT`/`OFMT` que pede mais argumentos do que existem (`"%d %d"`, `"%2$d"`).
//!
//! Comportamento reproduzido (medido como caixa-preta contra o gawk 5.2.1 em `LC_ALL=C.UTF-8`):
//!
//! - Uma especificação inválida (caractere de conversão desconhecido, flag depois do ponto, segundo
//!   ponto, segundo `l`, dígito depois da precisão...) é copiada literalmente, sem consumir argumento
//!   (os argumentos já consumidos por um `*` dessa especificação continuam consumidos), e a varredura
//!   continua depois do caractere que a invalidou. Um `%` no fim, ou uma especificação cortada pelo
//!   fim do formato, também sai literal. O byte 0xFF é a exceção: vale como flag sem efeito.
//! - Flags valem em qualquer posição antes do ponto, inclusive depois da largura (`%5-d`, `%5+d`); um
//!   `0` depois de outra flag (sem `-`) recomeça a largura (`%5+0d` tem largura 0); com `-` ativo o
//!   `0` é ignorado em qualquer posição. Um `-` logo depois do ponto põe a precisão em -1: dígitos
//!   depois dele a descartam; sem dígitos ela continua ligada e negativa (o `printf` do C a trata
//!   como omitida, o `%s` como sem limite, o `%d` deixa de preencher com zeros). `l`, `L`, `h`, `j`,
//!   `z`, `t` e o `P` (modo POSIX de NaN e infinito) valem uma vez cada.
//! - Largura e precisão por `*` truncam o número como o `cvttsd2si` do x86-64 (fora do intervalo vira
//!   `i64::MIN`); largura negativa alinha à esquerda; precisão negativa vale como omitida. Nas
//!   conversões de ponto flutuante a largura e a precisão vão pro `printf` do C como `int`.
//! - NaN e infinito saem `+nan`, `-nan`, `+inf`, `-inf` (maiúsculas nas conversões maiúsculas),
//!   sem largura nem flags, em todas as conversões numéricas; com `P` vão pro `printf` do C.
//! - `%d`/`%i` truncam em direção a zero e imprimem todos os dígitos exatos do `double`.
//!   `%o %u %x %X` aceitam valores que cabem em `intmax_t`/`uintmax_t` (negativo vira complemento de
//!   dois); fora disso caem para `%g` com as mesmas flags, largura e precisão.
//! - `%c` numérico usa o código truncado como `wchar_t` de 32 bits; código que o `wcrtomb` do glibc
//!   rejeita (negativo, substituto UTF-16) sai como o byte baixo; a largura conta bytes. `%c` de texto
//!   pega o primeiro caractere multibyte e a largura conta caracteres.
//! - `%s` conta caracteres como o gawk: se o texto começa com um caractere válido, conta até o
//!   primeiro byte inválido, sequência incompleta ou NUL (e só copia até ali quando há largura ou
//!   precisão); se começa com byte inválido ou NUL, conta bytes.
//! - O ponto flutuante segue o glibc byte a byte, inclusive o `%#g` que perde os zeros quando o
//!   arredondamento sobe o expoente até a precisão (`%#g` de 999999.5 dá `1.e+06`).
//! - Largura `LONG_MIN` (de `*` com NaN, infinito ou valor fora de `long`) ou negativa por estouro de
//!   dígitos faz o gawk pedir um buffer de tamanho absurdo no caminho de ponto flutuante e morrer com
//!   `cannot reallocate`; o tamanho da mensagem depende do buffer acumulado, que é emulado.
//!
//! Divergências conhecidas (todas dependem de memória da máquina ou de falha do próprio gawk):
//!
//! - Saídas acima de 1 TiB devolvem o erro fatal de realocação em vez de tentar alocar; o gawk falha
//!   no limite de memória da máquina dele, com outro tamanho na mensagem.
//! - `CONVFMT`/`OFMT` com `%s`: o gawk morre por recursão infinita (ver [`try_num_to_str`]).
//! - Largura ou precisão de ponto flutuante que vira `INT_MIN` ao truncar para `int` (2^31, por
//!   exemplo): o gawk não termina (medido com 20 s de limite); aqui vale o que o `printf` do C
//!   definiria para o valor truncado.

use std::borrow::Cow;

/// Um argumento do `printf`, visto como número ou como texto.
pub trait FmtArg {
    /// Valor numérico (conversão do awk: prefixo numérico do texto, `strtod`).
    fn to_num(&self) -> f64;
    /// Valor textual (números convertidos com `CONVFMT`, inteiros como inteiros).
    fn to_str(&self) -> Cow<'_, [u8]>;
    /// Verdadeiro se o valor é número (ou texto de entrada com cara de número, o "strnum").
    /// O `%c` usa isso pra decidir entre código de caractere e primeiro caractere.
    fn is_numeric(&self) -> bool;
}

/// Erro fatal de formatação: o texto que o gawk escreve depois de `fatal: `.
///
/// Quando a mensagem inclui o próprio formato (argumentos insuficientes), o formato sai até o
/// primeiro NUL (o gawk imprime com `%s` do C) e bytes que não são UTF-8 viram U+FFFD, porque a
/// mensagem é `String`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatError(pub String);

/// Formata `fmt` com `args` como o `sprintf` do gawk 5.2.1 em `LC_ALL=C.UTF-8`.
pub fn format(fmt: &[u8], args: &[&dyn FmtArg], warnings: &mut Vec<String>) -> Result<Vec<u8>, FormatError> {
    format_mode(fmt, args, warnings, false)
}

/// Como [`format`], com o modo mawk opcional: `%c` numérico vira um byte só (valor módulo 256) e
/// `%d`/`%i` com valor fora do intervalo de `i64` caem para `%g`.
pub fn format_mode(fmt: &[u8], args: &[&dyn FmtArg], warnings: &mut Vec<String>, mawk: bool) -> Result<Vec<u8>, FormatError> {
    // Sem `--lint` o gawk não avisa nada ao formatar; o parâmetro fica para o contrato.
    let _ = warnings;
    let mut out = Vec::with_capacity(fmt.len() + 16);
    Formatter { fmt, args, cur_arg: 0, used_dollar: false, osiz: OBUF_INITIAL, mawk }.run(&mut out)?;
    Ok(out)
}

/// Número pra texto pela regra do gawk: inteiro exato quando o valor é inteiro, senão `sprintf(convfmt, x)`.
/// NaN e infinito saem como o gawk escreve (`-nan`, `+nan`, `-inf`, `+inf`).
///
/// Se o formato pede argumentos que não existem (o gawk morre com erro fatal), devolve a conversão
/// com o `CONVFMT` padrão `%.6g`; quem precisa reproduzir o erro usa [`try_num_to_str`].
pub fn num_to_str(x: f64, convfmt: &[u8]) -> Vec<u8> {
    match try_num_to_str(x, convfmt) {
        Ok(v) => v,
        Err(_) => try_num_to_str(x, b"%.6g").unwrap_or_default(),
    }
}

/// Igual a [`num_to_str`], mas devolve o erro fatal do gawk quando o formato não pode ser
/// satisfeito com um único argumento (`"%d %d"`, `"%*d"`, `"%2$d"`).
///
/// Divergência conhecida: com uma conversão `%s` no formato (`CONVFMT="%s"`) o gawk entra em
/// recursão infinita e morre com `internal error: segfault`; aqui o `%s` recebe o número formatado
/// com `%.6g`.
pub fn try_num_to_str(x: f64, convfmt: &[u8]) -> Result<Vec<u8>, FormatError> {
    if !x.is_finite() {
        return Ok(nan_inf_text(x, b'g').as_bytes().to_vec());
    }
    if x.trunc() == x {
        // Caminho quente (subscrito de array): inteiro no intervalo de `i64` vira texto sem alocação extra.
        if x.abs() < 9.0e18 {
            let mut buf = [0u8; 20];
            let mut v = Vec::with_capacity(20);
            let n = x as i64;
            if n < 0 {
                v.push(b'-');
            }
            v.extend_from_slice(u64_digits(n.unsigned_abs(), &mut buf));
            return Ok(v);
        }
        let mut v = Vec::new();
        if x < 0.0 {
            v.push(b'-');
        }
        push_int_digits(&mut v, x.abs());
        return Ok(v);
    }
    let arg = ConvNum(x);
    format(convfmt, &[&arg], &mut Vec::new())
}

/// O número que `try_num_to_str` passa para o formato.
struct ConvNum(f64);

impl FmtArg for ConvNum {
    fn to_num(&self) -> f64 {
        self.0
    }
    fn to_str(&self) -> Cow<'_, [u8]> {
        // O gawk entraria em recursão infinita aqui (ver `try_num_to_str`); o `%.6g` padrão é a
        // saída sensata.
        let mut out = Vec::new();
        c_float(&mut out, b'g', self.0, CFlags::default(), 0, 6);
        Cow::Owned(out)
    }
    fn is_numeric(&self) -> bool {
        true
    }
}

/// O que um dígito ou um `*` está preenchendo na especificação corrente.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Width,
    Prec,
    /// Precisão já lida: dígito, `*`, `.`, `$` ou flag a partir daqui invalidam a especificação.
    Done,
}

/// Estado de uma especificação `%...` em análise.
struct Spec {
    fw: i64,
    prec: i64,
    have_prec: bool,
    target: Target,
    /// Índice (base 1) do `N$`, ou 0 se a especificação não é posicional.
    argnum: i64,
    lj: bool,
    sign: u8,
    alt: bool,
    zero: bool,
    magic_posix: bool,
    mod_l: bool,
    mod_ll: bool,
    mod_h: bool,
    mod_j: bool,
    mod_z: bool,
    mod_t: bool,
    /// Modo mawk, copiado do formatador.
    mawk: bool,
}

impl Spec {
    fn new() -> Spec {
        Spec {
            fw: 0,
            prec: 0,
            have_prec: false,
            target: Target::Width,
            argnum: 0,
            lj: false,
            sign: 0,
            alt: false,
            zero: false,
            magic_posix: false,
            mod_l: false,
            mod_ll: false,
            mod_h: false,
            mod_j: false,
            mod_z: false,
            mod_t: false,
            mawk: false,
        }
    }

    /// Flags do `printf` do C para o caminho de ponto flutuante.
    fn cflags(&self) -> CFlags {
        CFlags { lj: self.lj, sign: self.sign, alt: self.alt, zero: self.zero }
    }

    /// Largura como `int` do C (o gawk passa `(int) fw`).
    fn c_width(&self) -> i32 {
        self.fw as i32
    }

    /// Precisão como `int` do C; negativa quando omitida.
    fn c_prec(&self) -> i32 {
        if self.have_prec { self.prec as i32 } else { -1 }
    }
}

/// Resultado de um caractere da especificação.
enum Step {
    /// Continua lendo a especificação.
    More,
    /// Especificação inválida: sai literal, a varredura segue depois do caractere atual.
    Literal,
    /// Conversão feita.
    Done,
}

struct Formatter<'a, 'b> {
    fmt: &'a [u8],
    args: &'a [&'b dyn FmtArg],
    /// Próximo argumento sequencial (base 0).
    cur_arg: usize,
    /// Já apareceu um `N$` neste formato.
    used_dollar: bool,
    /// Tamanho emulado do buffer de saída do gawk (`osiz`), para reproduzir o erro fatal de
    /// realocação do caminho de ponto flutuante (ver [`Formatter::float_out`]).
    osiz: u64,
    /// Modo mawk (ver [`format_mode`]).
    mawk: bool,
}

const MUST_USE_COUNT: &str = "must use `count$' on all formats or none";

/// Tamanho inicial do buffer de saída do `format_tree`.
const OBUF_INITIAL: u64 = 64;

/// Acima disto nenhuma realocação dá certo (é mais memória do que qualquer máquina que roda o
/// oráculo tem); o gawk morre com `cannot reallocate`. Abaixo disto, se a máquina tem ou não a
/// memória é questão do host: aqui a saída é produzida.
const ALLOC_LIMIT: u64 = 1 << 40;

/// Mensagem do `erealloc` do gawk quando o buffer de saída não cresce. `line` é a linha do
/// `builtin.c` do gawk 5.2.1 onde a macro de crescimento foi expandida.
fn obuf_error(line: u32, size: u64) -> FormatError {
    FormatError(format!(
        "builtin.c:{line}:format_tree: obuf: cannot reallocate {} bytes of memory: Cannot allocate memory",
        size as i64
    ))
}

/// Linhas do `builtin.c` do gawk 5.2.1 com os crescimentos de buffer que podem falhar.
const LINE_PAD_LEFT: u32 = 1512;
const LINE_PAD_RIGHT: u32 = 1530;
const LINE_FLOAT_CHKSIZE: u32 = 1607;

impl Formatter<'_, '_> {
    fn run(&mut self, out: &mut Vec<u8>) -> Result<(), FormatError> {
        let fmt = self.fmt;
        let n = fmt.len();
        // `s0` marca o início do texto ainda não copiado (literal ou especificação inválida).
        let mut s0 = 0usize;
        let mut i = 0usize;
        'scan: while i < n {
            match fmt[i..].iter().position(|&b| b == b'%') {
                Some(k) => i += k,
                None => break,
            }
            out.extend_from_slice(&fmt[s0..i]);
            s0 = i;
            i += 1;
            let mut sp = Spec::new();
            sp.mawk = self.mawk;
            loop {
                let Some(&c) = fmt.get(i) else { break 'scan };
                i += 1;
                match self.step(out, &mut sp, c, &mut i)? {
                    Step::More => {}
                    Step::Literal => continue 'scan,
                    Step::Done => {
                        s0 = i;
                        continue 'scan;
                    }
                }
            }
        }
        out.extend_from_slice(&fmt[s0..]);
        Ok(())
    }

    /// Trata o caractere `c` da especificação; `i` aponta para o caractere seguinte.
    fn step(&mut self, out: &mut Vec<u8>, sp: &mut Spec, c: u8, i: &mut usize) -> Result<Step, FormatError> {
        let fmt = self.fmt;
        match c {
            b'%' => {
                // Largura e precisão são ignoradas (o gawk só reclama com `--lint`).
                out.push(b'%');
                return Ok(Step::Done);
            }
            b'0' if sp.lj => {
                if sp.target == Target::Width {
                    sp.zero = true;
                }
            }
            b'0'..=b'9' => {
                if c == b'0' && sp.target == Target::Width {
                    sp.zero = true;
                }
                if sp.target == Target::Done {
                    return Ok(Step::Literal);
                }
                // Depois de `.-` o primeiro dígito não é atribuído e os seguintes continuam a conta
                // sobre o -1 (a precisão fica negativa, salvo estouro), e aí é descartada.
                let negative_prec = sp.target == Target::Prec && sp.prec < 0;
                let mut v = if negative_prec { sp.prec } else { i64::from(c - b'0') };
                while let Some(&d) = fmt.get(*i)
                    && d.is_ascii_digit()
                {
                    v = v.wrapping_mul(10).wrapping_add(i64::from(d - b'0'));
                    *i += 1;
                }
                if sp.target == Target::Width {
                    sp.fw = v;
                } else {
                    sp.prec = v;
                    if v < 0 {
                        sp.have_prec = false;
                    }
                    sp.target = Target::Done;
                }
            }
            b'.' => {
                if sp.target != Target::Width {
                    return Ok(Step::Literal);
                }
                sp.target = Target::Prec;
                sp.have_prec = true;
            }
            b'*' => {
                if sp.target == Target::Done {
                    return Ok(Step::Literal);
                }
                let next_is_digit = fmt.get(*i).is_some_and(u8::is_ascii_digit);
                let val = if next_is_digit {
                    // `*N$`: o índice é `int` no gawk (estoura com volta).
                    let mut v: i32 = 0;
                    while let Some(&d) = fmt.get(*i)
                        && d.is_ascii_digit()
                    {
                        v = v.wrapping_mul(10).wrapping_add(i32::from(d - b'0'));
                        *i += 1;
                    }
                    if fmt.get(*i) != Some(&b'$') {
                        return Err(FormatError("no `$' supplied for positional field width or precision".into()));
                    }
                    *i += 1;
                    if v < 0 || v as usize > self.args.len() {
                        return Err(self.too_few(*i));
                    }
                    if v == 0 {
                        // O argumento 0 do gawk é a própria string de formato.
                        awk_str_to_num(fmt)
                    } else {
                        self.args[v as usize - 1].to_num()
                    }
                } else if self.used_dollar {
                    // Esta mensagem do gawk já traz o `fatal: ` dentro do texto.
                    return Err(FormatError(format!("fatal: {MUST_USE_COUNT}")));
                } else {
                    self.next_arg(sp, *i)?.to_num()
                };
                let v = cvt_i64(val);
                if sp.target == Target::Width {
                    if v < 0 {
                        sp.fw = v.wrapping_neg();
                        sp.lj = true;
                    } else {
                        sp.fw = v;
                    }
                } else {
                    sp.prec = v;
                    sp.have_prec = v >= 0;
                    sp.target = Target::Done;
                }
            }
            b'$' => {
                if sp.target != Target::Width {
                    return Err(FormatError("`$' not permitted after period in format".into()));
                }
                let argnum = sp.fw;
                sp.fw = 0;
                self.used_dollar = true;
                if argnum <= 0 {
                    return Err(FormatError("argument index with `$' must be > 0".into()));
                }
                if argnum as u64 > self.args.len() as u64 {
                    return Err(FormatError(format!(
                        "argument index {argnum} greater than total number of supplied arguments"
                    )));
                }
                sp.argnum = argnum;
            }
            b'-' => {
                if sp.prec < 0 {
                    return Ok(Step::Literal);
                }
                if sp.target == Target::Prec {
                    // `.-` sem dígitos depois deixa `have_prec` ligado com precisão -1: o `printf`
                    // do C a trata como omitida e o `%s` compara como `size_t` (sem limite), mas o
                    // `%d` perde o preenchimento com zeros.
                    sp.prec = -1;
                    return Ok(Step::More);
                }
                sp.lj = true;
                return Ok(check_pos(sp));
            }
            b' ' => {
                if sp.sign == 0 {
                    sp.sign = b' ';
                }
                return Ok(check_pos(sp));
            }
            b'+' => {
                sp.sign = b'+';
                return Ok(check_pos(sp));
            }
            b'#' => {
                sp.alt = true;
                return Ok(check_pos(sp));
            }
            b'\'' => {
                // Em C.UTF-8 não há separador de milhar: a flag não muda a saída.
                return Ok(check_pos(sp));
            }
            0xff => {
                // O byte 0xFF (o -1 do `char` com sinal) o gawk aceita como uma flag sem efeito:
                // ignorado antes do ponto, invalida a especificação depois dele (medido).
                return Ok(check_pos(sp));
            }
            b'l' => return Ok(set_once(&mut sp.mod_l)),
            b'L' => return Ok(set_once(&mut sp.mod_ll)),
            b'h' => return Ok(set_once(&mut sp.mod_h)),
            b'j' => return Ok(set_once(&mut sp.mod_j)),
            b'z' => return Ok(set_once(&mut sp.mod_z)),
            b't' => return Ok(set_once(&mut sp.mod_t)),
            b'P' => return Ok(set_once(&mut sp.magic_posix)),
            b'c' => {
                let arg = self.next_arg(sp, *i)?;
                conv_char(out, sp, arg)?;
                return Ok(Step::Done);
            }
            b's' => {
                let arg = self.next_arg(sp, *i)?;
                conv_string(out, sp, arg)?;
                return Ok(Step::Done);
            }
            b'd' | b'i' => {
                let x = self.next_arg(sp, *i)?.to_num();
                if let Some(conv) = conv_signed(out, sp, x, c)? {
                    self.float_out(out, sp, conv, x)?;
                }
                return Ok(Step::Done);
            }
            b'o' | b'u' | b'x' | b'X' => {
                let x = self.next_arg(sp, *i)?.to_num();
                if let Some(conv) = conv_unsigned(out, sp, x, c)? {
                    self.float_out(out, sp, conv, x)?;
                }
                return Ok(Step::Done);
            }
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' => {
                let x = self.next_arg(sp, *i)?.to_num();
                if !x.is_finite() && !sp.magic_posix {
                    out.extend_from_slice(nan_inf_text(x, c).as_bytes());
                } else {
                    self.float_out(out, sp, c, x)?;
                }
                return Ok(Step::Done);
            }
            _ => return Ok(Step::Literal),
        }
        Ok(Step::More)
    }

    /// Caminho de ponto flutuante do gawk (`fmt1`): `chksize(fw + prec + 11)` no buffer de saída e
    /// depois o `snprintf` do C. O `chksize` com largura negativa (`*` com NaN, infinito ou valor
    /// fora de `long`, ou dígitos que estouram) pede um tamanho absurdo e o gawk morre; a conta do
    /// tamanho pedido depende do tamanho corrente do buffer, por isso ele é emulado.
    fn float_out(&mut self, out: &mut Vec<u8>, sp: &Spec, conv: u8, x: f64) -> Result<(), FormatError> {
        let used = out.len() as u64;
        // O buffer dobra sempre que o texto já escrito não cabe (`ofre = osiz - 1 - used`).
        while used >= self.osiz {
            self.osiz *= 2;
        }
        let mut ofre = self.osiz - 1 - used;
        let prec = if sp.have_prec { sp.prec } else { 6 };
        let need = sp.fw.wrapping_add(prec).wrapping_add(11);
        // `long` comparado com `size_t`: negativo vira enorme.
        if need as u64 >= ofre {
            let delta = self.osiz.wrapping_add(need as u64).wrapping_sub(ofre);
            let size = self.osiz.wrapping_add(delta);
            if size > ALLOC_LIMIT {
                return Err(obuf_error(LINE_FLOAT_CHKSIZE, size));
            }
            self.osiz = size;
            ofre = ofre.wrapping_add(delta);
        }
        let start = out.len();
        c_float(out, conv, x, sp.cflags(), sp.c_width(), sp.c_prec());
        let nc = (out.len() - start) as u64;
        if nc >= ofre {
            // O `snprintf` não coube: o gawk cresce o buffer exatamente para o que ele pediu.
            self.osiz = self.osiz.wrapping_add(self.osiz.wrapping_add(nc).wrapping_sub(ofre));
        }
        Ok(())
    }

    /// O próximo argumento da conversão (o `parse_next_arg` do gawk); `i` é a posição depois do
    /// caractere corrente, usada no circunflexo da mensagem de argumentos insuficientes.
    fn next_arg(&mut self, sp: &Spec, i: usize) -> Result<&dyn FmtArg, FormatError> {
        if sp.argnum > 0 {
            if self.cur_arg > 0 {
                return Err(FormatError(MUST_USE_COUNT.into()));
            }
            return Ok(self.args[sp.argnum as usize - 1]);
        }
        if self.used_dollar {
            return Err(FormatError(MUST_USE_COUNT.into()));
        }
        if self.cur_arg >= self.args.len() {
            return Err(self.too_few(i));
        }
        self.cur_arg += 1;
        Ok(self.args[self.cur_arg - 1])
    }

    /// Mensagem de argumentos insuficientes, com o circunflexo sob o caractere antes de `i`.
    fn too_few(&self, i: usize) -> FormatError {
        let shown = match self.fmt.iter().position(|&b| b == 0) {
            Some(k) => &self.fmt[..k],
            None => self.fmt,
        };
        FormatError(format!(
            "not enough arguments to satisfy format string\n\t`{}'\n\t{}^ ran out for this one",
            String::from_utf8_lossy(shown),
            " ".repeat(i - 1)
        ))
    }
}

/// Flag depois do ponto invalida a especificação.
fn check_pos(sp: &Spec) -> Step {
    if sp.target == Target::Width { Step::More } else { Step::Literal }
}

/// Modificador que só pode aparecer uma vez.
fn set_once(flag: &mut bool) -> Step {
    if *flag {
        Step::Literal
    } else {
        *flag = true;
        Step::More
    }
}

/// Acrescenta `n` cópias de `b`, se `n` for positivo. `line` identifica o crescimento do buffer do
/// gawk que falharia se o total passasse de [`ALLOC_LIMIT`] (o tamanho na mensagem é o da próxima
/// dobra do buffer; o valor exato do gawk depende da memória da máquina).
fn pad(out: &mut Vec<u8>, b: u8, n: i64, line: u32) -> Result<(), FormatError> {
    if n > 0 {
        let need = (out.len() as u64).saturating_add(n as u64);
        if need >= ALLOC_LIMIT {
            let mut size = OBUF_INITIAL;
            while size <= need {
                size = size.saturating_mul(2);
            }
            return Err(obuf_error(line, size));
        }
        out.resize(need as usize, b);
    }
    Ok(())
}

/// O `pr_tail` do gawk: alinha `body` (que vale `units` unidades de largura) em `fw` com `fill`.
fn pr_tail(out: &mut Vec<u8>, fill: u8, lj: bool, fw: i64, units: i64, body: &[u8]) -> Result<(), FormatError> {
    if !lj {
        pad(out, fill, fw.saturating_sub(units), LINE_PAD_LEFT)?;
    }
    out.extend_from_slice(body);
    if lj {
        pad(out, fill, fw.saturating_sub(units), LINE_PAD_RIGHT)?;
    }
    Ok(())
}

/// `%c`.
fn conv_char(out: &mut Vec<u8>, sp: &Spec, arg: &dyn FmtArg) -> Result<(), FormatError> {
    if sp.mawk {
        // mawk: o valor numérico vira `int` e depois um byte só (módulo 256); texto dá o primeiro byte.
        let byte: [u8; 1] = if arg.is_numeric() {
            [cvt_i64(arg.to_num()) as u8]
        } else {
            let s = arg.to_str();
            [s.first().copied().unwrap_or(0)]
        };
        return pr_tail(out, b' ', sp.lj, sp.fw, 1, &byte);
    }
    if arg.is_numeric() {
        // `get_number_uj` seguido da conversão para `wchar_t` (32 bits com sinal).
        let uval = cvt_u64(arg.to_num());
        let wc = uval as u32;
        let mut buf = [0u8; 6];
        let n = if wc <= 0x7fff_ffff && !(0xd800..=0xdfff).contains(&wc) {
            encode_wc(wc, &mut buf)
        } else {
            // O `wcrtomb` rejeita: sai o byte baixo do valor.
            buf[0] = uval as u8;
            1
        };
        // A largura conta bytes aqui (o gawk não ajusta `fw` no caso numérico).
        return pr_tail(out, b' ', sp.lj, sp.fw, n as i64, &buf[..n]);
    }
    let s = arg.to_str();
    let mut fw = sp.fw;
    let first: &[u8] = match mb_len(&s) {
        Some(n) if n > 0 => {
            if fw > 0 {
                fw = fw.wrapping_add(n as i64 - 1);
            }
            &s[..n]
        }
        // Inválido, incompleto ou NUL: um byte. Texto vazio: o terminador NUL do gawk.
        _ => s.get(..1).unwrap_or(b"\0"),
    };
    pr_tail(out, b' ', sp.lj, fw, first.len() as i64, first)
}

/// `%s`.
fn conv_string(out: &mut Vec<u8>, sp: &Spec, arg: &dyn FmtArg) -> Result<(), FormatError> {
    let s = arg.to_str();
    if sp.fw == 0 && !sp.have_prec {
        out.extend_from_slice(&s);
        return Ok(());
    }
    let count = char_count(&s) as i64;
    // O gawk compara a precisão (`long`) com a contagem (`size_t`): negativa vale como sem limite.
    let units = if !sp.have_prec || sp.prec > count || sp.prec < 0 { count } else { sp.prec };
    let nbytes = byte_count(&s, units as usize);
    pr_tail(out, b' ', sp.lj, sp.fw, units, &s[..nbytes])
}

/// Campo vazio de `%d`/`%x` com precisão zero e valor zero: só o preenchimento com espaços.
fn empty_field(out: &mut Vec<u8>, sp: &Spec) -> Result<Option<u8>, FormatError> {
    let line = if sp.lj { LINE_PAD_RIGHT } else { LINE_PAD_LEFT };
    pad(out, b' ', sp.fw, line)?;
    Ok(None)
}

/// NaN e infinito em `%d %i %o %u %x %X`: o texto do gawk, ou o caminho de ponto flutuante com
/// `%g` no modo `P`.
fn int_nan_inf(out: &mut Vec<u8>, sp: &Spec, x: f64, c: u8) -> Option<u8> {
    if sp.magic_posix {
        Some(b'g')
    } else {
        out.extend_from_slice(nan_inf_text(x, c).as_bytes());
        None
    }
}

/// `%d` e `%i`. Devolve `Some(conv)` quando o valor tem de ir pro caminho de ponto flutuante.
fn conv_signed(out: &mut Vec<u8>, sp: &Spec, x: f64, c: u8) -> Result<Option<u8>, FormatError> {
    if !x.is_finite() {
        return Ok(int_nan_inf(out, sp, x, c));
    }
    let t = x.trunc();
    if sp.mawk && (t >= 9_223_372_036_854_775_808.0 || t < -9_223_372_036_854_775_808.0) {
        // mawk: fora do intervalo de inteiro de 64 bits o `%d` vira `%g`.
        return Ok(Some(b'g'));
    }
    if sp.have_prec && sp.prec == 0 && t == 0.0 {
        // "O resultado de converter zero com precisão zero é nenhum caractere."
        return empty_field(out, sp);
    }
    let mut digits = Vec::with_capacity(24);
    push_int_digits(&mut digits, t.abs());
    let sign: &[u8] = if t < 0.0 {
        b"-"
    } else if sp.sign != 0 {
        std::slice::from_ref(&sp.sign)
    } else {
        b""
    };
    emit_int(out, sp, sign, &digits)?;
    Ok(None)
}

/// `%o`, `%u`, `%x` e `%X`. Devolve `Some(conv)` quando o valor tem de ir pro caminho de ponto
/// flutuante (fora do intervalo de inteiro vira `%g`).
fn conv_unsigned(out: &mut Vec<u8>, sp: &Spec, x: f64, c: u8) -> Result<Option<u8>, FormatError> {
    if !x.is_finite() {
        return Ok(int_nan_inf(out, sp, x, c));
    }
    // O teste de zero usa o valor original, não o truncado (`%.0x` de 0.5 imprime `0`).
    if !sp.alt && sp.have_prec && sp.prec == 0 && x == 0.0 {
        return empty_field(out, sp);
    }
    let t = x.trunc();
    let uval = if x < 0.0 {
        let v = cvt_i64(x);
        if v as f64 != t {
            return Ok(Some(b'g'));
        }
        v as u64
    } else {
        let v = cvt_u64(x);
        if v as f64 != t {
            return Ok(Some(b'g'));
        }
        v
    };
    let (base, table): (u64, &[u8; 16]) = match c {
        b'o' => (8, HEX_LOWER),
        b'u' => (10, HEX_LOWER),
        b'x' => (16, HEX_LOWER),
        _ => (16, HEX_UPPER),
    };
    let mut buf = [0u8; 24];
    let mut k = buf.len();
    let mut v = uval;
    loop {
        k -= 1;
        buf[k] = table[(v % base) as usize];
        v /= base;
        if v == 0 {
            break;
        }
    }
    // Com `#` e valor não nulo: `0x`/`0X` no hexadecimal; no octal o gawk sempre acrescenta um `0`
    // na frente dos zeros da precisão, mesmo que eles já comecem com zero (`%#.3o` de 8 dá `0010`).
    let prefix: &[u8] = match (sp.alt && x != 0.0, c) {
        (true, b'x') => b"0x",
        (true, b'X') => b"0X",
        (true, b'o') => b"0",
        _ => b"",
    };
    emit_int(out, sp, prefix, &buf[k..])?;
    Ok(None)
}

/// Saída de inteiro: prefixo (sinal ou `0x`), zeros da precisão, dígitos, alinhados na largura.
/// Preenche com zeros só com `0`, sem `-` e sem precisão; os zeros entram depois do prefixo.
fn emit_int(out: &mut Vec<u8>, sp: &Spec, prefix: &[u8], digits: &[u8]) -> Result<(), FormatError> {
    let prec_zeros = if sp.have_prec { (sp.prec.saturating_sub(digits.len() as i64)).max(0) } else { 0 };
    let len = (prefix.len() as i64 + digits.len() as i64).saturating_add(prec_zeros);
    let fill_zero = !sp.lj && sp.zero && !sp.have_prec;
    let gap = sp.fw.saturating_sub(len);
    if sp.lj {
        out.extend_from_slice(prefix);
        pad(out, b'0', prec_zeros, LINE_PAD_LEFT)?;
        out.extend_from_slice(digits);
        pad(out, b' ', gap, LINE_PAD_RIGHT)?;
    } else if fill_zero {
        out.extend_from_slice(prefix);
        pad(out, b'0', gap, LINE_PAD_LEFT)?;
        out.extend_from_slice(digits);
    } else {
        pad(out, b' ', gap, LINE_PAD_LEFT)?;
        out.extend_from_slice(prefix);
        pad(out, b'0', prec_zeros, LINE_PAD_LEFT)?;
        out.extend_from_slice(digits);
    }
    Ok(())
}

/// Texto do gawk para NaN e infinito (`format_nan_inf`).
fn nan_inf_text(x: f64, c: u8) -> &'static str {
    let upper = c.is_ascii_uppercase();
    match (x.is_nan(), x.is_sign_negative(), upper) {
        (true, true, false) => "-nan",
        (true, false, false) => "+nan",
        (true, true, true) => "-NAN",
        (true, false, true) => "+NAN",
        (false, true, false) => "-inf",
        (false, false, false) => "+inf",
        (false, true, true) => "-INF",
        (false, false, true) => "+INF",
    }
}

/// Flags do `printf` do C que o gawk repassa no caminho de ponto flutuante.
#[derive(Clone, Copy, Default)]
struct CFlags {
    lj: bool,
    sign: u8,
    alt: bool,
    zero: bool,
}

const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";
const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

/// `printf("%<flags>*.*<conv>", width, prec, x)` do glibc, com `conv` em `aAeEfFgG`.
/// `prec` negativa vale como omitida (6, ou a representação exata no `%a`).
fn c_float(out: &mut Vec<u8>, conv: u8, x: f64, flags: CFlags, width: i32, prec: i32) {
    let mut lj = flags.lj;
    let w = if width < 0 {
        lj = true;
        u64::from(width.unsigned_abs())
    } else {
        width as u64
    };
    let upper = conv.is_ascii_uppercase();
    let sign: &[u8] = if x.is_sign_negative() {
        b"-"
    } else if flags.sign != 0 {
        std::slice::from_ref(&flags.sign)
    } else {
        b""
    };
    let ax = x.abs();
    let prec = usize::try_from(prec).ok();
    let mut prefix: &[u8] = b"";
    let body: Vec<u8> = if x.is_nan() {
        (if upper { "NAN" } else { "nan" }).into()
    } else if x.is_infinite() {
        (if upper { "INF" } else { "inf" }).into()
    } else {
        match conv.to_ascii_lowercase() {
            b'f' => fixed_body(ax, prec.unwrap_or(6), flags.alt),
            b'e' => exp_body(ax, prec.unwrap_or(6), flags.alt, upper),
            b'g' => general_body(ax, prec.unwrap_or(6), flags.alt, upper),
            _ => {
                prefix = if upper { b"0X" } else { b"0x" };
                hex_body(ax, prec, flags.alt, upper)
            }
        }
    };
    let len = (sign.len() + prefix.len() + body.len()) as u64;
    let gap = w.saturating_sub(len) as usize;
    if lj {
        out.extend_from_slice(sign);
        out.extend_from_slice(prefix);
        out.extend_from_slice(&body);
        out.resize(out.len() + gap, b' ');
    } else if flags.zero && x.is_finite() {
        out.extend_from_slice(sign);
        out.extend_from_slice(prefix);
        out.resize(out.len() + gap, b'0');
        out.extend_from_slice(&body);
    } else {
        out.resize(out.len() + gap, b' ');
        out.extend_from_slice(sign);
        out.extend_from_slice(prefix);
        out.extend_from_slice(&body);
    }
}

/// `%.<p>f` de `ax >= 0` (a formatação do Rust com precisão explícita é exata e arredonda o
/// empate para o par, como o glibc).
fn fixed_body(ax: f64, p: usize, alt: bool) -> Vec<u8> {
    let mut s = format!("{ax:.p$}").into_bytes();
    if alt && p == 0 {
        s.push(b'.');
    }
    s
}

/// Mantissa e expoente decimal de `ax` com `p` dígitos depois do ponto.
fn exp_parts(ax: f64, p: usize) -> (String, i32) {
    let s = format!("{ax:.p$e}");
    let k = s.rfind('e').unwrap_or(s.len());
    let exp = s[k + 1..].parse().unwrap_or(0);
    let mut mant = s;
    mant.truncate(k);
    (mant, exp)
}

/// Acrescenta o expoente no estilo do C: sinal sempre e pelo menos dois dígitos.
fn push_exp(out: &mut Vec<u8>, exp: i32, upper: bool) {
    out.push(if upper { b'E' } else { b'e' });
    out.push(if exp < 0 { b'-' } else { b'+' });
    let a = exp.unsigned_abs();
    if a < 10 {
        out.push(b'0');
    }
    let mut buf = [0u8; 20];
    out.extend_from_slice(u64_digits(u64::from(a), &mut buf));
}

/// `%.<p>e` de `ax >= 0`.
fn exp_body(ax: f64, p: usize, alt: bool, upper: bool) -> Vec<u8> {
    let (mant, exp) = exp_parts(ax, p);
    let mut out = mant.into_bytes();
    if alt && p == 0 {
        out.push(b'.');
    }
    push_exp(&mut out, exp, upper);
    out
}

/// `%.<p>g` de `ax >= 0`.
fn general_body(ax: f64, p: usize, alt: bool, upper: bool) -> Vec<u8> {
    let p = p.max(1);
    let (mant, exp) = exp_parts(ax, p - 1);
    let mut out;
    if exp >= -4 && i64::from(exp) < p as i64 {
        // Estilo `%f` com `P-1-X` casas: arredonda na mesma posição decimal que o `%e` com `P-1`
        // casas (no caso em que o arredondamento sobe o expoente os dois dão a potência de 10),
        // então são os mesmos `P` dígitos com o ponto em outro lugar.
        let digits: Vec<u8> = mant.bytes().filter(|&b| b != b'.').collect();
        out = Vec::with_capacity(p + 8);
        if exp >= 0 {
            let int_len = exp as usize + 1;
            out.extend_from_slice(&digits[..int_len]);
            if int_len < digits.len() || alt {
                out.push(b'.');
            }
            out.extend_from_slice(&digits[int_len..]);
        } else {
            out.extend_from_slice(b"0.");
            out.resize(out.len() + exp.unsigned_abs() as usize - 1, b'0');
            out.extend_from_slice(&digits);
        }
        if !alt {
            strip_fraction_zeros(&mut out);
        }
    } else {
        out = mant.into_bytes();
        if alt {
            // Peculiaridade do glibc: se antes de arredondar o valor caía no estilo `%f` e o
            // arredondamento subiu o expoente para `P`, o `%#g` sai sem os zeros (`1.e+06` para
            // 999999.5, e não `1.00000e+06`). Sem `#` os zeros sumiriam de qualquer jeito.
            // Com 21 algarismos o arredondamento nunca sobe de expoente (o `double` imediatamente
            // abaixo de uma potência de 10 tem no máximo 16 noves), então é o expoente exato.
            if i64::from(exp) == p as i64 && i64::from(exp_parts(ax, 20).1) == p as i64 - 1 {
                out = b"1.".to_vec();
            } else if !out.contains(&b'.') {
                out.push(b'.');
            }
        } else {
            strip_fraction_zeros(&mut out);
        }
        push_exp(&mut out, exp, upper);
    }
    out
}

/// Tira zeros à direita da parte fracionária e o ponto, se sobrar sozinho.
fn strip_fraction_zeros(s: &mut Vec<u8>) {
    if !s.contains(&b'.') {
        return;
    }
    while s.last() == Some(&b'0') {
        s.pop();
    }
    if s.last() == Some(&b'.') {
        s.pop();
    }
}

/// Corpo do `%a` do glibc (depois do `0x`) para `ax >= 0`: dígito inicial `0` só para zero e
/// subnormais (expoente fixo `-1022`), sem renormalizar depois do arredondamento (`0x2.0p+0`).
fn hex_body(ax: f64, prec: Option<usize>, alt: bool, upper: bool) -> Vec<u8> {
    let table = if upper { HEX_UPPER } else { HEX_LOWER };
    let bits = ax.to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    let (lead, exp) = if biased == 0 { (0u64, if frac == 0 { 0 } else { -1022 }) } else { (1u64, biased - 1023) };
    let mut out = Vec::with_capacity(24);
    let mut frac_digits: Vec<u8> = Vec::with_capacity(16);
    let lead_digit = match prec {
        None => {
            let mut f = frac;
            let mut nd = 13;
            while nd > 0 && f & 0xf == 0 {
                f >>= 4;
                nd -= 1;
            }
            for k in (0..nd).rev() {
                frac_digits.push(table[((f >> (4 * k)) & 0xf) as usize]);
            }
            lead
        }
        Some(p) if p >= 13 => {
            for k in (0..13).rev() {
                frac_digits.push(table[((frac >> (4 * k)) & 0xf) as usize]);
            }
            frac_digits.resize(p, b'0');
            lead
        }
        Some(p) => {
            // Arredonda para o mais próximo, empate para o par, no inteiro `lead.frac` de 53 bits.
            let full = (lead << 52) | frac;
            let shift = (13 - p) * 4;
            let mut q = full >> shift;
            let rem = full & ((1u64 << shift) - 1);
            let half = 1u64 << (shift - 1);
            if rem > half || (rem == half && q & 1 == 1) {
                q += 1;
            }
            let fbits = p * 4;
            let fq = if fbits == 0 { 0 } else { q & ((1u64 << fbits) - 1) };
            for k in (0..p).rev() {
                frac_digits.push(table[((fq >> (4 * k)) & 0xf) as usize]);
            }
            q >> fbits
        }
    };
    out.push(table[lead_digit as usize]);
    if !frac_digits.is_empty() || alt {
        out.push(b'.');
    }
    out.extend_from_slice(&frac_digits);
    out.push(if upper { b'P' } else { b'p' });
    out.push(if exp < 0 { b'-' } else { b'+' });
    let mut buf = [0u8; 20];
    out.extend_from_slice(u64_digits(u64::from(exp.unsigned_abs()), &mut buf));
    out
}

/// Dígitos decimais de `v` no fim de `buf`.
fn u64_digits(mut v: u64, buf: &mut [u8; 20]) -> &[u8] {
    let mut k = buf.len();
    loop {
        k -= 1;
        buf[k] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    &buf[k..]
}

/// Dígitos exatos do inteiro `v >= 0` representado em `double` (o `%.0f` do gawk).
fn push_int_digits(out: &mut Vec<u8>, v: f64) {
    if v < 18_446_744_073_709_551_616.0 {
        let mut buf = [0u8; 20];
        out.extend_from_slice(u64_digits(v as u64, &mut buf));
    } else {
        out.extend_from_slice(format!("{v:.0}").as_bytes());
    }
}

/// Conversão `double` para `int64_t` como o `cvttsd2si` do x86-64: NaN e fora do intervalo dão `i64::MIN`.
fn cvt_i64(x: f64) -> i64 {
    if x.is_nan() || x >= 9_223_372_036_854_775_808.0 || x < -9_223_372_036_854_775_808.0 {
        i64::MIN
    } else {
        x as i64
    }
}

/// Conversão `double` para `uint64_t` como o gcc gera no x86-64 (o gawk compilado no host):
/// abaixo de 2^63 passa por `cvttsd2si`; a partir daí subtrai 2^63 e liga o bit alto.
fn cvt_u64(x: f64) -> u64 {
    const TWO63: f64 = 9_223_372_036_854_775_808.0;
    if x >= TWO63 {
        (cvt_i64(x - TWO63) as u64) ^ (1u64 << 63)
    } else {
        cvt_i64(x) as u64
    }
}

/// Codifica `wc` (até 0x7FFFFFFF) como o `wcrtomb` do glibc em C.UTF-8 (UTF-8 estendido de até 6 bytes).
fn encode_wc(wc: u32, buf: &mut [u8; 6]) -> usize {
    if wc < 0x80 {
        buf[0] = wc as u8;
        return 1;
    }
    let (n, lead): (usize, u8) = match wc {
        0..=0x7ff => (2, 0xc0),
        0x800..=0xffff => (3, 0xe0),
        0x1_0000..=0x1f_ffff => (4, 0xf0),
        0x20_0000..=0x3ff_ffff => (5, 0xf8),
        _ => (6, 0xfc),
    };
    let mut v = wc;
    for k in (1..n).rev() {
        buf[k] = 0x80 | (v & 0x3f) as u8;
        v >>= 6;
    }
    buf[0] = lead | v as u8;
    n
}

/// O `mbrlen` do glibc em C.UTF-8 no início de `s`: `Some(n)` para caractere completo e válido
/// (`Some(0)` para o NUL), `None` para sequência inválida, incompleta ou texto vazio. Aceita o UTF-8
/// estendido até 0x7FFFFFFF e rejeita formas longas demais e substitutos UTF-16.
fn mb_len(s: &[u8]) -> Option<usize> {
    let b0 = *s.first()?;
    if b0 == 0 {
        return Some(0);
    }
    if b0 < 0x80 {
        return Some(1);
    }
    let (n, min, init): (usize, u32, u32) = match b0 {
        0xc2..=0xdf => (2, 0x80, u32::from(b0 & 0x1f)),
        0xe0..=0xef => (3, 0x800, u32::from(b0 & 0x0f)),
        0xf0..=0xf7 => (4, 0x1_0000, u32::from(b0 & 0x07)),
        0xf8..=0xfb => (5, 0x20_0000, u32::from(b0 & 0x03)),
        0xfc..=0xfd => (6, 0x400_0000, u32::from(b0 & 0x01)),
        _ => return None,
    };
    if s.len() < n {
        return None;
    }
    let mut v = init;
    for &b in &s[1..n] {
        if b & 0xc0 != 0x80 {
            return None;
        }
        v = (v << 6) | u32::from(b & 0x3f);
    }
    if v < min || (0xd800..=0xdfff).contains(&v) {
        return None;
    }
    Some(n)
}

/// Quantos caracteres o `%s` do gawk enxerga em `s` (ver o cabeçalho do módulo).
fn char_count(s: &[u8]) -> usize {
    if !matches!(mb_len(s), Some(n) if n > 0) {
        return s.len();
    }
    let mut i = 0;
    let mut count = 0;
    while i < s.len() {
        let b = s[i];
        let n = if b != 0 && b < 0x80 {
            1
        } else {
            match mb_len(&s[i..]) {
                Some(n) if n > 0 => n,
                _ => break,
            }
        };
        i += n;
        count += 1;
    }
    count
}

/// Bytes dos primeiros `chars` caracteres de `s`, na mesma contagem de [`char_count`].
fn byte_count(s: &[u8], chars: usize) -> usize {
    if !matches!(mb_len(s), Some(n) if n > 0) {
        return chars.min(s.len());
    }
    let mut i = 0;
    for _ in 0..chars {
        match mb_len(&s[i..]) {
            Some(n) if n > 0 => i += n,
            _ => break,
        }
    }
    i
}

/// Valor numérico de um texto pela regra do awk (o `force_number` do gawk em constante de texto):
/// espaços iniciais, prefixo decimal com sinal, ponto e expoente; `+inf`, `-inf`, `+nan` e `-nan`
/// exatos; o resto vale 0 (inclusive hexadecimal e `inf`/`nan` sem sinal). Só o `*0$` usa isto.
fn awk_str_to_num(s: &[u8]) -> f64 {
    let is_space = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let t = &s[i..];
    if t.len() >= 4 && matches!(t[0], b'+' | b'-') && t[1].is_ascii_alphabetic() {
        let word = &t[1..4];
        if t[4..].iter().all(|&b| is_space(b)) {
            let neg = t[0] == b'-';
            if word.eq_ignore_ascii_case(b"inf") {
                return if neg { f64::NEG_INFINITY } else { f64::INFINITY };
            }
            if word.eq_ignore_ascii_case(b"nan") {
                return if neg { -f64::NAN } else { f64::NAN };
            }
        }
        return 0.0;
    }
    let mut j = 0;
    if j < t.len() && matches!(t[j], b'+' | b'-') {
        j += 1;
    }
    let int_start = j;
    while j < t.len() && t[j].is_ascii_digit() {
        j += 1;
    }
    let mut digits = j - int_start;
    if j < t.len() && t[j] == b'.' {
        let k = j + 1;
        let mut m = k;
        while m < t.len() && t[m].is_ascii_digit() {
            m += 1;
        }
        digits += m - k;
        j = m;
    }
    if digits == 0 {
        return 0.0;
    }
    if j < t.len() && matches!(t[j], b'e' | b'E') {
        let mut m = j + 1;
        if m < t.len() && matches!(t[m], b'+' | b'-') {
            m += 1;
        }
        let e0 = m;
        while m < t.len() && t[m].is_ascii_digit() {
            m += 1;
        }
        if m > e0 {
            j = m;
        }
    }
    std::str::from_utf8(&t[..j]).ok().and_then(|v| v.parse().ok()).unwrap_or(0.0)
}

/// Casos fixos com a saída do gawk 5.2.1 do host (`LC_ALL=C.UTF-8`), capturada como caixa-preta.
/// `N` é número, `S` é texto constante e `F` é strnum (texto de entrada com cara de número).
#[cfg(test)]
// Valores como 3.14159 são entradas dos testes do gawk, não aproximações de constantes.
#[allow(clippy::approx_constant)]
mod tests {
    use super::*;

    enum A {
        N(f64),
        S(&'static [u8]),
        F(&'static [u8]),
    }
    use A::{F, N, S};

    impl FmtArg for A {
        fn to_num(&self) -> f64 {
            match self {
                N(x) => *x,
                // Constante de texto: o gawk perde o sinal do zero.
                S(s) => {
                    let v = awk_str_to_num(s);
                    if v == 0.0 { 0.0 } else { v }
                }
                F(s) => awk_str_to_num(s),
            }
        }
        fn to_str(&self) -> Cow<'_, [u8]> {
            match self {
                N(x) => Cow::Owned(num_to_str(*x, b"%.6g")),
                S(s) | F(s) => Cow::Borrowed(s),
            }
        }
        fn is_numeric(&self) -> bool {
            !matches!(self, S(_))
        }
    }

    #[track_caller]
    fn fmt(f: &[u8], args: &[A], expected: Result<&[u8], &str>) {
        let refs: Vec<&dyn FmtArg> = args.iter().map(|a| a as &dyn FmtArg).collect();
        let mut warnings = Vec::new();
        let got = format(f, &refs, &mut warnings);
        let want = expected.map(<[u8]>::to_vec).map_err(|m| FormatError(m.to_string()));
        assert_eq!(got, want, "formato {:?}", String::from_utf8_lossy(f));
        assert!(warnings.is_empty());
    }

    #[track_caller]
    fn conv(f: &[u8], x: f64, expected: Result<&[u8], &str>) {
        let want = expected.map(<[u8]>::to_vec).map_err(|m| FormatError(m.to_string()));
        assert_eq!(try_num_to_str(x, f), want, "CONVFMT {:?}", String::from_utf8_lossy(f));
    }

    #[test]
    #[rustfmt::skip]
    fn gawk_suite() {
        // printf1, printfbad3, zeroflag, pcntplus, intprec, printfchar, mbprintf2, zero2, nofmtch,
        // printfbad1, printfbad4 e o golden de argumentos insuficientes.
        fmt(b"|%8.5d|", &[N(1e2)], Ok(b"|   00100|"));
        fmt(b"|%#o|", &[N(0e0)], Ok(b"|0|"));
        fmt(b"|%#.1o|", &[N(0e0)], Ok(b"|0|"));
        fmt(b"|%#.0o|", &[N(0e0)], Ok(b"|0|"));
        fmt(b"|%#x|", &[N(0e0)], Ok(b"|0|"));
        fmt(b"|%.0d|", &[N(0e0)], Ok(b"||"));
        fmt(b"|%5.0d|", &[N(0e0)], Ok(b"|     |"));
        fmt(b">>%.0x<< >>%#x<< >>%#x<<\n", &[N(0e0), N(1.67e2), N(1.67e2)], Ok(b">><< >>0xa7<< >>0xa7<<\n"));
        fmt(b"%2.1d---%02.1d\n", &[N(2e0), N(2e0)], Ok(b" 2--- 2\n"));
        fmt(b"%+d %d\n", &[N(3e0), N(4e0)], Ok(b"+3 4\n"));
        fmt(b"%.10d:%.10x\n", &[N(5e0), N(1.4e1)], Ok(b"0000000005:000000000e\n"));
        fmt(b"%c\n", &[S(b"65")], Ok(b"6\n"));
        fmt(b"%c\n", &[N(6.5e1)], Ok(b"A\n"));
        fmt(b"%c\n", &[S(b"AA")], Ok(b"A\n"));
        fmt(b"%d\n", &[N(-4e-1)], Ok(b"0\n"));
        fmt(b"%d\n", &[N(-0.0)], Ok(b"0\n"));
        fmt(b"%d\n", &[N(-9e-1)], Ok(b"0\n"));
        fmt(b"%3", &[N(1e0)], Ok(b"%3"));
        fmt(b"%3$*10$.*1$s\n", &[N(2e1), N(1e1), S(b"hello")], Err("not enough arguments to satisfy format string\n\t`%3$*10$.*1$s\n'\n\t      ^ ran out for this one"));
        fmt(b"%03$*d %2$d \n", &[N(4e0), N(5e0), N(1e0)], Err("fatal: must use `count$' on all formats or none"));
        fmt(b"%s %s\n", &[S(b"a")], Err("not enough arguments to satisfy format string\n\t`%s %s\n'\n\t    ^ ran out for this one"));
        // fmttest (Beebe)
        fmt(b"%c|%.15c|%15c|%-15c", &[S(b"ABC"), S(b"ABC"), S(b"ABC"), S(b"ABC")], Ok(b"A|A|              A|A              "));
        fmt(b"%c|%.15c|%15c|%-15c", &[N(1.23e2), N(1.23e2), N(1.23e2), N(1.23e2)], Ok(b"{|{|              {|{              "));
        fmt(b"%d|%.15d|%15d|%-15d", &[S(b"ABC"), S(b"ABC"), N(1.23e2), N(1.23e2)], Ok(b"0|000000000000000|            123|123            "));
        fmt(b"%e|%.25e|%25e|%-25e", &[N(1.23e2), N(1.23e2), N(1.23e2), N(1.23e2)], Ok(b"1.230000e+02|1.2300000000000000000000000e+02|             1.230000e+02|1.230000e+02             "));
        fmt(b"%f|%.25f|%25f|%-25f", &[N(1.23e2), N(1.23e2), N(1.23e2), N(1.23e2)], Ok(b"123.000000|123.0000000000000000000000000|               123.000000|123.000000               "));
        fmt(b"%g|%.25g|%25g|%-25g", &[N(1.23e2), N(1.23e2), N(1.23e2), N(1.23e2)], Ok(b"123|123|                      123|123                      "));
        fmt(b"%o|%.15o|%15o|%-15o", &[N(1.23e2), N(1.23e2), N(1.23e2), N(1.23e2)], Ok(b"173|000000000000173|            173|173            "));
        fmt(b"%u|%.15u|%15u|%-15u|%X|%.15X", &[N(1.23e2), N(1.23e2), N(1.23e2), N(1.23e2), N(1.23e2), N(1.23e2)], Ok(b"123|000000000000123|            123|123            |7B|00000000000007B"));
    }

    #[test]
    #[rustfmt::skip]
    fn integers() {
        fmt(b"%d|%i|%d|%d", &[N(1.1805916207174113e21), N(1e30), N(-1e300), N(9.007199254740992e15)], Ok(b"1180591620717411303424|1000000000000000019884624838656|-1000000000000000052504760255204420248704468581108159154915854115511802457988908195786371375080447864043704443832883878176942523235360430575644792184786706982848387200926575803737830233794788090059368953234970799945081119038967640880074652742780142494579258788820056842838115669472196386865459400540160|9007199254740992"));
        fmt(b"%d|%d|%d|%+d|% d|%+d", &[N(2.9e0), N(-2.9e0), N(-5e-1), N(-5e-1), N(7e0), N(-7e0)], Ok(b"2|-2|0|+0| 7|-7"));
        fmt(b"%u|%u|%x|%o|%u|%x|%X|%x|%x", &[N(-1e0), N(-9.223372036854776e18), N(-1.5e0), N(-1e0), N(-1e19), N(1.8446744073709552e19), N(1.844674407370955e19), N(9.223372036854776e18), N(1.1805916207174113e21)], Ok(b"18446744073709551615|9223372036854775808|ffffffffffffffff|1777777777777777777777|-1e+19|1.84467e+19|FFFFFFFFFFFFF800|8000000000000000|1.18059e+21"));
        fmt(b"[%.0x][%.0x][%#x][%#.0x][%#.0o][%#o][%#.3o][%#5o][%.0d][%.0u]", &[N(0e0), N(5e-1), N(5e-1), N(0e0), N(0e0), N(8e0), N(8e0), N(8e0), N(5e-1), N(5e-1)], Ok(b"[][0][0x0][0][0][010][0010][  010][][0]"));
        fmt(b"[%+u][% x][%+x][%#X][%#015x][%#-15x][%015x][%-+15.2x]", &[N(5e0), N(5e0), N(1.1805916207174113e21), N(1.1805916207174113e21), N(1.1805916207174113e21), N(1.1805916207174113e21), N(1.1805916207174113e21), N(1.1805916207174113e21)], Ok(b"[5][5][+1.18059e+21][1.18059e+21][00001.18059e+21][1.18059e+21    ][00001.18059e+21][+1.2e+21       ]"));
        fmt(b"[%05d][%-05d][%0+5d][% 05d][%#05x][%#5x][%#-5o][%05.3d][%-+8.3d]", &[N(4e0), N(4e0), N(6e0), N(7e0), N(8e0), N(9e0), N(1e1), N(1.1e1), N(-1.2e1)], Ok(b"[00004][4    ][+0006][ 0007][0x008][  0x9][012  ][  011][-012    ]"));
    }

    #[test]
    #[rustfmt::skip]
    fn nan_and_infinity() {
        fmt(b"[%10d][%-10d][%010d][%x][%X][%10f][%F][%E][%G][%A][%a][%5.2e][%s][%i][%o][%u]", &[N(-f64::NAN), N(f64::NAN), N(f64::INFINITY), N(-f64::NAN), N(f64::NEG_INFINITY), N(-f64::NAN), N(f64::INFINITY), N(f64::NAN), N(f64::INFINITY), N(f64::NEG_INFINITY), N(-f64::NAN), N(f64::INFINITY), N(-f64::NAN), N(f64::INFINITY), N(f64::NEG_INFINITY), N(f64::NAN)], Ok(b"[-nan][+nan][+inf][-nan][-INF][-nan][+INF][+NAN][+INF][-INF][-nan][+inf][-nan][+inf][-inf][+nan]"));
        fmt(b"[%Pf][%Pd][%10Pf][%PF][%Px][%Pg][%PPf][%PE][%P5.1e][%-+P10f][%0P8e]", &[N(-f64::NAN), N(f64::NAN), N(f64::INFINITY), N(f64::NEG_INFINITY), N(-f64::NAN), N(f64::INFINITY), N(f64::NAN), N(-f64::NAN), N(f64::INFINITY), N(f64::INFINITY)], Ok(b"[-nan][nan][       inf][-INF][-nan][inf][%PPf][NAN][ -nan][+inf      ][     inf]"));
        fmt(b"[%c][%c][%c][%c]", &[N(f64::NAN), N(-f64::NAN), N(f64::INFINITY), N(f64::NEG_INFINITY)], Ok(b"[\x00][\x00][\x00][\x00]"));
    }

    #[test]
    #[rustfmt::skip]
    fn chars() {
        fmt(b"[%c][%c][%c][%c][%c][%c][%c][%c][%c][%c][%c]", &[N(-1e0), N(1.8446744073709552e19), N(1e30), N(4.294967361e9), N(6.59e1), N(0e0), N(2.55e2), N(2.56e2), N(5.5296e4), N(1.114111e6), N(1.114112e6)], Ok(b"[\xff][\x00][\x00][A][A][\x00][\xc3\xbf][\xc4\x80][\x00][\xf4\x8f\xbf\xbf][\xf4\x90\x80\x80]"));
        fmt(b"[%c][%c][%c][%c][%c][%c][%c][%c]", &[N(2.147483648e9), N(2.147483713e9), N(-6.5e1), N(9.223372036854776e18), N(2.147483647e9), N(5.7343e4), N(6.5534e4), N(-9.223372036854776e18)], Ok(b"[\x00][A][\xbf][\x00][\xfd\xbf\xbf\xbf\xbf\xbf][\xff][\xef\xbf\xbe][\x00]"));
        fmt(b"[%3c][%-3c][%3c][%-3c][%03c][%.0c][%3.0c][%c]", &[N(2.52e2), N(2.52e2), S(b"\xc3\xbc"), S(b"\xc3\xbcx"), N(6.5e1), N(6.6e1), S(b"abc"), S(b"")], Ok(b"[ \xc3\xbc][\xc3\xbc ][  \xc3\xbc][\xc3\xbc  ][  A][B][  a][\x00]"));
        fmt(b"[%c][%3c][%-3c][%.0c]", &[F(b"65"), F(b" 65 "), S(b"0x41"), F(b"1e2")], Ok(b"[A][  A][0  ][d]"));
        fmt(b"[%c][%3c][%-3c][%c][%3c]", &[S(b"\xff"), S(b"\xff"), S(b"\xc3"), S(b"\x00ab"), S(b"\xe2\x82\xac!")], Ok(b"[\xff][  \xff][\xc3  ][\x00][  \xe2\x82\xac]"));
    }

    #[test]
    #[rustfmt::skip]
    fn strings() {
        fmt(b"[%10s][%10s][%10s][%10s][%10s]", &[S(b"Z\xe2\x82"), S(b"\xe2\x82"), S(b"\xc3Z"), S(b"Z\xc3"), S(b"\xe2\x82Z")], Ok(b"[         Z][        \xe2\x82][        \xc3Z][         Z][       \xe2\x82Z]"));
        fmt(b"[%10s][%10s][%10s][%10s][%10s]", &[S(b"a\xe2\x82b"), S(b"\xc3\xa9\xe2\x82"), S(b"\xe2\x82\xc3\xa9"), S(b"ab\xc3cd"), S(b"\xf0\x9f\x98\x80Z")], Ok(b"[         a][         \xc3\xa9][      \xe2\x82\xc3\xa9][        ab][        \xf0\x9f\x98\x80Z]"));
        fmt(b"[%.1s][%.2s][%.3s][%.4s][%6.2s]", &[S(b"\xe2\xc3\xa9Z"), S(b"\xe2\xc3\xa9Z"), S(b"\xe2\xc3\xa9Z"), S(b"\xe2\xc3\xa9Z"), S(b"\xe2\xc3\xa9Z")], Ok(b"[\xe2][\xe2\xc3][\xe2\xc3\xa9][\xe2\xc3\xa9Z][    \xe2\xc3]"));
        fmt(b"[%5s][%.1s][%.2s][%s][%c]", &[S(b"\x00ab"), S(b"\x00ab"), S(b"\x00ab"), S(b"\x00ab"), S(b"\x00ab")], Ok(b"[  \x00ab][\x00][\x00a][\x00ab][\x00]"));
        fmt(b"[%5s][%.3s][%s][%-4s]", &[S(b"ab\x00"), S(b"ab\x00"), S(b"ab\x00"), S(b"a\x00b")], Ok(b"[   ab][ab][ab\x00][a   ]"));
        fmt(b"[%5s][%5s][%5s][%5s][%5s][%5s]", &[S(b"\xed\xa0\x80Z"), S(b"\xc0\x80Z"), S(b"\xf4\x90\x80\x80Z"), S(b"\xf8\x88\x80\x80\x80Z"), S(b"\xfd\xbf\xbf\xbf\xbf\xbfZ"), S(b"\xf0\x80\x80\x80Z")], Ok(b"[ \xed\xa0\x80Z][  \xc0\x80Z][   \xf4\x90\x80\x80Z][   \xf8\x88\x80\x80\x80Z][   \xfd\xbf\xbf\xbf\xbf\xbfZ][\xf0\x80\x80\x80Z]"));
        fmt(b"[%-7s|][%7s][%.2s][%3.1s]", &[S(b"\xc3\x85\xc3\x83\xc3\x86"), S(b"\xc3\xbaltimo"), S(b"\xc3\xbaltimo"), S(b"\xc3\xbaltimo")], Ok(b"[\xc3\x85\xc3\x83\xc3\x86    |][ \xc3\xbaltimo][\xc3\xbal][  \xc3\xba]"));
        // `.-` com `-` ativo deixa a precisão ligada e negativa: conta caracteres e para no inválido.
        fmt(b"%- 0.-0s|", &[S(b"0x1A\x80")], Ok(b"0x1A|"));
    }

    #[test]
    #[rustfmt::skip]
    fn flags_and_invalid_specs() {
        fmt(b"[%5-d][%5 d][%1 d][%5+d][%5#x][%.3-d][%-.3d][%.-3d][% +d][%+ d][%'d][%5'd][%.5'd]", &[N(1e0), N(2e0), N(2e0), N(3e0), N(4e0), N(5e0), N(6e0), N(7e0), N(8e0), N(9e0), N(1e1), N(1.1e1), N(1.2e1)], Ok(b"[1    ][    2][ 2][   +3][  0x4][%.3-d][005][6][+7][+8][9][   10][%.5'd]"));
        fmt(b"[%.-3s|%.-s|%.-0s|%.-10s]", &[S(b"abcdef"), S(b"abc"), S(b"abc"), S(b"abc")], Ok(b"[abcdef|abc|abc|abc]"));
        fmt(b"[%-+5d][%+-5d][%0-5d][%-05d][%00005d][%0+5d][% 05d][%#05x][%#5x][%#-5o]", &[N(1e0), N(2e0), N(3e0), N(4e0), N(5e0), N(6e0), N(7e0), N(8e0), N(9e0), N(1e1)], Ok(b"[+1   ][+2   ][3    ][4    ][00005][+0006][ 0007][0x008][  0x9][012  ]"));
        fmt(b"[%jjd][%zzd][%ttd][%LLf][%hld][%lhd][%Lld][%lLd][%hLd][%jzd][%l5d][%5ld][%.3ld][%.3l5d][%-l5d][%l-5d]", &[N(1e0), N(2e0), N(3e0), N(4e0), N(5e0), N(6e0), N(7e0), N(8e0), N(9e0), N(1e1), N(1.1e1), N(1.2e1), N(1.3e1), N(1.4e1), N(1.5e1), N(1.6e1)], Ok(b"[%jjd][%zzd][%ttd][%LLf][1][2][3][4][5][6][    7][    8][009][%.3l5d][10   ][11   ]"));
        fmt(b"[%5*d][%**d][%.**d][%*.*.*d][%-*d][%.*d][%.*d][%*-d]", &[N(1e0), N(2e0), N(3e0), N(4e0), N(5e0), N(6e0), N(7e0), N(8e0), N(9e0), N(1e1), N(1.1e1), N(1.2e1), N(1.3e1), N(1.4e1), N(1.5e1), N(1.6e1), N(1.7e1), N(1.8e1), N(1.9e1), N(2e1)], Ok(b"[2][   5][%.**d][%*.*.*d][10       ][00000000012][0000000000014][16             ]"));
        fmt(b"[%5+0d][%5-0d][%5+3d][%-.0*d][%.0*d][%5.3 d][%5.3#x][%.3.4d][%..3d][%5.3.d][%5.3*d]", &[N(1e0), N(2e0), N(3e0), N(4e0), N(5e0), N(6e0), N(7e0), N(8e0), N(9e0), N(1e1), N(1.1e1), N(1.2e1), N(1.3e1), N(1.4e1), N(1.5e1)], Ok(b"[+1][2    ][ +3][0005][%.0*d][%5.3 d][%5.3#x][%.3.4d][%..3d][%5.3.d][%5.3*d]"));
        fmt(b"[%*d][%*d][%-*d][%.*d][%.*d][%.*f][%*.*f]", &[N(-5e0), N(1e0), S(b"3x"), N(2e0), N(-4e0), N(3e0), N(-3e0), N(4e0), S(b"-2"), N(5e0), N(-1e0), N(3.14159e0), N(2.9e0), N(1.9e0), N(2.71828e0)], Ok(b"[1    ][  2][3   ][4][5][3.141590][2.7]"));
        fmt(b"[%05.-3d][%5.-d][%.-*d][%.-*d][%-.-3d][%0.-d][%.-0d]", &[N(7e0), N(8e0), N(3e0), N(9e0), N(-3e0), N(1e1), N(1.1e1), N(1.2e1), N(1.3e1)], Ok(b"[00007][    8][009][10][11][12][13]"));
        fmt(b"[%.3.%d][%z%d][%5%%d][%l%d][%.3.%%d][%lz%d]", &[N(5e0), N(6e0), N(7e0), N(8e0), N(9e0), N(1e1)], Ok(b"[%.3.5][%d][%6][%d][%.3.%d][%d]"));
        fmt(b"[%n][%p][%C][%S][%m][%Id][%qd][%w][%b][%k][%z][%Z]", &[N(6.5e1)], Ok(b"[%n][%p][%C][%S][%m][%Id][%qd][%w][%b][%k][%z][%Z]"));
        fmt(b"[%'5d][%#c][%+s][%+c][%05s][%05c][%#s][%5.2c][%-05s]", &[N(6.5e1), N(6.5e1), N(6.5e1), N(6.5e1), N(6.5e1), N(6.5e1), N(6.5e1), N(6.5e1), N(6.5e1)], Ok(b"[   65][A][65][A][   65][    A][65][    A][65   ]"));
        // O byte 0xFF é uma flag sem efeito; os outros bytes altos invalidam a especificação.
        fmt(b"[%\xffd][%\xff\xffd][%5\xff.2\xfff][%.\xff3d][%.3\xffd][%-\xff5d][%\xff%][%\xff]", &[N(1e0), N(2e0), N(3e0), N(4e0), N(5e0), N(6e0), N(7e0), N(8e0), N(9e0)], Ok(b"[1][2][%5\xff.2\xfff][%.\xff3d][%.3\xffd][3    ][%][%\xff]"));
        fmt(b"[%5\xffd][%.5\xffd][%*\xffd][%.*\xffd][%\xff5d][%\xff05d][%0\xff5d][%\xffx][%\xff.3f][%-5\xff-d][%5\xff+d]", &[N(1e0), N(2e0), N(3e0), N(4e0), N(5e0), N(6e0), N(7e0), N(8e0), N(9e0), N(1e1), N(1.1e1), N(1.2e1), N(1.3e1)], Ok(b"[    1][%.5\xffd][ 3][%.*\xffd][    5][00006][00007][8][9.000][10   ][  +11]"));
        fmt(b"[%\x80d][%\xfed][%3j\xff8c]", &[N(1e0), N(-1e30)], Ok(b"[%\x80d][%\xfed][       \x01]"));
        fmt(b"abc%5", &[N(1e0)], Ok(b"abc%5"));
        fmt(b"%.5", &[N(1e0)], Ok(b"%.5"));
        fmt(b"%-", &[N(1e0)], Ok(b"%-"));
        fmt(b"%*", &[N(1e0)], Ok(b"%*"));
        fmt(b"%1$", &[N(1e0)], Ok(b"%1$"));
        fmt(b"x%%y%", &[N(1e0)], Ok(b"x%y%"));
        fmt(b"%5%|%-5%|%.3%|%05%", &[N(1e0)], Ok(b"%|%|%|%"));
    }

    #[test]
    #[rustfmt::skip]
    fn positional() {
        fmt(b"%1$d %d\n", &[N(1e0), N(2e0)], Err("must use `count$' on all formats or none"));
        fmt(b"%d %1$d\n", &[N(1e0), N(2e0)], Err("must use `count$' on all formats or none"));
        fmt(b"%1$*d\n", &[N(1e0), N(2e0)], Err("fatal: must use `count$' on all formats or none"));
        fmt(b"%*1$d\n", &[N(1e0), N(2e0)], Ok(b"1\n"));
        fmt(b"%1$*2d\n", &[N(1e0), N(2e0)], Err("no `$' supplied for positional field width or precision"));
        fmt(b"%.1$d\n", &[N(1e0), N(2e0)], Err("`$' not permitted after period in format"));
        fmt(b"%1$*3$d\n", &[N(1e0), N(2e0)], Err("not enough arguments to satisfy format string\n\t`%1$*3$d\n'\n\t     ^ ran out for this one"));
        fmt(b"%1$*2$d|\n", &[N(1e0), N(5e0)], Ok(b"    1|\n"));
        fmt(b"%0$d", &[N(1e0)], Err("argument index with `$' must be > 0"));
        fmt(b"%2$d", &[N(1e0)], Err("argument index 2 greater than total number of supplied arguments"));
        fmt(b"%99999999999999999999$d", &[N(3e0), N(4e0)], Err("argument index 7766279631452241919 greater than total number of supplied arguments"));
        fmt(b"%9223372036854775808$d", &[N(3e0), N(4e0)], Err("argument index with `$' must be > 0"));
        fmt(b"%18446744073709551617$d", &[N(3e0), N(4e0)], Ok(b"3"));
        fmt(b"%*2147483648$d", &[N(3e0), N(4e0)], Err("not enough arguments to satisfy format string\n\t`%*2147483648$d'\n\t            ^ ran out for this one"));
        fmt(b"%*4294967297$d", &[N(3e0), N(4e0)], Ok(b"  3"));
        fmt(b"%-1$5d|%1$-5d|%1$.2d|%1$1$d|%1$2$d|%1$*1$d|%1$.*1$d|", &[N(3e0), N(4e0)], Ok(b"3    |3    |03|3|4|  3|003|"));
        fmt(b"%$d", &[N(3e0), N(4e0)], Err("argument index with `$' must be > 0"));
        fmt(b"%*$d", &[N(3e0), N(4e0)], Err("argument index 3 greater than total number of supplied arguments"));
        fmt(b"%1$*$d", &[N(3e0), N(4e0)], Err("fatal: must use `count$' on all formats or none"));
        fmt(b"%2$d %1$d|%2$s %1$x", &[N(3e0), N(4e0)], Ok(b"4 3|4 3"));
        // `*0$` lê a própria string de formato como número.
        fmt(b"%*0$d|", &[N(1e0), N(2e0)], Ok(b"1|"));
        fmt(b"12%*0$d|%.*0$f|", &[N(5e0), N(2e0)], Ok(b"12           5|2.000000000000|"));
        fmt(b"  -7.5e1xyz%*0$d|", &[N(5e0)], Ok(b"  -7.5e1xyz5                                                                          |"));
        fmt(b"0x10%*0$d|", &[N(5e0)], Ok(b"0x105|"));
        fmt(b"%*1$", &[N(1e0)], Ok(b"%*1$"));
    }

    #[test]
    #[rustfmt::skip]
    fn too_few_arguments() {
        fmt(b"%*", &[], Err("not enough arguments to satisfy format string\n\t`%*'\n\t ^ ran out for this one"));
        fmt(b"%.*", &[], Err("not enough arguments to satisfy format string\n\t`%.*'\n\t  ^ ran out for this one"));
        fmt(b"ab%*d", &[], Err("not enough arguments to satisfy format string\n\t`ab%*d'\n\t   ^ ran out for this one"));
        fmt(b"%d", &[], Err("not enough arguments to satisfy format string\n\t`%d'\n\t ^ ran out for this one"));
        fmt(b"%c", &[], Err("not enough arguments to satisfy format string\n\t`%c'\n\t ^ ran out for this one"));
        fmt(b"ab\x00cd%d|%s", &[], Err("not enough arguments to satisfy format string\n\t`ab'\n\t      ^ ran out for this one"));
        fmt(b"%*1$", &[], Err("not enough arguments to satisfy format string\n\t`%*1$'\n\t   ^ ran out for this one"));
        fmt(b"%d%d%d", &[N(1e0), N(2e0)], Err("not enough arguments to satisfy format string\n\t`%d%d%d'\n\t     ^ ran out for this one"));
    }

    #[test]
    #[rustfmt::skip]
    fn floats() {
        fmt(b"%.0f %.1f %.2f %.0e %.1e %.2e %.30f %.20e", &[N(5e-1), N(2.5e-1), N(1.25e-1), N(2.5e0), N(2.5e-1), N(1.25e-1), N(1e-1), N(1e-1)], Ok(b"0 0.2 0.12 2e+00 2.5e-01 1.25e-01 0.100000000000000005551115123126 1.00000000000000005551e-01"));
        fmt(b"%.0f %.0f %.0f %.0f %.2f %.2f %.3e", &[N(1.5e0), N(2.5e0), N(3.5e0), N(-5e-1), N(1.005e0), N(2.675e0), N(1.0005e0)], Ok(b"2 2 4 -0 1.00 2.67 1.000e+00"));
        fmt(b"%g %g %g %g %g %g %g %g %g", &[N(1e5), N(1e6), N(1e-4), N(1e-5), N(1.23456789e8), N(1.23456e-4), N(1e-300), N(5e-324), N(1.7976931348623157e308)], Ok(b"100000 1e+06 0.0001 1e-05 1.23457e+08 0.000123456 1e-300 4.94066e-324 1.79769e+308"));
        fmt(b"%#g %#.0g %#.3g %#g %.0g %.1g %#.0e %#.0f %.0e", &[N(1e0), N(1e0), N(1e2), N(1e10), N(5e-1), N(9.5e0), N(3e0), N(3e0), N(5e0)], Ok(b"1.00000 1. 100. 1.00000e+10 0.5 1e+01 3.e+00 3. 5e+00"));
        // Peculiaridade do glibc no `%#g` quando o arredondamento sobe o expoente até a precisão.
        fmt(b"%##4g|%-#7g|%#.3g|%#.2g|%#.10g|%#.2g", &[N(9.999995e5), N(9.999995e5), N(9.995e2), N(9.95e1), N(9.9999999995e9), N(9.96e0)], Ok(b"1.e+06|1.e+06 |1.e+03|1.e+02|1.e+10|10."));
        fmt(b"%G %E %F %e %f", &[N(1e-10), N(1.5e300), N(1e20), N(-0.0), N(-0.0)], Ok(b"1E-10 1.500000E+300 100000000000000000000.000000 -0.000000e+00 -0.000000"));
        fmt(b"%+.3e|% .3e|%-12.3e|%012.3e|%+012.3f|%-+12.2f|% 012g", &[N(1.2345e3), N(1.2345e3), N(1.2345e3), N(-1.2345e3), N(3.14159e0), N(2.5e0), N(4.2e1)], Ok(b"+1.234e+03| 1.234e+03|1.234e+03   |-001.234e+03|+0000003.142|+2.50       | 00000000042"));
        fmt(b"[%a][%a][%a][%a][%a][%A][%#a][%#.0a][%.0a][%.1a][%.1a][%.1a][%.1a][%.0a][%.0a][%.0a]", &[N(1e0), N(0e0), N(-0.0), N(1e-1), N(5e-324), N(2.555e2), N(1e0), N(1e0), N(1.5e0), N(1.03125e0), N(1.09375e0), N(1.96875e0), N(1.0312500000000002e0), N(2.5e0), N(3.5e0), N(1.5e0)], Ok(b"[0x1p+0][0x0p+0][-0x0p+0][0x1.999999999999ap-4][0x0.0000000000001p-1022][0X1.FFP+7][0x1.p+0][0x1.p+0][0x2p+0][0x1.0p+0][0x1.2p+0][0x2.0p+0][0x1.1p+0][0x1p+1][0x2p+1][0x2p+0]"));
        fmt(b"[%a][%a][%.3a][%.20a][%010a][%-12a][%+a][% a][%12.3A][%a][%.2a][%.0a]", &[N(2.2250738585072014e-308), N(2.225073858507201e-308), N(2.225073858507201e-308), N(1e0), N(1e0), N(1e0), N(1e0), N(1e0), N(1e-1), N(1e300), N(1.5e-323), N(2.225073858507201e-308)], Ok(b"[0x1p-1022][0x0.fffffffffffffp-1022][0x1.000p-1022][0x1.00000000000000000000p+0][0x00001p+0][0x1p+0      ][+0x1p+0][ 0x1p+0][  0X1.99AP-4][0x1.7e43c8800759cp+996][0x0.00p-1022][0x1p-1022]"));
        fmt(b"[%.13a][%.12a][%.0a][%.0a][%.0a][%.3a][%a]", &[N(1e-1), N(1e-1), N(1.4999999999999998e0), N(1.5000000000000002e0), N(1.75e0), N(0e0), N(-1.7976931348623157e308)], Ok(b"[0x1.999999999999ap-4][0x1.99999999999ap-4][0x1p+0][0x2p+0][0x2p+0][0x0.000p+0][-0x1.fffffffffffffp+1023]"));
        fmt(b"%.40f|%.60e", &[N(5e-324), N(5e-324)], Ok(b"0.0000000000000000000000000000000000000000|4.940656458412465441765687928682213723650598026143247644255857e-324"));
        fmt(b"%.0f|%.0f", &[N(1e300), N(-1.7976931348623157e308)], Ok(b"1000000000000000052504760255204420248704468581108159154915854115511802457988908195786371375080447864043704443832883878176942523235360430575644792184786706982848387200926575803737830233794788090059368953234970799945081119038967640880074652742780142494579258788820056842838115669472196386865459400540160|-179769313486231570814527423731704356798070567525844996598917476803157260780028538760589558632766878171540458953514382464234321326889464182768467546703537516986049910576551282076245490090389328944075868508455133942304583236903222948165808559332123348274797826204144723168738177180919299881250404026184124858368"));
        // Largura e precisão vão pro `printf` do C como `int`.
        fmt(b"[%*f][%.*f][%.*e][%*x]", &[N(4.294967308e9), N(1e0), N(4.294967298e9), N(1e0), N(4.294967298e9), N(1e0), N(4.294967308e9), N(1.1805916207174113e21)], Ok(b"[    1.000000][1.00][1.00e+00][ 1.18059e+21]"));
        fmt(b"[%'f][%'d][%'g][%'.3e]", &[N(1.2345675e6), N(1.234567e6), N(1.234567e6), N(1.234567e6)], Ok(b"[1234567.500000][1234567][1.23457e+06][1.235e+06]"));
    }

    #[test]
    #[rustfmt::skip]
    fn float_buffer_overflow() {
        fmt(b"[%*d][%.*d][%*.*f][%*s]", &[N(1e30), N(1e0), N(1e30), N(2e0), N(f64::NAN), N(f64::NEG_INFINITY), N(1.5e0), N(f64::INFINITY), S(b"x")], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -9223372036854775719 bytes of memory: Cannot allocate memory"));
        fmt(b"%-#*E|", &[N(1e30), N(-4.997e-5)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -9223372036854775726 bytes of memory: Cannot allocate memory"));
        fmt(b"<%*f|", &[N(f64::NAN), N(-1.4757395258967641e20)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -9223372036854775725 bytes of memory: Cannot allocate memory"));
        fmt(b"%' *tx|", &[N(f64::NAN), N(-4.722366482869645e21)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -9223372036854775726 bytes of memory: Cannot allocate memory"));
        fmt(b"%*a", &[N(1e30), N(1e0)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -9223372036854775726 bytes of memory: Cannot allocate memory"));
        fmt(b"%*.3f", &[N(1e30), N(1e0)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -9223372036854775729 bytes of memory: Cannot allocate memory"));
        fmt(b"%*.*f", &[N(1e30), N(4.294967296e9), N(1e0)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -9223372032559808436 bytes of memory: Cannot allocate memory"));
        fmt(b"%*.*f", &[N(1e30), N(-5e0), N(1e0)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -9223372036854775726 bytes of memory: Cannot allocate memory"));
        fmt(b"%*Pd", &[N(1e30), N(-f64::NAN)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -9223372036854775726 bytes of memory: Cannot allocate memory"));
        fmt(b"%9223372036854775808f", &[N(1e0)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -9223372036854775726 bytes of memory: Cannot allocate memory"));
        fmt(b"%9999999999999999999e", &[N(1e0)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -8446744073709551535 bytes of memory: Cannot allocate memory"));
        fmt(b"%s%*f", &[S(b"0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789"), N(1e30), N(1e0)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -9223372036854775562 bytes of memory: Cannot allocate memory"));
        fmt(b"%.0f%*f", &[N(2.037035976334486e90), N(1e30), N(1e0)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -9223372036854775543 bytes of memory: Cannot allocate memory"));
        fmt(b"%70f%*f", &[N(1e0), N(1e30), N(1e0)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -9223372036854775568 bytes of memory: Cannot allocate memory"));
        fmt(b"%*s|%*f", &[N(1e30), S(b"a"), N(1e30), N(1e0)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -9223372036854775724 bytes of memory: Cannot allocate memory"));
        fmt(b"%*E", &[N(4.611686018427388e18), N(1e0)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate 4611686018427387986 bytes of memory: Cannot allocate memory"));
        // `.-` com `-` ativo: a precisão -1 entra no `chksize`.
        fmt(b"<%0'0*.-0e|", &[N(-f64::NAN), N(4.73e2)], Err("builtin.c:1607:format_tree: obuf: cannot reallocate -9223372036854775732 bytes of memory: Cannot allocate memory"));
        fmt(b"%*d|%*x|%*s|", &[N(1e30), N(1e0), N(1e30), N(1e0), N(1e30), S(b"a")], Ok(b"1|1|a|"));
    }

    #[test]
    #[rustfmt::skip]
    fn number_to_string() {
        conv(b"%.6g", 3.75e0, Ok(b"3.75"));
        conv(b"%.6g", -0.0, Ok(b"0"));
        conv(b"%.6g", 1e30, Ok(b"1000000000000000019884624838656"));
        conv(b"%.6g", -9.223372036854776e18, Ok(b"-9223372036854775808"));
        conv(b"%.6g", 1.23456789012e11, Ok(b"123456789012"));
        conv(b"%.6g", 1e-1, Ok(b"0.1"));
        conv(b"%.6g", f64::NAN, Ok(b"+nan"));
        conv(b"%.6g", -f64::NAN, Ok(b"-nan"));
        conv(b"%.6g", f64::INFINITY, Ok(b"+inf"));
        conv(b"%.6g", f64::NEG_INFINITY, Ok(b"-inf"));
        conv(b"%d", 3.75e0, Ok(b"3"));
        conv(b"%x", -2.5e0, Ok(b"fffffffffffffffe"));
        conv(b"abc", 3.75e0, Ok(b"abc"));
        conv(b"%c", 6.55e1, Ok(b"A"));
        conv(b"%5.2f|", 3.14159e0, Ok(b" 3.14|"));
        conv(b"%.30g", 1e-1, Ok(b"0.100000000000000005551115123126"));
        conv(b"%%", 5e-1, Ok(b"%"));
        conv(b"%a", 3.75e0, Ok(b"0x1.ep+1"));
        conv(b"%d %d", 3.75e0, Err("not enough arguments to satisfy format string\n\t`%d %d'\n\t    ^ ran out for this one"));
        conv(b"%*d", 3.75e0, Err("not enough arguments to satisfy format string\n\t`%*d'\n\t  ^ ran out for this one"));
        conv(b"%2$s", 3.75e0, Err("argument index 2 greater than total number of supplied arguments"));
        conv(b"%1$.3f", 3.75e0, Ok(b"3.750"));
        conv(b"%.2f", 2.5e0, Ok(b"2.50"));
        conv(b"%.2f", 7e0, Ok(b"7"));
        conv(b"%.3e", 1e100, Ok(b"10000000000000000159028911097599180468360808563945281389781327557747838772170381060813469985856815104"));
        // A versão sem erro cai no `%.6g` quando o formato não pode ser satisfeito.
        assert_eq!(num_to_str(3.75, b"%d %d"), b"3.75");
        assert_eq!(num_to_str(42.0, b"%d %d"), b"42");
        assert_eq!(num_to_str(-17.0, b"%.6g"), b"-17");
    }
}
