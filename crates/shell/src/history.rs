//! A lista de histórico do shell interativo (`lib/readline/history.c` e `bashhist.c` do bash 5.2):
//! carregada do `HISTFILE` na partida, filtrada pelo `HISTCONTROL` e pelo `HISTIGNORE` ausente,
//! limitada pelo `HISTSIZE`, gravada de volta no `HISTFILE` na saída (cortada pelo `HISTFILESIZE`),
//! e o builtin `history`.

use sysabi::{Fd, OFlags};

use crate::shell::{Exec, Shell, sys, write_fd};

/// `HISTSIZE` e `HISTFILESIZE` sem valor: o padrão do bash (`HISTSIZE_DEFAULT`).
const DEFAULT_SIZE: usize = 500;

#[derive(Clone, Default)]
pub struct History {
    pub entries: Vec<Vec<u8>>,
    /// Número do primeiro item da lista (`history_base`): cresce quando o `HISTSIZE` corta o começo.
    pub base: usize,
    /// O `HISTFILE` já foi lido nesta sessão.
    loaded: bool,
}

/// Valor numérico de uma variável de tamanho: vazia ou não numérica é o padrão, negativa é sem
/// limite (`None`), como no `sv_histsize` do bash.
fn size_var(sh: &Shell, name: &str) -> Option<usize> {
    match sh.var_bytes(name).and_then(|v| std::str::from_utf8(v).ok()).map(str::trim) {
        None => Some(DEFAULT_SIZE),
        Some(v) => match v.parse::<i64>() {
            Ok(n) if n < 0 => None,
            Ok(n) => Some(n as usize),
            Err(_) => Some(DEFAULT_SIZE),
        },
    }
}

fn histfile(sh: &Shell) -> Option<Vec<u8>> {
    sh.var_bytes("HISTFILE").filter(|v| !v.is_empty()).map(<[u8]>::to_vec)
}

impl History {
    fn trim(&mut self, limit: Option<usize>) {
        if let Some(limit) = limit
            && self.entries.len() > limit
        {
            let cut = self.entries.len() - limit;
            self.entries.drain(..cut);
            self.base += cut;
        }
    }
}

/// O `HISTFILE` padrão do shell interativo (`~/.bash_history`), quando a variável não existe.
pub fn init(sh: &mut Shell) {
    if sh.var_bytes("HISTFILE").is_none()
        && let Some(home) = sh.var_bytes("HOME").map(<[u8]>::to_vec)
    {
        let _ = sh.assign_scalar("HISTFILE", [home.as_slice(), b"/.bash_history"].concat(), false);
    }
    if sh.var_bytes("HISTSIZE").is_none() {
        let _ = sh.assign_scalar("HISTSIZE", DEFAULT_SIZE.to_string().into_bytes(), false);
    }
    if sh.var_bytes("HISTFILESIZE").is_none() {
        let _ = sh.assign_scalar("HISTFILESIZE", DEFAULT_SIZE.to_string().into_bytes(), false);
    }
}

/// Lê o `HISTFILE` (depois dos arquivos de partida, que podem mudar `HISTFILE` e `HISTSIZE`).
/// Linhas que começam com `#` seguido de dígito são as marcas de tempo do `HISTTIMEFORMAT`.
pub fn load(sh: &mut Shell) {
    if sh.history.loaded {
        return;
    }
    sh.history.loaded = true;
    let Some(path) = histfile(sh) else { return };
    let Ok(data) = sysabi::sys::read_file(&path) else { return };
    let mut entries: Vec<Vec<u8>> = data
        .split(|c| *c == b'\n')
        .filter(|l| !l.is_empty() && !(l[0] == b'#' && l.get(1).is_some_and(u8::is_ascii_digit)))
        .map(<[u8]>::to_vec)
        .collect();
    if let Some(limit) = size_var(sh, "HISTFILESIZE")
        && entries.len() > limit
    {
        entries.drain(..entries.len() - limit);
    }
    sh.history.entries = entries;
    sh.history.base = 1;
    let limit = size_var(sh, "HISTSIZE");
    sh.history.trim(limit);
}

