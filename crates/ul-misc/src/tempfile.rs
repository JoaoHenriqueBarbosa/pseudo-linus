//! `tempfile` do debianutils 5.23 (Debian 13): cria um arquivo temporário e imprime o nome.
//!
//! O nome é `DIR/PREFIXXXXXXXSUFIXO` (seis caracteres aleatórios, como o `mkstemps`), em `$TMPDIR`, no
//! `-d`, em `/tmp`, nessa ordem: só `EEXIST` passa pro próximo, qualquer outro erro encerra com
//! `mkstemps: <erro>`. `-n` usa o nome dado (`O_EXCL`, modo `-m` sujeito à umask); sem `-n` o modo vem
//! de um `fchmod`, sem umask. Todo uso, mesmo `--help`, começa com o aviso de que o programa está
//! obsoleto.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, OFlags, sys};

use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("prefix", HasArg::Required, b'p' as i32),
    LongOpt::new("suffix", HasArg::Required, b's' as i32),
    LongOpt::new("directory", HasArg::Required, b'd' as i32),
    LongOpt::new("mode", HasArg::Required, b'm' as i32),
    LongOpt::new("name", HasArg::Required, b'n' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'v' as i32),
];

const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// `usage(status)`: ajuda no stdout com 0; a dica no stderr com qualquer outro valor.
fn usage(progname: &str, status: i32) -> i32 {
    if status != 0 {
        io::eprint(format!("Try `{progname} --help' for more information.\n"));
    } else {
        let mut out = io::stdout();
        let text = format!(
            "Usage: {progname} [OPTION]\n\nCreate a temporary file in a safe manner.\n\n\
-d, --directory=DIR  place temporary file in DIR\n\
-m, --mode=MODE      open with MODE instead of 0600\n\
-n, --name=FILE      use FILE instead of tempnam(3)\n\
-p, --prefix=STRING  set temporary file's prefix to STRING\n\
-s, --suffix=STRING  set temporary file's suffix to STRING\n\
\x20   --help           display this help and exit\n\
\x20   --version        output version information and exit\n"
        );
        let _ = out.write_all(text.as_bytes());
    }
    status
}

/// `perror` + `exit(1)`.
fn syserror(what: &str, e: Errno) -> i32 {
    let _ = io::flush_stdout();
    io::eprint(format!("{what}: {}\n", e.message()));
    1
}

/// `parsemode`: octal de 0 a 07777 (o `strtol` aceita espaço à frente e sinal); `None` se inválido.
fn parsemode(s: &[u8]) -> Option<u32> {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut neg = false;
    let mut j = i;
    if j < s.len() && (s[j] == b'+' || s[j] == b'-') {
        neg = s[j] == b'-';
        j += 1;
    }
    let digits_start = j;
    let mut value: u64 = 0;
    while j < s.len() && (b'0'..=b'7').contains(&s[j]) {
        value = value
            .saturating_mul(8)
            .saturating_add(u64::from(s[j] - b'0'));
        j += 1;
    }
    if j == digits_start {
        // sem conversão: o endptr volta ao começo, e só a cadeia vazia passa
        return if s.is_empty() { Some(0) } else { None };
    }
    if j < s.len() {
        return None;
    }
    if neg && value != 0 {
        return None;
    }
    if value > 0o7777 {
        return None;
    }
    Some(value as u32)
}

