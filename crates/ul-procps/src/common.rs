//! Peças comuns dos programas do crate: nomes de usuário e grupo, nome do terminal a partir do
//! número do dispositivo, tabela de sinais do procps, contagem de usuários do utmp e conversões de
//! número no estilo `strtol`.

use std::collections::HashMap;

use sysabi::{Fd, FileType, Pid, sys};
use sysio::users::{self, Group, Passwd};
use ul_common::ctype::strtol_whole;
use ul_common::signal;

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

/// Como o procps lê o nome de um sinal: qualquer caixa, `SIG` opcional, o 29 é `POLL` e `IO`, `IOT`
/// e `CLD` também valem.
pub const SIGNALS: signal::Table =
    signal::Table { sig29: signal::Sig29::Poll, case: signal::Case::Any, aliases: &[("IO", 29), ("IOT", 6), ("CLD", 17)] };

/// Nome de sinal do procps (tabela de 1 a 31 com os apelidos, ou tempo real em qualquer caixa).
pub fn signal_by_name(s: &str) -> Option<i32> {
    signal::parse_name(s.as_bytes(), &SIGNALS).or_else(|| signal::parse_realtime(s.as_bytes(), signal::Case::Any))
}

/// `kill -l` sem argumento: os nomes de 1 a 16 numa linha e os de 17 a 31 na outra.
pub fn signal_list() -> String {
    let names = signal::standard_names(signal::Sig29::Poll);
    let (first, second) = names.split_at(16);
    format!("{}\n{}\n", first.join(" "), second.join(" "))
}

/// Resultado de [`strtol`].
pub use ul_common::ctype::WholeLong as Strtol;

/// `strtol(s, &end, 10)` exigindo que tudo seja consumido (espaço à esquerda e sinal aceitos).
pub fn strtol(s: &str) -> Strtol {
    strtol_whole(s.as_bytes(), 10)
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
        assert_eq!(signal_by_name("sigkill"), Some(9));
        assert_eq!(signal_by_name("POLL"), Some(29));
        assert_eq!(signal_by_name("IO"), Some(29));
        assert_eq!(signal_by_name("RTMIN+2"), Some(36));
        assert_eq!(signal_by_name("rtmax-1"), Some(63));
        assert_eq!(signal_by_name("9"), None);
        assert!(signal_list().starts_with("HUP INT QUIT ILL TRAP ABRT BUS FPE KILL USR1 SEGV USR2 PIPE ALRM TERM STKFLT\nCHLD CONT"));
        assert!(signal_list().ends_with(" WINCH POLL PWR SYS\n"));
        assert_eq!(parse_long(" -12"), Some(-12));
        assert_eq!(parse_long("+5"), Some(5));
        assert_eq!(parse_long("5x"), None);
    }
}
