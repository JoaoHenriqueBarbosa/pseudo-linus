//! Peças comuns dos programas do crate: nomes de usuário e grupo, nome do terminal a partir do
//! número do dispositivo, tabela de sinais do procps, contagem de usuários do utmp e conversões de
//! número no estilo `strtol`.

use std::collections::HashMap;

use sysabi::{Fd, FileType, Pid, sys};
use sysio::users::{self, Group, Passwd};

/// `prog: msg` no stderr, numa escrita só.
pub fn warn(prog: &str, msg: &str) {
    ul_misc::util::io::eprint(format!("{prog}: {msg}\n"));
}

/// `/etc/passwd` e `/etc/group` lidos uma vez por execução (como o cache do NSS "files" num
/// processo só). Nada é guardado entre processos.
#[derive(Default)]
pub struct Names {
    passwd: Option<Vec<Passwd>>,
    groups: Option<Vec<Group>>,
    by_uid: HashMap<u32, Option<String>>,
    by_gid: HashMap<u32, Option<String>>,
}

impl Names {
    pub fn new() -> Names {
        Names::default()
    }

    fn passwd(&mut self) -> &[Passwd] {
        self.passwd.get_or_insert_with(users::all_passwd)
    }

    fn groups(&mut self) -> &[Group] {
        self.groups.get_or_insert_with(users::all_groups)
    }

    /// Nome do usuário, se existe.
    pub fn user(&mut self, uid: u32) -> Option<String> {
        if let Some(v) = self.by_uid.get(&uid) {
            return v.clone();
        }
        let v = self.passwd().iter().find(|p| p.uid == uid).map(|p| p.name.clone());
        self.by_uid.insert(uid, v.clone());
        v
    }

    /// Nome do usuário, ou o número quando não existe.
    pub fn user_or_id(&mut self, uid: u32) -> String {
        self.user(uid).unwrap_or_else(|| uid.to_string())
    }

    pub fn group(&mut self, gid: u32) -> Option<String> {
        if let Some(v) = self.by_gid.get(&gid) {
            return v.clone();
        }
        let v = self.groups().iter().find(|g| g.gid == gid).map(|g| g.name.clone());
        self.by_gid.insert(gid, v.clone());
        v
    }

    pub fn group_or_id(&mut self, gid: u32) -> String {
        self.group(gid).unwrap_or_else(|| gid.to_string())
    }

    pub fn uid_of(&mut self, name: &str) -> Option<u32> {
        self.passwd().iter().find(|p| p.name == name).map(|p| p.uid)
    }

    pub fn gid_of(&mut self, name: &str) -> Option<u32> {
        self.groups().iter().find(|g| g.name == name).map(|g| g.gid)
    }
}

/// Maior e menor de um `dev_t` (codificação do glibc).
pub fn dev_major(dev: u64) -> u64 {
    ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff)
}

pub fn dev_minor(dev: u64) -> u64 {
    (dev & 0xff) | ((dev >> 12) & !0xff)
}

/// `dev_t` a partir de maior e menor (`makedev` do glibc).
pub fn makedev(major: u64, minor: u64) -> u64 {
    ((major & 0xfff) << 8) | ((major & !0xfff) << 32) | (minor & 0xff) | ((minor & !0xff) << 12)
}

/// O `tty_nr` do `/proc/<pid>/stat` como `dev_t`.
pub fn tty_nr_dev(tty_nr: i32) -> u64 {
    let t = tty_nr as u32 as u64;
    makedev((t >> 8) & 0xfff, (t & 0xff) | ((t >> 12) & 0xfff00))
}

fn is_char_dev_with(path: &str, dev: u64) -> bool {
    match sys::stat(path.as_bytes()) {
        Ok(st) => st.file_type() == FileType::CharDevice && st.rdev == dev,
        Err(_) => false,
    }
}