/// Acrescenta um comando digitado, seguindo o `HISTCONTROL` (`ignorespace`, `ignoredups`,
/// `ignoreboth`, `erasedups`).
pub fn add(sh: &mut Shell, line: &[u8]) {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    if line.iter().all(u8::is_ascii_whitespace) {
        return;
    }
    if sh.history.base == 0 {
        sh.history.base = 1;
    }
    let control = sh.var_bytes("HISTCONTROL").unwrap_or_default().to_vec();
    let has = |w: &[u8]| control.split(|c| *c == b':').any(|x| x == w);
    let ignorespace = has(b"ignorespace") || has(b"ignoreboth");
    let ignoredups = has(b"ignoredups") || has(b"ignoreboth");
    if ignorespace && line[0] == b' ' {
        return;
    }
    if ignoredups && sh.history.entries.last().is_some_and(|l| l == line) {
        return;
    }
    if has(b"erasedups") {
        sh.history.entries.retain(|l| l != line);
    }
    sh.history.entries.push(line.to_vec());
    let limit = size_var(sh, "HISTSIZE");
    sh.history.trim(limit);
}

/// Grava a lista no `HISTFILE` ao sair do shell interativo, com no máximo `HISTFILESIZE` linhas.
pub fn save(sh: &mut Shell) {
    if !sh.history.loaded {
        return;
    }
    let Some(path) = histfile(sh) else { return };
    let mut entries: &[Vec<u8>] = &sh.history.entries;
    if let Some(limit) = size_var(sh, "HISTFILESIZE")
        && entries.len() > limit
    {
        entries = &entries[entries.len() - limit..];
    }
    let mut data = Vec::new();
    for e in entries {
        data.extend_from_slice(e);
        data.push(b'\n');
    }
    let Ok(fd) = sysabi::sys::open(&path, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC, 0o600) else { return };
    let _ = write_fd(fd, &data);
    let _ = sys().close(fd);
}

/// O builtin `history`: sem argumentos lista tudo, com `N` os últimos N, `-c` limpa, `-d N` apaga
/// um item, `-w`/`-a` gravam e `-r` lê o `HISTFILE`.
pub fn builtin(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let mut args = argv[1..].iter().peekable();
    let mut count: Option<usize> = None;
    while let Some(a) = args.next() {
        match a.as_slice() {
            b"--" => {}
            b"-c" => {
                sh.history.entries.clear();
                return Ok(0);
            }
            b"-w" | b"-a" => {
                save(sh);
                return Ok(0);
            }
            b"-r" => {
                sh.history.loaded = false;
                load(sh);
                return Ok(0);
            }
            b"-d" => {
                let Some(n) = args.next() else {
                    let _ = write_fd(Fd::STDERR, b"bash: history: -d: option requires an argument\nhistory: usage: history [-c] [-d offset] [n] or history -anrw [filename] or history -ps arg [arg...]\n");
                    return Ok(2);
                };
                let shown = String::from_utf8_lossy(n).into_owned();
                let Some(idx) = shown.parse::<usize>().ok().and_then(|n| n.checked_sub(sh.history.base)).filter(|i| *i < sh.history.entries.len()) else {
                    let _ = write_fd(Fd::STDERR, format!("bash: history: {shown}: history position out of range\n").as_bytes());
                    return Ok(1);
                };
                sh.history.entries.remove(idx);
                return Ok(0);
            }
            n if n.first().is_some_and(u8::is_ascii_digit) => {
                let shown = String::from_utf8_lossy(n).into_owned();
                match shown.parse() {
                    Ok(v) => count = Some(v),
                    Err(_) => {
                        let _ = write_fd(Fd::STDERR, format!("bash: history: {shown}: numeric argument required\n").as_bytes());
                        return Ok(1);
                    }
                }
            }
            other => {
                let shown = String::from_utf8_lossy(other).into_owned();
                let _ = write_fd(
                    Fd::STDERR,
                    format!("bash: history: {shown}: invalid option\nhistory: usage: history [-c] [-d offset] [n] or history -anrw [filename] or history -ps arg [arg...]\n").as_bytes(),
                );
                return Ok(2);
            }
        }
    }
    let total = sh.history.entries.len();
    let skip = count.map_or(0, |n| total.saturating_sub(n));
    let mut out = Vec::new();
    for (i, e) in sh.history.entries.iter().enumerate().skip(skip) {
        out.extend(format!("{:5}  ", sh.history.base.max(1) + i).as_bytes());
        out.extend_from_slice(e);
        out.push(b'\n');
    }
    let _ = write_fd(Fd::STDOUT, &out);
    Ok(0)
}