/// `mkstemps`: cria `template` (que termina em `XXXXXX` + sufixo de `suffixlen` bytes) com 0600.
fn mkstemps(template: &mut [u8], suffixlen: usize) -> Result<sysabi::Fd, Errno> {
    let n = template.len();
    let pos = n.saturating_sub(suffixlen + 6);
    for _ in 0..4096 {
        let mut rnd = [0u8; 6];
        let mut filled = 0;
        while filled < rnd.len() {
            match sys::current().getrandom(&mut rnd[filled..]) {
                Ok(0) => break,
                Ok(k) => filled += k,
                Err(Errno::EINTR) => {}
                Err(_) => break,
            }
        }
        for (k, b) in rnd.iter().enumerate() {
            template[pos + k] = ALPHABET[usize::from(*b) % ALPHABET.len()];
        }
        match sys::open(template, OFlags::RDWR | OFlags::CREAT | OFlags::EXCL, 0o600) {
            Ok(fd) => return Ok(fd),
            Err(Errno::EEXIST) => continue,
            Err(e) => return Err(e),
        }
    }
    Err(Errno::EEXIST)
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let progname = io::argv0(args);

    io::eprint("WARNING: tempfile is deprecated; consider using mktemp instead.\n");

    let mut name: Option<Vec<u8>> = None;
    let mut dir: Option<Vec<u8>> = None;
    let mut pfx: Vec<u8> = b"file".to_vec();
    let mut sfx: Option<Vec<u8>> = None;
    let mut mode: u32 = 0o600;

    let mut g = Getopt::from_env(&argv[1..], "p:s:d:m:n:", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&progname)));
                return usage(&progname, 1);
            }
        };
        let arg = o.arg.clone().unwrap_or_default();
        match o.short() {
            Some('p') => pfx = arg,
            Some('s') => sfx = Some(arg),
            Some('d') => dir = Some(arg),
            Some('m') => match parsemode(&arg) {
                Some(m) => mode = m,
                None => {
                    io::eprint(format!(
                        "Invalid mode `{}'.  Mode must be octal.\n",
                        io::lossy(&arg)
                    ));
                    return usage(&progname, 1);
                }
            },
            Some('n') => name = Some(arg),
            Some('h') => return usage(&progname, 0),
            Some('v') => {
                let mut out = io::stdout();
                let _ = out.write_all(b"tempfile 5.23.1\n");
                return 0;
            }
            _ => return usage(&progname, 1),
        }
    }

    let filename: Vec<u8>;
    let fd;
    if let Some(n) = &name {
        match sys::open(n, OFlags::RDWR | OFlags::CREAT | OFlags::EXCL, mode) {
            Ok(f) => fd = f,
            Err(e) => return syserror("open", e),
        }
        filename = n.clone();
    } else {
        // $TMPDIR, depois -d, depois P_tmpdir e por fim /tmp
        let tmpdirs: [Option<Vec<u8>>; 4] = [
            sys::getenv("TMPDIR"),
            dir.clone(),
            Some(b"/tmp".to_vec()),
            Some(b"/tmp".to_vec()),
        ];
        let mut found: Option<(sysabi::Fd, Vec<u8>)> = None;
        for tmpdir in tmpdirs.iter().flatten() {
            let mut template: Vec<u8> = tmpdir.clone();
            template.push(b'/');
            template.extend_from_slice(&pfx);
            template.extend_from_slice(b"XXXXXX");
            let sfxlen = sfx.as_ref().map_or(0, Vec::len);
            if let Some(s) = &sfx {
                template.extend_from_slice(s);
            }
            match mkstemps(&mut template, sfxlen) {
                Ok(f) => {
                    if let Err(e) = sys::current().fchmod(f, mode) {
                        return syserror("fchmod", e);
                    }
                    found = Some((f, template));
                    break;
                }
                Err(Errno::EEXIST) => continue,
                Err(e) => return syserror("mkstemps", e),
            }
        }
        match found {
            Some((f, t)) => {
                fd = f;
                filename = t;
            }
            None => {
                // O original usa um descritor não inicializado aqui; chegar a este ponto exige
                // esgotar os nomes em todos os diretórios.
                return syserror("close", Errno::EBADF);
            }
        }
    }

    if let Err(e) = sys::close(fd) {
        return syserror("close", e);
    }
    let mut out = io::stdout();
    let mut line = filename;
    line.push(b'\n');
    let _ = out.write_all(&line);
    if out.flush().is_err() {
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::parsemode;

    #[test]
    fn modes_follow_strtol_base_8() {
        assert_eq!(parsemode(b"0644"), Some(0o644));
        assert_eq!(parsemode(b"7777"), Some(0o7777));
        assert_eq!(parsemode(b"10000"), None);
        assert_eq!(parsemode(b"999"), None);
        assert_eq!(parsemode(b"0x10"), None);
        assert_eq!(parsemode(b""), Some(0));
        assert_eq!(parsemode(b"-0"), Some(0));
        assert_eq!(parsemode(b"-1"), None);
        assert_eq!(parsemode(b"+17"), Some(0o17));
        assert_eq!(parsemode(b" 7"), Some(7));
        assert_eq!(parsemode(b"7 "), None);
    }
}
