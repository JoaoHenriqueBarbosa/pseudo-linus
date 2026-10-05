//! Escolha dos arquivos (fileio.c e unix.c): filtros `-i`/`-x`/`-R`, `newname`, `procname`, busca
//! de entradas e remoção de duplicatas.

use sysabi::mode::{S_IFDIR, S_IFIFO, S_IFLNK, S_IFMT, S_IFREG};
use sysabi::{Clock, Fd};

use super::consts::*;
use super::matching::{namecmp, shmatch};
use super::names::{display_name, ex2in, local_to_utf8};
use super::state::{FileInfo, Flist, IzTimes, R, Zip};
use super::times::unix2dostime;
use crate::sysutil;

impl Zip {
    pub fn now_sec(&self) -> i64 {
        match sysabi::sys::try_current() {
            Some(s) => s.clock_gettime(Clock::Realtime).map(|t| t.sec).unwrap_or(0),
            None => 0,
        }
    }

    /// `MATCH` com as opções correntes.
    pub fn matches(&self, p: &[u8], s: &[u8], cs: bool) -> bool {
        shmatch(p, s, cs, self.no_wild, self.wild_stop_at_dir, self.allow_regex)
    }

    /// `filter`: o nome deve entrar? Considera `-x` (que vence), `-i` e `-R`.
    pub fn filter(&self, name: &[u8], casesensitive: bool) -> bool {
        let mut imatch = self.icount == 0;
        let mut rmatch = self.rcount == 0;
        if self.patterns.is_empty() {
            return true;
        }
        for pat in &self.patterns {
            if pat.zname.is_empty() {
                continue;
            }
            let mut p: &[u8] = name;
            match pat.select {
                b'R' => {
                    if rmatch {
                        continue;
                    }
                    // Com -R, um padrão de N componentes só testa os últimos N do nome.
                    let mut slashes = pat.zname.iter().filter(|&&c| c == b'/').count() as i64;
                    slashes -= name.iter().filter(|&&c| c == b'/').count() as i64;
                    if slashes < 0 {
                        for (i, &c) in name.iter().enumerate() {
                            if c == b'/' {
                                slashes += 1;
                                if slashes == 0 {
                                    p = &name[i + 1..];
                                    break;
                                }
                            }
                        }
                    }
                }
                b'i' => {
                    if imatch {
                        continue;
                    }
                }
                _ => {}
            }
            if self.matches(&pat.zname, p, casesensitive) {
                match pat.select {
                    b'x' => return false,
                    b'R' => rmatch = true,
                    _ => imatch = true,
                }
            }
        }
        imatch && rmatch
    }

    /// `filetime`: `None` se o arquivo não existe. Fala `error("fstat(stdin)")` se a entrada padrão
    /// não pode ser examinada.
    pub fn filetime(&mut self, f: &[u8]) -> R<Option<FileInfo>> {
        let st = if f == b"-" {
            match sysutil::fstat(Fd::STDIN) {
                Ok(s) => s,
                Err(_) => return Err(self.ziperr(ZE_LOGIC, "fstat(stdin)")),
            }
        } else {
            let mut name = f.to_vec();
            if name.last() == Some(&b'/') {
                name.pop();
            }
            let r = if self.linkput { sysutil::lstat(&name) } else { sysutil::stat(&name) };
            match r {
                Ok(s) => s,
                Err(_) => return Ok(None),
            }
        };
        let mode = st.mode;
        let mut attr: u64 = ((mode as u64) << 16) | ((mode & 0o200 == 0) as u64);
        if mode & S_IFMT == S_IFDIR {
            attr |= MSDOS_DIR_ATTR;
        }
        let size: i64 = if mode & S_IFMT == S_IFREG || mode & S_IFMT == S_IFLNK { st.size as i64 } else { -1 };
        let utim = IzTimes { atime: st.atime.sec, mtime: st.mtime.sec, ctime: st.mtime.sec };
        Ok(Some(FileInfo { tim: unix2dostime(st.mtime.sec, &self.tz), attr, size, utim }))
    }