/// Nome do terminal (`pts/0`, `tty1`) de um processo, como o procps resolve: a tabela de
/// `/proc/tty/drivers` dá o diretório do driver e o nó em `/dev` tem que existir com o mesmo número;
/// sem isso, os links `/proc/<pid>/fd/2` e `fd/255`. `None` quando não há terminal ou ele não tem
/// nó em `/dev` (o ps mostra `?`).
pub fn tty_name(tty_nr: i32, pid: Pid) -> Option<String> {
    if tty_nr == 0 {
        return None;
    }
    let dev = tty_nr_dev(tty_nr);
    let (major, minor) = (dev_major(dev), dev_minor(dev));
    let mut candidates: Vec<String> = Vec::new();
    if let Some(data) = crate::procfs::read("/proc/tty/drivers") {
        for line in String::from_utf8_lossy(&data).lines() {
            let f: Vec<&str> = line.split_ascii_whitespace().collect();
            if f.len() < 4 {
                continue;
            }
            if f[2].parse::<u64>().ok() != Some(major) {
                continue;
            }
            let (lo, hi) = match f[3].split_once('-') {
                Some((a, b)) => (a.parse::<u64>().unwrap_or(0), b.parse::<u64>().unwrap_or(0)),
                None => {
                    let v = f[3].parse::<u64>().unwrap_or(u64::MAX);
                    (v, v)
                }
            };
            if minor < lo || minor > hi {
                continue;
            }
            candidates.push(format!("{}{minor}", f[1]));
            candidates.push(format!("{}/{minor}", f[1]));
        }
    } else {
        match major {
            136..=143 => candidates.push(format!("/dev/pts/{}", minor + (major - 136) * 256)),
            4 if minor < 64 => candidates.push(format!("/dev/tty{minor}")),
            4 => candidates.push(format!("/dev/ttyS{}", minor - 64)),
            5 if minor == 1 => candidates.push("/dev/console".to_string()),
            _ => {}
        }
    }
    for c in &candidates {
        if is_char_dev_with(c, dev) {
            return Some(c.trim_start_matches("/dev/").to_string());
        }
    }
    let sysc = sys::current();
    for fd in [2, 255] {
        if let Ok(target) = sysc.readlinkat(Fd::CWD, format!("/proc/{pid}/fd/{fd}").as_bytes()) {
            let t = String::from_utf8_lossy(&target).into_owned();
            if t.starts_with("/dev/") && is_char_dev_with(&t, dev) {
                return Some(t.trim_start_matches("/dev/").to_string());
            }
        }
    }
    None
}

/// Número do dispositivo de um terminal dado pelo nome (`pts/0`, `/dev/pts/0`, `tty1`), como o
/// `-t` do ps e do pgrep entendem: o nó tem que existir em `/dev`.
pub fn tty_dev_by_name(name: &str) -> Option<u64> {
    let mut tries = Vec::new();
    if name.starts_with('/') {
        tries.push(name.to_string());
    } else {
        tries.push(format!("/dev/{name}"));
        tries.push(format!("/dev/tty{name}"));
        tries.push(format!("/dev/pts/{name}"));
    }
    for t in tries {
        if let Ok(st) = sys::stat(t.as_bytes())
            && st.file_type() == FileType::CharDevice {
                return Some(st.rdev);
            }
    }
    None
}

/// Nomes dos sinais 1 a 31 na tabela do procps (o 29 é `POLL`, não `IO`).
pub const SIGNAL_NAMES: [&str; 31] = [
    "HUP", "INT", "QUIT", "ILL", "TRAP", "ABRT", "BUS", "FPE", "KILL", "USR1", "SEGV", "USR2", "PIPE", "ALRM",
    "TERM", "STKFLT", "CHLD", "CONT", "STOP", "TSTP", "TTIN", "TTOU", "URG", "XCPU", "XFSZ", "VTALRM", "PROF",
    "WINCH", "POLL", "PWR", "SYS",
];

/// Apelidos que a tabela do procps também aceita como nome de sinal.
const SIGNAL_ALIASES: [(&str, i32); 3] = [("IO", 29), ("IOT", 6), ("CLD", 17)];

/// Nome de sinal sem o prefixo `SIG`, maiúsculo, pra um número de 1 a 31.
pub fn signal_name(n: i32) -> Option<&'static str> {
    usize::try_from(n).ok().and_then(|i| i.checked_sub(1)).and_then(|i| SIGNAL_NAMES.get(i)).copied()
}

/// Nome (com ou sem `SIG`, qualquer caixa) pro número, só com a tabela de 1 a 31 e os apelidos.
pub fn signal_by_table_name(s: &str) -> Option<i32> {
    let up = s.to_ascii_uppercase();
    let bare = up.strip_prefix("SIG").unwrap_or(&up);
    if let Some(i) = SIGNAL_NAMES.iter().position(|n| *n == bare) {
        return Some(i as i32 + 1);
    }
    SIGNAL_ALIASES.iter().find(|(n, _)| *n == bare).map(|(_, v)| *v)
}

