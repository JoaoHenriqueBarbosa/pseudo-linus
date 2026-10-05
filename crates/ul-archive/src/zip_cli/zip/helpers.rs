//! Funções de apoio do `main` do zip: filtros, leitura de nomes e comentários, estatísticas na
//! tela, arquivo temporário, substituição do zip, `-T`, `-o` e `-m`.

use sysabi::{AtFlags, Errno, Fd, OFlags, SetTime, TimeSpec, WaitOptions, WaitStatus, WaitTarget};
use sysabi::sys;

use super::consts::*;
use super::extra::get_ef_ut_ztime;
use super::names::{ex2in, getnam};
use super::state::{IzTimes, Plist, R, Zip};
use super::times::{dos2unixtime, unix2dostime};
use super::zipup::write_num_string;
use crate::sysutil;

/// `ReadNumString`: número com sufixo K, M, G ou T. `None` é o `(uzoff_t)-1` de erro; as
/// advertências saem pelo `warn`.
pub fn read_num_string(s: &[u8], warn: &mut dyn FnMut(&str, &[u8])) -> Option<u64> {
    if s.is_empty() || !s[0].is_ascii_digit() {
        warn("Unable to read number (must start with digit): ", s);
        return None;
    }
    if s.len() > 8 {
        warn("Number too long to read (8 characters max): ", s);
        return None;
    }
    let mut i = 0usize;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let num: u64 = std::str::from_utf8(&s[..i]).ok().and_then(|t| t.parse().ok()).unwrap_or(0);
    if i == s.len() {
        return Some(num);
    }
    if i + 1 < s.len() {
        return None;
    }
    let mult: u64 = match s[i].to_ascii_uppercase() {
        b'K' => 1 << 10,
        b'M' => 1 << 20,
        b'G' => 1 << 30,
        b'T' => 1 << 40,
        _ => return None,
    };
    Some(num * mult)
}

impl Zip {
    /// `ziptyp`: acrescenta ".zip" se o último componente não tem ponto (e não é `-A`).
    pub fn ziptyp(&self, s: &[u8]) -> Vec<u8> {
        let mut t = s.to_vec();
        if self.adjust {
            return t;
        }
        let comp = super::names::last(&t, b'/');
        if !comp.contains(&b'.') {
            t.extend_from_slice(b".zip");
        }
        t
    }

    /// `add_filter`: um padrão de `-i`, `-x` ou `-R`; `@arquivo` traz um padrão por linha.
    pub fn add_filter(&mut self, flag: u8, pattern: &[u8]) -> R<()> {
        if pattern.first() == Some(&b'@') {
            if pattern.len() == 1 {
                return Err(self.ziperr(ZE_PARMS, "missing file after @"));
            }
            let data = match sysutil::read_path(&pattern[1..]) {
                Ok(d) => d,
                Err(e) => {
                    self.last_errno = Some(e);
                    let msg = format!("{} pattern file '{}'", flag as char, String::from_utf8_lossy(pattern));
                    return Err(self.ziperr(ZE_OPEN, &msg));
                }
            };
            let mut pos = 0usize;
            while let Some(p) = getnam(&data, &mut pos) {
                let iname = ex2in(&p, self.pathput, self.dosify);
                self.filterlist.push(Plist { zname: iname, select: flag });
            }
        } else {
            let iname = ex2in(pattern, self.pathput, self.dosify);
            self.filterlist.push(Plist { zname: iname, select: flag });
        }
        Ok(())
    }

    /// `filterlist_to_patterns`.
    pub fn filterlist_to_patterns(&mut self) {
        let list = std::mem::take(&mut self.filterlist);
        for p in list {
            match p.select {
                b'i' => self.icount += 1,
                b'R' => self.rcount += 1,
                _ => {}
            }
            self.patterns.push(p);
        }
    }

    pub fn pcount(&self) -> usize {
        self.patterns.len() + self.filterlist.len()
    }

    /// `-sd`: mensagem de depuração.
    pub fn sd(&mut self, m: &str) {
        let s = format!("sd: {m}\n");
        self.mesg_raw(s.as_bytes());
    }

    // ---- entrada padrão em linhas ----

    fn fill_stdin(&mut self, fd: Fd) {
        if self.stdin_eof {
            return;
        }
        let mut buf = vec![0u8; 4096];
        loop {
            match sys::read(fd, &mut buf) {
                Ok(0) => {
                    self.stdin_eof = true;
                    return;
                }
                Ok(n) => {
                    self.stdin_buf.extend_from_slice(&buf[..n]);
                    return;
                }
                Err(Errno::EINTR) => continue,
                Err(_) => {
                    self.stdin_eof = true;
                    return;
                }
            }
        }
    }