    /// `newname`: acrescenta (ou marca) o nome de um arquivo existente.
    pub fn newname(&mut self, name: &[u8], isdir: bool, casesensitive: bool) -> R<()> {
        if self.noisy {
            if self.scan_count == 0 {
                self.scan_start = self.now_sec();
            }
            self.scan_count += 1;
            if self.scan_count % 100 == 0 {
                let current = self.now_sec();
                if current - self.scan_start > self.scan_delay {
                    if self.scan_last == 0 {
                        self.zipmessage_nl(b"Scanning files ", false);
                        self.scan_last = current;
                    }
                    if current - self.scan_last > self.scan_dot_time {
                        self.scan_last = current;
                        self.mesg_raw(b".");
                    }
                }
            }
        }
        let _ = isdir;
        let dosflag = self.dosify;
        let iname = ex2in(name, self.pathput, self.dosify);
        if iname.is_empty() {
            if self.pathput && self.recurse == 0 {
                return Err(self.ziperr(ZE_LOGIC, "empty name without -j or -r"));
            }
            return Ok(());
        }
        let undosm: Vec<u8> = if dosflag || !self.pathput { ex2in(name, true, false) } else { iname.clone() };
        let zname = iname.clone();
        let oname = display_name(&iname);
        if let Some(zi) = self.zsearch(&zname) {
            if !self.patterns.is_empty() && !self.filter(&undosm, casesensitive) {
                if self.verbose > 0 {
                    let mut l = b"excluding ".to_vec();
                    l.extend_from_slice(&oname);
                    l.push(b'\n');
                    self.mesg_raw(&l);
                }
            } else {
                let z = &mut self.zfiles[zi];
                z.mark = 1;
                z.name = name.to_vec();
                z.oname = oname;
                z.dosflag = dosflag;
            }
        } else if self.patterns.is_empty() || self.filter(&undosm, casesensitive) {
            // Não acrescenta o próprio arquivo zip a ele mesmo.
            if self.zipstate == -1 {
                self.zipstatb = None;
                if self.zipfile != b"-" {
                    if let Ok(s) = sysutil::stat(&self.zipfile) {
                        self.zipstatb = Some(s);
                    }
                }
                self.zipstate = self.zipstatb.is_some() as i32;
            }
            if self.zipstate == 1 {
                if let (Some(zs), Ok(sb)) = (self.zipstatb.as_ref(), sysutil::stat(name)) {
                    if zs.mode == sb.mode && zs.ino == sb.ino && zs.dev == sb.dev && zs.uid == sb.uid && zs.gid == sb.gid && zs.size == sb.size && zs.mtime.sec == sb.mtime.sec && zs.ctime.sec == sb.ctime.sec {
                        if self.verbose > 0 {
                            self.mesg_raw(b"file matches zip file -- skipping\n");
                        }
                        return Ok(());
                    }
                }
            }
            let uname = local_to_utf8(&iname);
            self.found.push(Flist { name: name.to_vec(), iname, zname, oname, uname, dosflag, usize: 0 });
        }
        Ok(())
    }