/// Sinal de tempo real `RTMIN`, `RTMIN+n`, `RTMAX`, `RTMAX-n` (com ou sem `SIG`).
pub fn signal_rt(s: &str) -> Option<i32> {
    let up = s.to_ascii_uppercase();
    let bare = up.strip_prefix("SIG").unwrap_or(&up);
    let (base, rest) = if let Some(r) = bare.strip_prefix("RTMIN") {
        (sysabi::linux::SIGRTMIN, r)
    } else {
        let r = bare.strip_prefix("RTMAX")?;
        (sysabi::linux::SIGRTMAX, r)
    };
    if rest.is_empty() {
        return Some(base);
    }
    let off: i32 = rest.parse().ok()?;
    let v = base + off;
    (sysabi::linux::SIGRTMIN..=sysabi::linux::SIGRTMAX).contains(&v).then_some(v)
}

/// Resultado de [`strtol`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strtol {
    Ok(i64),
    /// Estourou (o strtol satura e marca ERANGE).
    Range(i64),
    /// Não é número inteiro do começo ao fim.
    Invalid,
}

/// `strtol(s, &end, 10)` exigindo que tudo seja consumido (espaço à esquerda e sinal aceitos).
pub fn strtol(s: &str) -> Strtol {
    let t = s.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    if t.is_empty() {
        return Strtol::Invalid;
    }
    let (neg, digits) = match t.as_bytes()[0] {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Strtol::Invalid;
    }
    // Acumula em negativo pra caber o LONG_MIN.
    let mut v: i64 = 0;
    for b in digits.bytes() {
        match v.checked_mul(10).and_then(|x| x.checked_sub(i64::from(b - b'0'))) {
            Some(x) => v = x,
            None => return Strtol::Range(if neg { i64::MIN } else { i64::MAX }),
        }
    }
    if neg {
        Strtol::Ok(v)
    } else {
        match v.checked_neg() {
            Some(x) => Strtol::Ok(x),
            None => Strtol::Range(i64::MAX),
        }
    }
}

/// [`strtol`] que aceita o valor saturado do estouro, como quem não olha o errno.
pub fn parse_long(s: &str) -> Option<i64> {
    match strtol(s) {
        Strtol::Ok(v) | Strtol::Range(v) => Some(v),
        Strtol::Invalid => None,
    }
}

/// Usuários logados segundo o utmp (`/var/run/utmp`): registros `USER_PROCESS` com nome. Sem o
/// arquivo, zero (é o que o procps mostra num container).
pub fn utmp_users() -> usize {
    const RECORD: usize = 384;
    const USER_PROCESS: i16 = 7;
    let Some(data) = sys::read_file(b"/var/run/utmp").ok() else { return 0 };
    data.as_chunks::<RECORD>().0.iter()
        .filter(|r| i16::from_le_bytes([r[0], r[1]]) == USER_PROCESS && r[44] != 0)
        .count()
}

/// Escreve no stdout com o buffer da glibc (via `sysio`); erro de escrita fica pro fim do `run`.
pub fn out(s: impl AsRef<[u8]>) {
    use std::io::Write;
    let _ = ul_misc::util::io::stdout().write_all(s.as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_numbers() {
        assert_eq!(tty_nr_dev(34816), makedev(136, 0));
        assert_eq!(dev_major(makedev(136, 5)), 136);
        assert_eq!(dev_minor(makedev(136, 300)), 300);
    }

    #[test]
    fn signal_names() {
        assert_eq!(signal_by_table_name("sigkill"), Some(9));
        assert_eq!(signal_by_table_name("POLL"), Some(29));
        assert_eq!(signal_by_table_name("IO"), Some(29));
        assert_eq!(signal_name(29), Some("POLL"));
        assert_eq!(signal_rt("RTMIN+2"), Some(36));
        assert_eq!(signal_rt("rtmax-1"), Some(63));
        assert_eq!(parse_long(" -12"), Some(-12));
        assert_eq!(parse_long("+5"), Some(5));
        assert_eq!(parse_long("5x"), None);
    }
}
