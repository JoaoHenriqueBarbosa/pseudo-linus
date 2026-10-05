//! `tput`, `clear`, `tset`/`reset`, `tabs`, `infocmp` e `toe` do ncurses 6.5.20250216 (ncurses-bin do
//! Debian 13), sobre o banco terminfo compilado do ncurses-base (`/usr/share/terminfo`), lido no
//! formato de 16 ou 32 bits com os nomes estendidos.
//!
//! Módulos:
//!
//! - [`caps_table`]: as capacidades predefinidas (gerado do `include/Caps` por `gen_caps.py`);
//! - [`terminfo`]: o leitor do formato compilado, a busca nos diretórios do banco e o `setupterm`;
//! - [`tparm`]: o interpretador de `tparm` e o `tputs` com o preenchimento (padding);
//! - [`tput`], [`clear`], [`tset`], [`tabs`], [`infocmp`], [`toe`]: os programas;
//! - [`expand`], [`infotocap`], [`dump`]: a formatação de cadeias e a saída do `infocmp`.

pub mod caps_table;
pub mod clear;
pub mod dump;
pub mod expand;
pub mod infocmp;
pub mod infotocap;
pub mod reset;
pub mod terminfo;
pub mod toe;
pub mod tparm;
pub mod tput;
pub mod tabs;
pub mod tset;

/// O que `curses_version()` devolve nesta versão.
pub const VERSION: &str = "ncurses 6.5.20250216";

/// Quantas capacidades predefinidas de cada tipo (`BOOLCOUNT`, `NUMCOUNT`, `STRCOUNT` do `term.h`).
pub const BOOLCOUNT: usize = 44;
pub const NUMCOUNT: usize = 39;
pub const STRCOUNT: usize = 414;
/// `BOOLWRITE`, `NUMWRITE`, `STRWRITE`: o que o formato compilado carrega.
pub const BOOLWRITE: usize = 37;
pub const NUMWRITE: usize = 33;
pub const STRWRITE: usize = 394;

/// Valores especiais das capacidades.
pub const ABSENT_NUMERIC: i32 = -1;
pub const CANCELLED_NUMERIC: i32 = -2;
pub const ABSENT_BOOLEAN: i8 = -1;
pub const CANCELLED_BOOLEAN: i8 = -2;

/// Tipo de uma capacidade.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Num,
    Str,
}

/// Uma linha da tabela `Caps`: o nome da variável C, o código terminfo, o código termcap e o que o
/// `infocmp` precisa saber pra traduzir.
#[derive(Copy, Clone, Debug)]
pub struct Cap {
    pub var: &'static str,
    pub info: &'static str,
    pub tc: &'static str,
    pub kind: Kind,
    /// A coluna 7 da tabela começa com `Y`: a capacidade vai pra um termcap em formato BSD.
    pub from_tc: bool,
    /// Só cadeias: `parametrized[]` (-1 sem tradução de `%` nem de padding, 0 só padding, 1 os dois).
    pub param: i8,
}

pub use caps_table::{BOOLS, NUMS, STRS};

/// Posição de uma capacidade predefinida pelo código terminfo e tipo (`_nc_find_type_entry`): em
/// nomes repetidos vale a última linha da tabela, como na tabela hash do ncurses.
pub fn find_type_entry(name: &[u8], kind: Kind) -> Option<usize> {
    let table: &[Cap] = match kind {
        Kind::Bool => &BOOLS,
        Kind::Num => &NUMS,
        Kind::Str => &STRS,
    };
    table.iter().rposition(|c| c.info.as_bytes() == name)
}

/// Índice de uma cadeia predefinida pelo nome da variável C (`clear_screen`...).
pub fn str_index(var: &str) -> usize {
    STRS.iter().position(|c| c.var == var).unwrap_or(usize::MAX)
}

/// Índice de um número predefinido pelo nome da variável C.
pub fn num_index(var: &str) -> usize {
    NUMS.iter().position(|c| c.var == var).unwrap_or(usize::MAX)
}

/// Índice de um booleano predefinido pelo nome da variável C.
pub fn bool_index(var: &str) -> usize {
    BOOLS.iter().position(|c| c.var == var).unwrap_or(usize::MAX)
}

/// `isspace` do locale C.
pub fn c_isspace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `isprint` do locale C (ASCII).
pub fn c_isprint(b: u8) -> bool {
    (0x20..0x7f).contains(&b)
}