    /// `procname`: processa um nome ou expressão da linha de comando. Devolve `ZE_OK` ou `ZE_MISS`.
    pub fn procname(&mut self, n: &[u8], caseflag: bool) -> R<i32> {
        if n == b"-" {
            self.newname(n, false, caseflag)?;
            return Ok(ZE_OK);
        }
        let st = if self.linkput { sysutil::lstat(n) } else { sysutil::stat(n) };
        let s = match st {
            Err(_) => {
                // Não é arquivo nem diretório: procura a expressão nas entradas do zip.
                let p = ex2in(n, self.pathput, self.dosify);
                let mut m = true;
                for i in 0..self.zfiles.len() {
                    if self.matches(&p, &self.zfiles[i].iname, caseflag) {
                        let mark = if !self.patterns.is_empty() { self.filter(&self.zfiles[i].zname, caseflag) as i32 } else { 1 };
                        self.zfiles[i].mark = mark;
                        if self.verbose > 0 {
                            let mut l = format!("zip diagnostic: {}cluding ", if mark != 0 { "in" } else { "ex" }).into_bytes();
                            l.extend_from_slice(&self.zfiles[i].name);
                            l.push(b'\n');
                            self.mesg_raw(&l);
                        }
                        m = false;
                    }
                }
                return Ok(if m { ZE_MISS } else { ZE_OK });
            }
            Ok(s) => s,
        };
        let mode = s.mode;
        if mode & S_IFREG == S_IFREG || mode & S_IFLNK == S_IFLNK {
            self.newname(n, false, caseflag)?;
        } else if mode & S_IFDIR == S_IFDIR {
            let mut p: Vec<u8>;
            if n == b"." {
                p = Vec::new();
            } else {
                p = n.to_vec();
                if p.last() != Some(&b'/') {
                    p.push(b'/');
                }
                if self.dirnames {
                    self.newname(&p, true, caseflag)?;
                }
            }
            if self.recurse != 0 {
                if let Ok(entries) = sysabi::sys::read_dir(n) {
                    for e in entries {
                        if e.name == b"." || e.name == b".." {
                            continue;
                        }
                        let mut a = p.clone();
                        a.extend_from_slice(&e.name);
                        let m = self.procname(&a, caseflag)?;
                        if m != ZE_OK {
                            if m == ZE_MISS {
                                self.zipwarn("name not matched: ", &a);
                            } else {
                                return Err(self.ziperr(m, &String::from_utf8_lossy(&a)));
                            }
                        }
                    }
                }
            }
        } else if mode & S_IFIFO == S_IFIFO {
            if self.allow_fifo {
                if self.noisy {
                    self.zipwarn("Reading FIFO (Named Pipe): ", n);
                }
                self.newname(n, false, caseflag)?;
            } else {
                self.zipwarn("ignoring FIFO (Named Pipe) - use -FI to read: ", n);
                return Ok(ZE_OK);
            }
        } else {
            self.zipwarn("ignoring special file: ", n);
        }
        Ok(ZE_OK)
    }

    /// `proc_archive_name`: marca as entradas do zip que casam com o nome (`-d`, `-U`).
    pub fn proc_archive_name(&mut self, n: &[u8], caseflag: bool) -> R<i32> {
        if n == b"-" {
            self.zipwarn("Cannot select stdin when selecting archive entries", "");
            return Ok(ZE_MISS);
        }
        let p = ex2in(n, self.pathput, self.dosify);
        let mut m = true;
        for i in 0..self.zfiles.len() {
            if self.matches(&p, &self.zfiles[i].iname, caseflag) {
                let mark = if !self.patterns.is_empty() { self.filter(&self.zfiles[i].zname, caseflag) as i32 } else { 1 };
                self.zfiles[i].mark = mark;
                if self.verbose > 0 {
                    let mut l = format!("zip diagnostic: {}cluding ", if mark != 0 { "in" } else { "ex" }).into_bytes();
                    l.extend_from_slice(&self.zfiles[i].oname);
                    l.push(b'\n');
                    self.mesg_raw(&l);
                }
                m = false;
            }
        }
        // Também os nomes Unicode escapados.
        for i in 0..self.zfiles.len() {
            if let Some(zu) = self.zfiles[i].zuname.clone() {
                if self.matches(&p, &zu, caseflag) {
                    let mark = if !self.patterns.is_empty() { self.filter(&zu, caseflag) as i32 } else { 1 };
                    self.zfiles[i].mark = mark;
                    if self.verbose > 0 {
                        let mut l = format!("zip diagnostic: {}cluding ", if mark != 0 { "in" } else { "ex" }).into_bytes();
                        l.extend_from_slice(&self.zfiles[i].oname);
                        l.extend_from_slice(b"\n     Escaped Unicode:  ");
                        l.extend_from_slice(self.zfiles[i].ouname.as_deref().unwrap_or(b""));
                        l.push(b'\n');
                        self.mesg_raw(&l);
                    }
                    m = false;
                }
            }
        }
        Ok(if m { ZE_MISS } else { ZE_OK })
    }