    /// `fgets(buf, max, stream)`: até `max - 1` bytes ou até a quebra de linha (inclusive).
    pub fn fgets(&mut self, fd: Fd, max: usize) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            if self.stdin_pos >= self.stdin_buf.len() {
                self.stdin_buf.clear();
                self.stdin_pos = 0;
                self.fill_stdin(fd);
                if self.stdin_buf.is_empty() {
                    return if out.is_empty() { None } else { Some(out) };
                }
            }
            let c = self.stdin_buf[self.stdin_pos];
            self.stdin_pos += 1;
            out.push(c);
            if c == b'\n' || out.len() >= max - 1 {
                return Some(out);
            }
        }
    }

    /// Todos os nomes que restam de um fluxo (o `getnam(stdin)` em laço).
    pub fn read_names(&mut self, fd: Fd) -> Vec<Vec<u8>> {
        let mut all: Vec<u8> = self.stdin_buf[self.stdin_pos..].to_vec();
        self.stdin_buf.clear();
        self.stdin_pos = 0;
        if !self.stdin_eof {
            if let Ok(rest) = sysutil::read_fd(fd) {
                all.extend_from_slice(&rest);
            }
            self.stdin_eof = true;
        }
        let mut pos = 0usize;
        let mut names = Vec::new();
        while let Some(n) = getnam(&all, &mut pos) {
            names.push(n);
        }
        names
    }

    // ---- estatísticas na tela ----

    pub fn display_running_stats(&mut self) {
        if self.mesg_line_started {
            self.mesg_raw(b"\n");
            self.mesg_line_started = false;
        }
        if self.logfile_line_started {
            self.log_raw(b"\n");
            self.logfile_line_started = false;
        }
        let mut s = String::new();
        if self.display_volume {
            s.push_str("1>1: ");
        }
        if self.display_counts {
            s.push_str(&format!("{:3}/{:3} ", self.files_so_far, self.files_total.wrapping_sub(self.files_so_far) as i64));
        }
        if self.display_bytes {
            s.push_str(&format!("[{:>4}", write_num_string(self.bytes_so_far)));
            if self.bytes_total >= self.bytes_so_far {
                s.push_str(&format!("/{:>4}] ", write_num_string(self.bytes_total - self.bytes_so_far)));
            } else {
                s.push_str(&format!("-{:>4}] ", write_num_string(self.bytes_so_far - self.bytes_total)));
            }
        }
        if !s.is_empty() {
            if self.noisy {
                self.mesg_raw(s.as_bytes());
                self.mesg_line_started = true;
            }
            if self.logall {
                self.log_raw(s.as_bytes());
                self.logfile_line_started = true;
            }
        }
    }

    pub fn blank_running_stats(&mut self) {
        let mut s = String::new();
        if self.display_volume {
            s.push_str("1>1: ");
        }
        if self.display_counts {
            s.push_str("   /    ");
        }
        if self.display_bytes {
            s.push_str("     /      ");
        }
        if !s.is_empty() {
            if self.noisy {
                self.mesg_raw(s.as_bytes());
                self.mesg_line_started = true;
            }
            if self.logall {
                self.log_raw(s.as_bytes());
                self.logfile_line_started = true;
            }
        }
    }

    // ---- arquivo temporário e substituição ----

    /// `mkstemp` do template `<dir>ziXXXXXX`: cria o arquivo (modo 0600) e devolve o fd e o nome.
    pub fn mkstemp_zip(&mut self) -> R<(Fd, Vec<u8>)> {
        let mut base: Vec<u8> = match &self.tempath {
            Some(t) => {
                let mut b = t.clone();
                if b.last() != Some(&b'/') {
                    b.push(b'/');
                }
                b
            }
            None => {
                let z = &self.zipfile;
                let cut = z.iter().rposition(|&c| c == b'/').map(|i| i + 1).unwrap_or(0);
                z[..cut].to_vec()
            }
        };
        base.extend_from_slice(b"zi");
        const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        for _ in 0..100 {
            let mut rnd = [0u8; 6];
            let _ = sys::current().getrandom(&mut rnd);
            let mut name = base.clone();
            for b in rnd {
                name.push(ALPHA[b as usize % ALPHA.len()]);
            }
            match sys::open(&name, OFlags::RDWR | OFlags::CREAT | OFlags::EXCL | OFlags::CLOEXEC, 0o600) {
                Ok(fd) => return Ok((fd, name)),
                Err(Errno::EEXIST) => continue,
                Err(e) => {
                    self.last_errno = Some(e);
                    return Err(self.ziperr(ZE_TEMP, &String::from_utf8_lossy(&name)));
                }
            }
        }
        let n = String::from_utf8_lossy(&base).into_owned();
        Err(self.ziperr(ZE_TEMP, &n))
    }

    /// `replace`: o zip temporário `s` passa a ser `d`. Devolve um código `ZE_*`.
    pub fn replace(&mut self, d: &[u8], s: &[u8]) -> i32 {
        let mut copy = false;
        if let Ok(t) = sysutil::lstat(d) {
            if t.nlink > 1 || (t.mode & sysabi::mode::S_IFMT) == sysabi::mode::S_IFLNK {
                copy = true;
            } else if let Err(e) = sys::current().unlinkat(Fd::CWD, d, AtFlags::empty()) {
                self.last_errno = Some(e);
                return ZE_CREAT;
            }
        }
        if !copy {
            if let Err(e) = sys::current().renameat2(Fd::CWD, s, Fd::CWD, d, sysabi::RenameFlags::empty()) {
                self.last_errno = Some(e);
                copy = true;
                if e != Errno::EXDEV {
                    return ZE_CREAT;
                }
            }
        }
        if copy {
            let data = match sysutil::read_path(s) {
                Ok(d) => d,
                Err(e) => {
                    self.last_errno = Some(e);
                    let m = format!(" replace: can't open {}\n", String::from_utf8_lossy(s));
                    self.mesg_raw(m.as_bytes());
                    return ZE_TEMP;
                }
            };
            let fd = match sys::open(d, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC, 0o666) {
                Ok(fd) => fd,
                Err(e) => {
                    self.last_errno = Some(e);
                    return ZE_CREAT;
                }
            };
            let w = sys::write_all(fd, &data);
            let c = sys::close(fd);
            if w.is_err() || c.is_err() {
                let _ = sys::current().unlinkat(Fd::CWD, d, AtFlags::empty());
                return ZE_WRITE;
            }
            let _ = sys::current().unlinkat(Fd::CWD, s, AtFlags::empty());
        }
        ZE_OK
    }

    pub fn destroy(&self, f: &[u8]) -> bool {
        sys::current().unlinkat(Fd::CWD, f, AtFlags::empty()).is_err()
    }

    /// `stamp`: dá ao arquivo o horário DOS `d`.
    pub fn stamp(&self, f: &[u8], d: u64) {
        let t = dos2unixtime(d, &self.tz);
        let ts = SetTime::At(TimeSpec { sec: t, nsec: 0 });
        let _ = sys::current().utimensat(Fd::CWD, f, ts, ts, AtFlags::empty());
    }

    // ---- -T ----

    /// `quote_arg`: o argumento entre aspas para o shell, com os escapes do Unix.
    pub fn quote_arg(s: &[u8]) -> Vec<u8> {
        let mut out = vec![b'"'];
        for &c in s {
            match c {
                b'"' => {
                    out.push(b'\\');
                    out.push(c);
                }
                b'!' => out.extend_from_slice(b"\"'!'\""),
                b'$' | b'`' | b'\\' => {
                    out.push(b'\\');
                    out.push(c);
                }
                _ => out.push(c),
            }
        }
        out.push(b'"');
        out
    }

    /// `check_zipfile`: testa o zip com `unzip -t` (ou o comando de `-TT`).
    pub fn check_zipfile(&mut self, zipname: &[u8]) -> R<()> {
        let status: i32 = if let Some(cmd0) = self.unzip_path.take() {
            let q = Zip::quote_arg(zipname);
            let mut cmd = Vec::new();
            match cmd0.windows(2).position(|w| w == b"{}") {
                Some(i) => {
                    cmd.extend_from_slice(&cmd0[..i]);
                    cmd.push(b' ');
                    cmd.extend_from_slice(&q);
                    cmd.push(b' ');
                    cmd.extend_from_slice(&cmd0[i + 2..]);
                }
                None => {
                    cmd.extend_from_slice(&cmd0);
                    cmd.push(b' ');
                    cmd.extend_from_slice(&q);
                }
            }
            run_shell(&cmd)
        } else {
            let mut argv: Vec<Vec<u8>> = vec![b"unzip".to_vec(), if self.verbose > 0 { b"-t".to_vec() } else { b"-tqq".to_vec() }];
            argv.push(zipname.to_vec());
            super::super::unzip::main(&argv)
        };
        if status != 0 {
            let m = format!("test of {} FAILED\n", String::from_utf8_lossy(&self.zipfile));
            self.mesg_raw(m.as_bytes());
            return Err(self.ziperr(ZE_TEST, "original files unmodified"));
        }
        if self.noisy {
            let m = format!("test of {} OK\n", String::from_utf8_lossy(&self.zipfile));
            self.mesg_raw(m.as_bytes());
        }
        if self.logfile.is_some() {
            let m = format!("test of {} OK\n", String::from_utf8_lossy(&self.zipfile));
            self.log_raw(m.as_bytes());
        }
        Ok(())
    }

    // ---- -o e -m ----

    /// `finish`: aplica `-o` e `-m` e devolve o código de saída.
    pub fn finish(&mut self, e: i32) -> R<i32> {
        if self.latest && !self.zipfile.is_empty() && self.zipfile != b"-" {
            if self.zfiles.is_empty() {
                self.zipwarn("zip file is empty, can't make it as old as latest entry", "");
            } else {
                let mut t: u64 = 0;
                for z in &self.zfiles {
                    if z.iname.last() != Some(&b'/') {
                        let mut zu = IzTimes::default();
                        let z_tim = if get_ef_ut_ztime(z, &mut zu) & EB_UT_FL_MTIME != 0 { unix2dostime(zu.mtime, &self.tz) } else { z.tim };
                        if t < z_tim {
                            t = z_tim;
                        }
                    }
                }
                if t != 0 {
                    let zf = self.zipfile.clone();
                    self.stamp(&zf, t);
                } else {
                    self.zipwarn("zip file has only directories, can't make it as old as latest entry", "");
                }
            }
        }
        if self.dispose {
            if let Err(code) = self.trash() {
                return Err(self.ziperr(code, "was deleting moved files and directories"));
            }
        }
        if let Some(fd) = self.logfile.take() {
            let _ = sys::close(fd);
        }
        Ok(e)
    }

    /// `trash`: apaga os arquivos que entraram no zip (e os diretórios que ficaram vazios).
    pub fn trash(&mut self) -> Result<(), i32> {
        let mut n = 0usize;
        for i in 0..self.zfiles.len() {
            if self.zfiles[i].mark == 1 || self.zfiles[i].trash {
                self.zfiles[i].mark = 1;
                if self.zfiles[i].iname.last() != Some(&b'/') {
                    let name = self.zfiles[i].name.clone();
                    if self.verbose > 0 {
                        let mut l = b"zip diagnostic: deleting file ".to_vec();
                        l.extend_from_slice(&name);
                        l.push(b'\n');
                        self.mesg_raw(&l);
                    }
                    if self.destroy(&name) {
                        self.zipwarn("error deleting ", &name);
                    }
                    if !self.dirnames {
                        let z = &mut self.zfiles[i];
                        cutpath(&mut z.name);
                        cutpath(&mut z.iname);
                        // O C troca o último caractere por '/' e acrescenta um NUL ao tamanho.
                        if !z.iname.is_empty() {
                            let l = z.iname.len();
                            z.iname[l - 1] = b'/';
                            z.iname.push(0);
                            n += 1;
                        }
                    }
                } else {
                    n += 1;
                }
            }
        }
        if n > 0 {
            let mut s: Vec<usize> = Vec::new();
            for i in 0..self.zfiles.len() {
                let z = &self.zfiles[i];
                if z.mark != 0 && !z.iname.is_empty() && z.iname.last() == Some(&b'/') && (s.is_empty() || z.name != self.zfiles[*s.last().unwrap()].name) {
                    s.push(i);
                }
            }
            s.sort_by(|&a, &b| self.zfiles[b].iname.cmp(&self.zfiles[a].iname));
            let mut prev: Option<Vec<u8>> = None;
            for &i in &s {
                let mut p = self.zfiles[i].name.clone();
                if p.is_empty() {
                    continue;
                }
                if p.last() == Some(&b'/') {
                    p.pop();
                }
                if prev.as_ref() != Some(&p) {
                    if self.verbose > 0 {
                        let mut l = b"deleting directory ".to_vec();
                        l.extend_from_slice(&p);
                        l.extend_from_slice(b" (if empty)                \n");
                        self.mesg_raw(&l);
                    }
                    let _ = sys::current().unlinkat(Fd::CWD, &p, AtFlags::REMOVEDIR);
                }
                prev = Some(p);
            }
        }
        Ok(())
    }
}

/// `cutpath`: tira o último componente do caminho (vira vazio se não há barra).
fn cutpath(p: &mut Vec<u8>) {
    match p.iter().rposition(|&c| c == b'/') {
        Some(i) => p.truncate(i),
        None => p.clear(),
    }
}

/// `system(cmd)` via `sh -c`; devolve 0 em sucesso.
fn run_shell(cmd: &[u8]) -> i32 {
    let s = sys::current();
    let spec = sysabi::SpawnSpec { path: b"/bin/sh".to_vec(), argv: vec![b"sh".to_vec(), b"-c".to_vec(), cmd.to_vec()], attrs: sysabi::ProcAttrs::default() };
    match s.spawn(spec) {
        Ok(pid) => loop {
            match s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()) {
                Ok(Some((_, WaitStatus::Exited(c)))) => return c,
                Ok(Some((_, _))) => return 1,
                Ok(None) => continue,
                Err(Errno::EINTR) => continue,
                Err(_) => return 1,
            }
        },
        Err(_) => 127,
    }
}