/// `strtol(s, &end, 0)` da glibc: devolve o valor e quantos bytes consumiu (0 quando não há dígito,
/// como `end == s`). Estouro satura em `LONG_MAX`/`LONG_MIN`.
pub fn strtol(s: &[u8]) -> (i64, usize) {
    let mut i = 0;
    while i < s.len() && c_isspace(s[i]) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut base = 10u32;
    if i + 2 < s.len() && s[i] == b'0' && (s[i + 1] == b'x' || s[i + 1] == b'X') && s[i + 2].is_ascii_hexdigit() {
        base = 16;
        i += 2;
    } else if i < s.len() && s[i] == b'0' {
        base = 8;
    }
    let digits_start = i;
    let mut acc: u128 = 0;
    let mut overflow = false;
    while i < s.len() {
        let d = match (s[i] as char).to_digit(base) {
            Some(d) => d,
            None => break,
        };
        if !overflow {
            acc = acc * u128::from(base) + u128::from(d);
            if acc > u128::from(u64::MAX) {
                overflow = true;
            }
        }
        i += 1;
    }
    if i == digits_start {
        return (0, 0);
    }
    let value = if neg {
        if overflow || acc > (i64::MAX as u128) + 1 { i64::MIN } else { (acc as i128).wrapping_neg() as i64 }
    } else if overflow || acc > i64::MAX as u128 {
        i64::MAX
    } else {
        acc as i64
    };
    (value, i)
}

/// O primeiro nome de uma lista `a|b|c` (`_nc_first_name`), no máximo `MAX_NAME_SIZE` bytes.
pub fn first_name(names: &[u8]) -> &[u8] {
    let end = names.iter().take(512).position(|b| *b == b'|').unwrap_or(names.len().min(512));
    &names[..end]
}

/// O nome base de um caminho (`_nc_rootname`; sem a conversão de caixa, que só vale em sistemas de
/// arquivos sem maiúsculas).
pub fn rootname(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|b| *b == b'/') {
        Some(p) => &path[p + 1..],
        None => path,
    }
}

/// Saídas do `tputs` sobre o stdout do processo (o `putchar`).
#[derive(Debug, Default)]
pub struct StdoutSink;

impl tparm::Sink for StdoutSink {
    fn put(&mut self, c: u8) {
        use std::io::Write;
        let _ = crate::util::io::stdout().write_all(&[c]);
    }

    fn flush(&mut self) {
        let _ = crate::util::io::flush_stdout();
    }
}

/// Saída acumulada que vai pra um fd de uma vez (o `FILE *` do `reset_cmd.c`, que é o stdout no
/// `tput` e o stderr no `tset`).
#[derive(Debug)]
pub struct FdSink {
    fd: sysabi::Fd,
    buf: Vec<u8>,
}

impl FdSink {
    pub fn new(fd: sysabi::Fd) -> FdSink {
        FdSink { fd, buf: Vec::new() }
    }

    pub fn write_all(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    pub fn finish(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        if self.fd == sysabi::Fd::STDOUT {
            use std::io::Write;
            let _ = crate::util::io::stdout().write_all(&self.buf);
        } else {
            let _ = sysabi::sys::write_all(self.fd, &self.buf);
        }
        self.buf.clear();
    }
}

impl tparm::Sink for FdSink {
    fn put(&mut self, c: u8) {
        self.buf.push(c);
    }

    fn flush(&mut self) {
        self.finish();
        if self.fd == sysabi::Fd::STDOUT {
            let _ = crate::util::io::flush_stdout();
        }
    }
}

/// `ErrSystem(n)` do `progs.priv.h`.
pub fn err_system(n: i32) -> i32 {
    4 + n
}

/// `save_tty_settings` (`tty_settings.c`): o primeiro dos fds 2, 1, 0 que é um terminal. Sem
/// nenhum, com `need_tty` tenta `/dev/tty` e morre com `terminal attributes: <erro>`. Os modos do
/// terminal (termios) não passam pelo `sysabi`, então só o fd é devolvido.
pub fn save_tty_settings(progname: &str, need_tty: bool) -> sysabi::Fd {
    use sysabi::{Errno, Fd, OFlags, sys};
    for fd in [Fd::STDERR, Fd::STDOUT, Fd::STDIN] {
        if terminfo::isatty(fd) {
            return fd;
        }
    }
    if need_tty {
        let errno = match sys::open(b"/dev/tty", OFlags::RDWR, 0) {
            Ok(fd) => {
                if terminfo::isatty(fd) {
                    return fd;
                }
                let _ = sys::close(fd);
                Errno(25)
            }
            Err(e) => e,
        };
        crate::util::io::eprint(format!("{progname}: terminal attributes: {}\n", errno.message()));
        crate::util::io::eprint("\n");
        sys::exit(err_system(errno.0));
    }
    Fd::STDOUT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_match_tables() {
        assert_eq!(BOOLS.len(), BOOLCOUNT);
        assert_eq!(NUMS.len(), NUMCOUNT);
        assert_eq!(STRS.len(), STRCOUNT);
        assert_eq!(NUMS[0].var, "columns");
        assert_eq!(NUMS[2].var, "lines");
    }

    #[test]
    fn strtol_base_zero() {
        assert_eq!(strtol(b"12"), (12, 2));
        assert_eq!(strtol(b"0x10"), (16, 4));
        assert_eq!(strtol(b"010"), (8, 3));
        assert_eq!(strtol(b"08"), (0, 1));
        assert_eq!(strtol(b"0xg"), (0, 1));
        assert_eq!(strtol(b"abc"), (0, 0));
        assert_eq!(strtol(b"  -5x"), (-5, 4));
        assert_eq!(strtol(b""), (0, 0));
    }
}