    /// `zsearch`: a entrada do zip com o nome externo `n` (busca binária como no C), ou a de nome
    /// Unicode igual.
    pub fn zsearch(&self, n: &[u8]) -> Option<usize> {
        if self.zfiles.is_empty() || self.zsort.is_empty() {
            return None;
        }
        if let Some(i) = bsearch(&self.zsort, |idx| namecmp(n, &self.zfiles[idx].zname)) {
            return Some(i);
        }
        if self.unicode_mismatch != 3 && self.fix != 2 {
            if let Some(i) = bsearch(&self.zusort, |idx| {
                let z = &self.zfiles[idx];
                namecmp(n, z.zuname.as_deref().unwrap_or(&z.zname))
            }) {
                return Some(i);
            }
        }
        None
    }

    /// `check_dup`: ordena os arquivos achados, tira os repetidos e avisa de nomes internos iguais.
    /// Devolve `ZE_PARMS` se há nomes repetidos no zip.
    pub fn check_dup(&mut self) -> i32 {
        if self.found.is_empty() {
            return ZE_OK;
        }
        // Pelo nome dado: remove duplicatas (mantém a primeira de cada nome).
        let mut order: Vec<usize> = (0..self.found.len()).collect();
        order.sort_by(|&a, &b| self.found[a].name.cmp(&self.found[b].name));
        let mut remove = vec![false; self.found.len()];
        for j in 1..order.len() {
            if self.found[order[j - 1]].name == self.found[order[j]].name {
                remove[order[j]] = true;
            }
        }
        let mut idx = 0usize;
        self.found.retain(|_| {
            let k = !remove[idx];
            idx += 1;
            k
        });
        // Depois pelo nome interno (ordenação estável: os iguais ficam na ordem dos nomes).
        let mut by_name: Vec<usize> = (0..self.found.len()).collect();
        by_name.sort_by(|&a, &b| self.found[a].name.cmp(&self.found[b].name));
        by_name.sort_by(|&a, &b| self.found[a].iname.cmp(&self.found[b].iname));
        for j in 1..by_name.len() {
            let (a, b) = (&self.found[by_name[j - 1]], &self.found[by_name[j]]);
            if a.iname == b.iname {
                let mut msg: Vec<u8> = Vec::new();
                msg.extend_from_slice(b"  first full name: ");
                msg.extend_from_slice(&a.name);
                msg.extend_from_slice(b"\n                     ");
                msg.extend_from_slice(b" second full name: ");
                msg.extend_from_slice(&b.name);
                msg.extend_from_slice(b"\n                     ");
                msg.extend_from_slice(b"name in zip file repeated: ");
                msg.extend_from_slice(&b.iname);
                if !self.pathput {
                    msg.extend_from_slice(b"\n                     this may be a result of using -j");
                }
                self.zipwarn(msg, "");
                return ZE_PARMS;
            }
        }
        ZE_OK
    }
}

/// `search`: busca binária na lista ordenada de índices; `cmp(idx)` é negativo se o alvo vem antes.
fn bsearch(list: &[usize], cmp: impl Fn(usize) -> i32) -> Option<usize> {
    let mut l: i64 = 0;
    let mut u: i64 = list.len() as i64 - 1;
    while u >= l {
        let i = l + ((u - l) as u64 >> 1) as i64;
        let r = cmp(list[i as usize]);
        if r < 0 {
            u = i - 1;
        } else if r > 0 {
            l = i + 1;
        } else {
            return Some(list[i as usize]);
        }
    }
    None
}
