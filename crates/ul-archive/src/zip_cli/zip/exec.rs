//! A segunda metade do `main` do zip: lê o zip existente, escolhe o que fazer com cada entrada e
//! arquivo, escreve o novo zip (entradas, diretório central, fim) e o põe no lugar do antigo.

use sysabi::sys;
use sysabi::{Fd, OFlags};

use super::consts::*;
use super::extra::get_ef_ut_ztime;
use super::matching::namecmp;
use super::names::{display_name, is_ascii};
use super::out::OutFile;
use super::state::{IzTimes, R, Zip, Zlist};
use super::times::unix2dostime;
use super::zipup::{display_num, percent};
use crate::sysutil;

impl Zip {
    fn procname_caseflag(&self) -> bool {
        (self.action == ARCHIVE || self.action == DELETE || self.action == FRESHEN) && self.filter_match_case
    }

    /// `zipmessage_nl("", 1)`: fecha a linha corrente.
    fn end_line(&mut self) {
        self.zipmessage_nl(b"", true);
    }

    /// Mostra `-sf` e variantes.
    fn show_files_report(&mut self) {
        let mut count: u64 = 0;
        let mut bytes: u64 = 0;
        let sf = self.show_files;
        if self.noisy && (sf == 1 || sf == 3 || sf == 5) {
            if self.mesg_line_started {
                self.mesg_raw(b"\n");
                self.mesg_line_started = false;
            }
            let t: &[u8] = if self.kk == 3 {
                b"Archive contains:\n"
            } else if self.action == DELETE {
                b"Would Delete:\n"
            } else if self.action == FRESHEN {
                b"Would Freshen:\n"
            } else if self.action == ARCHIVE {
                b"Would Copy:\n"
            } else {
                b"Would Add/Update:\n"
            };
            self.mesg_raw(t);
        }
        if self.logfile.is_some() {
            if self.logfile_line_started {
                self.log_raw(b"\n");
                self.logfile_line_started = false;
            }
            let t: &[u8] = if self.kk == 3 {
                b"Archive contains:\n"
            } else if self.action == DELETE {
                b"Would Delete:\n"
            } else if self.action == FRESHEN {
                b"Would Freshen:\n"
            } else if self.action == ARCHIVE {
                b"Would Copy:\n"
            } else {
                b"Would Add/Update:\n"
            };
            self.log_raw(t);
        }
        let line = |n: &[u8]| -> Vec<u8> {
            let mut l = b"  ".to_vec();
            l.extend_from_slice(n);
            l.push(b'\n');
            l
        };
        let zf = self.zfiles.clone();
        for z in &zf {
            if z.mark != 0 || self.kk == 3 {
                count += 1;
                if (z.len as i64) > 0 {
                    bytes += z.len;
                }
                if self.noisy && (sf == 1 || sf == 3) {
                    self.mesg_raw(&line(&z.oname));
                }
                if self.logfile.is_some() && !(sf == 5 || sf == 6) {
                    self.log_raw(&line(&z.oname));
                }
                if sf == 3 || sf == 4 {
                    if let Some(ou) = &z.ouname {
                        let mut l = b"     Escaped Unicode:  ".to_vec();
                        l.extend_from_slice(ou);
                        l.push(b'\n');
                        if self.noisy && sf == 3 {
                            self.mesg_raw(&l);
                        }
                        self.log_raw(&l);
                    }
                }
                if sf == 5 || sf == 6 {
                    let n: &[u8] = z.ouname.as_deref().unwrap_or(&z.oname);
                    if self.noisy && sf == 5 {
                        self.mesg_raw(&line(n));
                    }
                    self.log_raw(&line(n));
                }
            }
        }
        let fl = self.found.clone();
        for f in &fl {
            count += 1;
            if (f.usize as i64) > 0 {
                bytes += f.usize;
            }
            if self.noisy && (sf == 1 || sf == 3 || sf == 5) {
                self.mesg_raw(&line(&f.oname));
            }
            self.log_raw(&line(&f.oname));
        }
        if self.noisy || self.logfile.is_none() {
            let l = format!("Total {} entries ({} bytes)\n", count, bytes);
            self.mesg_raw(l.as_bytes());
        }
        if self.logfile.is_some() {
            let l = format!("Total {} entries ({} bytes)\n", count, bytes);
            self.log_raw(l.as_bytes());
        }
    }

    /// Escreve `msg` na tela (se `noisy`) e no log (se `logall`) sem tocar o estado de linha.
    fn say(&mut self, msg: &[u8]) {
        if self.noisy {
            self.mesg_raw(msg);
            self.mesg_line_started = true;
        }
        if self.logall {
            self.log_raw(msg);
            self.logfile_line_started = true;
        }
    }

    /// A segunda metade de `main`. Devolve o código de saída.
    pub fn execute(&mut self, argc: usize) -> R<i32> {
        if self.show_sd {
            self.sd("Reading archive");
        }
        let r = self.readzipfile()?;
        if r != ZE_OK {
            let zf = self.zipfile.clone();
            return Err(self.ziperr(r, &String::from_utf8_lossy(&zf)));
        }
        if !self.zipfile_exists && self.show_files != 0 && (self.kk == 3 || self.action == ARCHIVE) {
            let zf = self.zipfile.clone();
            return Err(self.ziperr(ZE_OPEN, &String::from_utf8_lossy(&zf)));
        }
        if self.zfiles.is_empty() && (self.action != ADD || self.grow) {
            let zf = self.zipfile.clone();
            self.zipwarn(zf, " not found or empty");
        }
        if self.have_out && self.kk == 3 {
            for i in 0..self.zfiles.len() {
                let m = if self.pcount() > 0 { self.filter(&self.zfiles[i].zname, self.filter_match_case) as i32 } else { 1 };
                self.zfiles[i].mark = m;
            }
        }

        // Os nomes da linha de comando.
        let filelist = std::mem::take(&mut self.filelist);
        if !filelist.is_empty() {
            if self.action == ARCHIVE {
                if self.show_sd {
                    self.sd("Scanning archive entries");
                }
                for name in &filelist {
                    let cf = self.filter_match_case;
                    let r = self.proc_archive_name(name, cf)?;
                    if r != ZE_OK {
                        if r == ZE_MISS {
                            self.zipwarn("not in archive: ", name);
                        } else {
                            return Err(self.ziperr(r, &String::from_utf8_lossy(name)));
                        }
                    }
                }
            } else {
                if self.show_sd {
                    self.sd("Scanning files");
                }
                for name in &filelist {
                    let cf = self.procname_caseflag();
                    let r = self.procname(name, cf)?;
                    if r != ZE_OK {
                        if r == ZE_MISS {
                            if self.bad_open_is_error {
                                self.zipwarn("name not matched: ", name);
                                return Err(self.ziperr(ZE_OPEN, &String::from_utf8_lossy(name)));
                            }
                            self.zipwarn("name not matched: ", name);
                        } else {
                            return Err(self.ziperr(r, &String::from_utf8_lossy(name)));
                        }
                    }
                }
            }
        }
        if self.recurse == 2 {
            let cf = self.procname_caseflag();
            let r = self.procname(b".", cf)?;
            if r != ZE_OK {
                if r == ZE_MISS {
                    if self.bad_open_is_error {
                        self.zipwarn("name not matched: ", "current directory for -R");
                        return Err(self.ziperr(ZE_OPEN, "-R"));
                    }
                    self.zipwarn("name not matched: ", "current directory for -R");
                } else {
                    return Err(self.ziperr(r, "-R"));
                }
            }
        }

        if self.show_sd {
            self.sd("Applying filters");
        }
        if self.kk != 4 && self.first_listarg == 0 && (self.action == UPDATE || self.action == FRESHEN) {
            for i in 0..self.zfiles.len() {
                let m = if self.pcount() > 0 { self.filter(&self.zfiles[i].zname, self.filter_match_case) as i32 } else { 1 };
                self.zfiles[i].mark = m;
            }
        }
        if self.show_sd {
            self.sd("Checking dups");
        }
        let r = self.check_dup();
        if r != ZE_OK {
            if r == ZE_PARMS {
                return Err(self.ziperr(r, "cannot repeat names in zip file"));
            }
            return Err(self.ziperr(r, "was processing list of files"));
        }
        self.zsort.clear();
        self.zusort.clear();

        // O diretório dos arquivos temporários é o do zip, salvo `-b`.
        if self.tempath.is_none() {
            if let Some(p) = self.zipfile.iter().rposition(|&c| c == b'/') {
                self.tempath = Some(self.zipfile[..p].to_vec());
            }
        }

        // Para cada entrada marcada: existe? é mais nova (-u, -f)?
        if self.show_sd {
            self.sd("Scanning files to update");
        }
        let mut k: i64 = 0;
        self.scan_started = false;
        self.scan_count = 0;
        let mut all_current = true;
        for i in 0..self.zfiles.len() {
            if self.noisy && self.scan_last != 0 {
                self.scan_count += 1;
                if self.scan_count % 100 == 0 {
                    let current = self.now_sec();
                    if current - self.scan_last > self.scan_dot_time {
                        if !self.scan_started {
                            self.scan_started = true;
                            self.mesg_raw(b" ");
                        }
                        self.scan_last = current;
                        self.mesg_raw(b".");
                    }
                }
            }
            self.zfiles[i].current = false;
            if self.zfiles[i].mark == 0 {
                all_current = false;
            }
            if self.zfiles[i].mark == 0 {
                continue;
            }
            let (csize, mut usz): (u64, i64) = (self.zfiles[i].siz, self.zfiles[i].len as i64);
            if self.action == DELETE || self.action == ARCHIVE {
                let mut zu = IzTimes::default();
                let z = &self.zfiles[i];
                let z_tim = if get_ef_ut_ztime(z, &mut zu) & EB_UT_FL_MTIME != 0 { unix2dostime(zu.mtime, &self.tz) } else { z.tim };
                if z_tim < self.before || (self.after != 0 && z_tim >= self.after) {
                    self.zfiles[i].mark = 0;
                } else {
                    self.files_total += 1;
                    self.zfiles[i].len = usz as u64;
                    if csize as i64 != -1 && csize as i64 != -2 {
                        self.bytes_total += csize;
                    }
                    k += 1;
                }
            } else {
                let isdirname = self.zfiles[i].name.last() == Some(&b'/');
                let name = self.zfiles[i].name.clone();
                let info = self.filetime(&name)?;
                let tf: u64 = match &info {
                    Some(inf) => {
                        usz = inf.size;
                        inf.tim
                    }
                    None => 0,
                };
                let f_utim = info.map(|i| i.utim).unwrap_or_default();
                if tf == 0 {
                    all_current = false;
                }
                let mut z_utim = IzTimes::default();
                let ut_ok = get_ef_ut_ztime(&self.zfiles[i], &mut z_utim) & EB_UT_FL_MTIME != 0;
                let ztim = self.zfiles[i].tim;
                let ulder = if ut_ok { f_utim.mtime <= z_utim.mtime } else { tf <= ztim };
                if tf == 0 || tf < self.before || (self.after != 0 && tf >= self.after) || ((self.action == UPDATE || self.action == FRESHEN) && ulder) {
                    self.zfiles[i].mark = if self.comadd { 2 } else { 0 };
                    let trash = tf != 0 && tf >= self.before && (self.after == 0 || tf < self.after);
                    self.zfiles[i].trash = trash;
                    if self.verbose > 0 {
                        let mut l = b"zip diagnostic: ".to_vec();
                        l.extend_from_slice(&self.zfiles[i].oname);
                        l.extend_from_slice(if trash { b" up to date\n" } else { b" missing or early\n" });
                        self.mesg_raw(&l);
                        self.log_raw(&l);
                    }
                } else if self.diff_mode && tf == ztim && ((isdirname && usz == -1) || usz as u64 == self.zfiles[i].len) {
                    self.zfiles[i].mark = 0;
                } else {
                    if tf == ztim && ((self.zfiles[i].len == 0 && usz == -1) || usz as u64 == self.zfiles[i].len) {
                        self.zfiles[i].current = true;
                    } else {
                        all_current = false;
                    }
                    self.files_total += 1;
                    if usz != -1 && usz != -2 {
                        self.zfiles[i].len = usz as u64;
                        self.bytes_total += usz as u64;
                    } else {
                        self.zfiles[i].len = 0;
                    }
                    k += 1;
                }
            }
        }

        // Dos arquivos novos, tira os que não existem, são antigos demais ou são o próprio zip.
        if self.show_sd {
            let m = format!("fcount = {}", self.found.len());
            self.sd(&m);
        }
        self.scan_count = 0;
        self.scan_started = false;
        let found = std::mem::take(&mut self.found);
        let mut kept = Vec::new();
        for mut f in found {
            if self.noisy {
                if !self.zip_to_stdout && self.scan_last == 0 && self.scan_count % 100 == 0 {
                    let current = self.now_sec();
                    if current - self.scan_start > self.scan_delay {
                        self.mesg_raw(b"Scanning files ");
                        self.mesg_line_started = true;
                        self.scan_last = current;
                    }
                }
                if self.scan_last != 0 {
                    self.scan_count += 1;
                    if self.scan_count % 100 == 0 {
                        let current = self.now_sec();
                        if current - self.scan_last > self.scan_dot_time {
                            if !self.scan_started {
                                self.scan_started = true;
                                self.mesg_raw(b" ");
                            }
                            self.scan_last = current;
                            self.mesg_raw(b".");
                        }
                    }
                }
            }
            let mut tf: u64 = 0;
            let mut usz: i64 = 0;
            if self.action != DELETE && self.action != FRESHEN {
                if let Some(inf) = self.filetime(&f.name)? {
                    tf = inf.tim;
                    usz = inf.size;
                }
            }
            if self.action == DELETE
                || self.action == FRESHEN
                || tf == 0
                || tf < self.before
                || (self.after != 0 && tf >= self.after)
                || (namecmp(&f.zname, &self.zipfile) == 0 && !self.zip_to_stdout)
            {
                continue;
            }
            self.files_total += 1;
            f.usize = 0;
            if usz != -1 && usz != -2 {
                self.bytes_total += usz as u64;
                f.usize = usz as u64;
            }
            kept.push(f);
        }
        self.found = kept;
        if self.mesg_line_started {
            self.mesg_raw(b"\n");
            self.mesg_line_started = false;
        }

        if self.show_files != 0 {
            self.show_files_report();
            return self.finish(ZE_OK);
        }

        // Há algo a fazer?
        if k == 0
            && self.found.is_empty()
            && !self.diff_mode
            && !(self.zfiles.is_empty() && self.allow_empty_archive)
            && !(!self.zfiles.is_empty() && (self.latest || self.fix != 0 || self.adjust || self.junk_sfx || self.comadd || self.zipedit))
        {
            if self.test && (!self.zfiles.is_empty() || self.zipbeg != 0) {
                let zf = self.zipfile.clone();
                self.check_zipfile(&zf)?;
                return self.finish(ZE_OK);
            }
            if self.action == UPDATE || self.action == FRESHEN {
                return self.finish(ZE_NONE);
            } else if self.zfiles.is_empty() && (self.latest || self.fix != 0 || self.adjust || self.junk_sfx) {
                let zf = self.zipfile.clone();
                return Err(self.ziperr(ZE_NAME, &String::from_utf8_lossy(&zf)));
            } else if self.recurse != 0 && self.pcount() == 0 && self.first_listarg > 0 {
                let args = self.args_final.clone();
                let mut errbuf = b"try: zip".to_vec();
                for a in args.iter().take(self.first_listarg as usize).skip(1) {
                    errbuf.push(b' ');
                    errbuf.extend_from_slice(a);
                }
                errbuf.extend_from_slice(b" . -i");
                for a in args.iter().take(argc).skip(self.first_listarg as usize) {
                    errbuf.push(b' ');
                    errbuf.extend_from_slice(a);
                }
                return Err(self.ziperr(ZE_NONE, &String::from_utf8_lossy(&errbuf)));
            } else {
                let zf = self.zipfile.clone();
                return Err(self.ziperr(ZE_NONE, &String::from_utf8_lossy(&zf)));
            }
        }
        if self.filesync && all_current && self.found.is_empty() {
            self.zipmessage("Archive is current", "");
            return self.finish(ZE_OK);
        }
        self.grow = self.grow && k == 0 && (self.zipbeg != 0 || !self.zfiles.is_empty());

        self.write_archive()
    }

    /// Cria o arquivo de saída (o temporário, o próprio zip no `-g`, ou o stdout) e escreve tudo.
    fn write_archive(&mut self) -> R<i32> {
        let d = self.grow;
        let to_stdout = self.zipfile == b"-";
        // O arquivo zip precisa ser gravável; guarda os atributos para restaurar no fim.
        if !to_stdout {
            if self.tempdir && self.zfiles.is_empty() && self.zipbeg == 0 {
                self.zip_attributes = 0;
            } else {
                let create = self.have_out || (self.zfiles.is_empty() && self.zipbeg == 0);
                let op = self.out_path.clone();
                let fl = if create { OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC } else { OFlags::RDWR };
                match sys::open(&op, fl | OFlags::CLOEXEC, 0o666) {
                    Ok(fd) => {
                        let _ = sys::close(fd);
                    }
                    Err(e) => {
                        self.last_errno = Some(e);
                        return Err(self.ziperr(ZE_CREAT, &String::from_utf8_lossy(&op)));
                    }
                }
                self.zip_attributes = sysutil::stat(&op).map(|s| s.mode).unwrap_or(0);
                if self.zfiles.is_empty() && self.zipbeg == 0 {
                    self.destroy(&op);
                }
            }
        } else {
            self.zip_attributes = 0;
        }
        if self.junk_sfx {
            self.zipbeg = 0;
        }
        if self.show_sd {
            self.sd("Open zip file and create temp file");
        }
        self.tempzn = 0;
        if to_stdout {
            self.y = Some(OutFile::from_fd(Fd::STDOUT));
            self.tempzip = Some(b"-".to_vec());
        } else if d {
            let zf = self.zipfile.clone();
            match sys::open(&zf, OFlags::RDWR | OFlags::CLOEXEC, 0) {
                Ok(fd) => {
                    let mut y = OutFile::from_fd(fd);
                    y.seek_set(self.cenbeg);
                    self.y = Some(y);
                }
                Err(e) => {
                    self.last_errno = Some(e);
                    return Err(self.ziperr(ZE_NAME, &String::from_utf8_lossy(&zf)));
                }
            }
            self.tempzip = Some(zf);
            self.bytes_this_split = self.cenbeg;
            self.tempzn = self.cenbeg;
        } else {
            if self.show_sd {
                self.sd("Creating new zip file");
            }
            let (fd, name) = self.mkstemp_zip()?;
            self.y = Some(OutFile::from_fd(fd));
            self.tempzip = Some(name);
        }
        let seekable = self.y.as_ref().map(|y| y.seekable).unwrap_or(false);
        self.output_seekable = seekable;
        if !seekable {
            self.use_descriptors = true;
        }
        // O que vem antes do zip (um sfx) é copiado.
        if !to_stdout && !d {
            if self.zipbeg != 0 {
                if self.ensure_in_file().is_err() {
                    let ip = self.in_path.clone();
                    return Err(self.ziperr(ZE_ABORT, &format!("could not open archive to read: {}", String::from_utf8_lossy(&ip))));
                }
                let zb = self.zipbeg;
                let r = self.bfcopy(0, zb)?;
                if r != ZE_OK {
                    let t = self.tempzip.clone().unwrap_or_default();
                    let zf = self.zipfile.clone();
                    let name = if r == ZE_TEMP { t } else { zf };
                    return Err(self.ziperr(r, &String::from_utf8_lossy(&name)));
                }
            }
            if let Some(f) = self.in_file.take() {
                f.close();
            }
            self.tempzn = self.zipbeg;
        }

        let mut o = false;
        if self.zfiles.is_empty() == false && self.show_sd {
            self.sd("Going through old zip file");
        }
        let old = std::mem::take(&mut self.zfiles);
        let mut kept: Vec<Zlist> = Vec::new();
        for mut z in old {
            if z.mark == 1 {
                let len: u64 = if (z.len as i64) == -1 { 0 } else { z.len };
                if self.action != ARCHIVE && self.action != DELETE {
                    if self.verbose > 0 || !(self.filesync && z.current) {
                        self.display_running_stats();
                    }
                    let mut m = Vec::new();
                    if self.action == FRESHEN {
                        m.extend_from_slice(b"freshening: ");
                        m.extend_from_slice(&z.oname);
                    } else if self.filesync && z.current {
                        if self.verbose > 0 {
                            m.extend_from_slice(b"      ok: ");
                            m.extend_from_slice(&z.oname);
                        }
                    } else {
                        m.extend_from_slice(b"updating: ");
                        m.extend_from_slice(&z.oname);
                    }
                    if !m.is_empty() {
                        self.say(&m);
                    }
                    match self.readlocal(&z)? {
                        Err(_) => {
                            self.zipwarn("could not read local entry information: ", &z.oname);
                            z.lflg = z.flg;
                            z.extra.clear();
                        }
                        Ok(lz) => {
                            z.lflg = lz.lflg;
                            z.extra = lz.extra;
                        }
                    }
                    let mut r = ZE_OK;
                    if !(self.filesync && z.current) {
                        r = self.zipup(&mut z)?;
                    }
                    if self.filesync && z.current {
                        let rc = self.zipcopy(&mut z)?;
                        if rc != ZE_OK {
                            let msg = format!("was copying {}", String::from_utf8_lossy(&z.oname));
                            return Err(self.ziperr(rc, &msg));
                        }
                        self.end_line();
                    }
                    if r == ZE_OPEN || r == ZE_MISS {
                        o = true;
                        self.end_line();
                        if r == ZE_OPEN {
                            if let Some(e) = self.last_errno {
                                let mut l = z.oname.clone();
                                l.extend_from_slice(format!(": {}\n", e.message()).as_bytes());
                                self.stderr_raw(&l);
                            }
                            self.zipwarn("could not open for reading: ", &z.oname);
                            if self.bad_open_is_error {
                                let msg = format!("was zipping {}", String::from_utf8_lossy(&z.name));
                                return Err(self.ziperr(r, &msg));
                            }
                        } else {
                            self.zipwarn("file and directory with the same name: ", &z.oname);
                        }
                        self.zipwarn("will just copy entry over: ", &z.oname);
                        let rc = self.zipcopy(&mut z)?;
                        if rc != ZE_OK {
                            let msg = format!("was copying {}", String::from_utf8_lossy(&z.oname));
                            return Err(self.ziperr(rc, &msg));
                        }
                        z.mark = 0;
                    }
                    self.files_so_far += 1;
                    self.good_bytes_so_far += z.len;
                    self.bytes_so_far += len;
                    kept.push(z);
                } else if self.action == ARCHIVE {
                    self.display_running_stats();
                    let mut m = b" copying: ".to_vec();
                    m.extend_from_slice(&z.oname);
                    if self.display_usize {
                        m.extend_from_slice(format!(" ({})", display_num(z.len)).as_bytes());
                    }
                    self.say(&m);
                    let r = self.zipcopy(&mut z)?;
                    if r != ZE_OK {
                        let msg = format!("was copying {}", String::from_utf8_lossy(&z.oname));
                        self.zipwarn("(try -F to attempt to fix)", "");
                        return Err(self.ziperr(r, &msg));
                    }
                    if self.noisy && self.mesg_line_started {
                        self.mesg_raw(b"\n");
                        self.mesg_line_started = false;
                    }
                    if self.logall && self.logfile_line_started {
                        self.log_raw(b"\n");
                        self.logfile_line_started = false;
                    }
                    self.files_so_far += 1;
                    self.good_bytes_so_far += z.siz;
                    self.bytes_so_far += z.siz;
                    kept.push(z);
                } else {
                    self.display_running_stats();
                    let mut m = b"deleting: ".to_vec();
                    m.extend_from_slice(&z.oname);
                    if self.display_usize {
                        m.extend_from_slice(format!(" ({})", display_num(z.len)).as_bytes());
                    }
                    m.push(b'\n');
                    if self.noisy {
                        self.mesg_raw(&m);
                    }
                    if self.logall {
                        self.log_raw(&m);
                    }
                    self.files_so_far += 1;
                    self.good_bytes_so_far += z.siz;
                    self.bytes_so_far += z.siz;
                }
            } else if self.action == ARCHIVE {
                // Não selecionada na cópia: some.
            } else {
                if self.filesync {
                    self.blank_running_stats();
                    let mut m = b"deleting: ".to_vec();
                    m.extend_from_slice(&z.oname);
                    if self.display_usize {
                        m.extend_from_slice(format!(" ({})", display_num(z.len)).as_bytes());
                    }
                    m.push(b'\n');
                    if self.noisy {
                        self.mesg_raw(&m);
                        self.mesg_line_started = false;
                    }
                    if self.logall {
                        self.log_raw(&m);
                        self.logfile_line_started = false;
                    }
                } else if !d && !self.diff_mode {
                    let r = self.zipcopy(&mut z)?;
                    if r != ZE_OK {
                        let msg = format!("was copying {}", String::from_utf8_lossy(&z.oname));
                        return Err(self.ziperr(r, &msg));
                    }
                }
                kept.push(z);
            }
        }
        self.zfiles = kept;

        // Os arquivos novos.
        if self.show_sd {
            self.sd("Zipping up new entries");
        }
        let found = std::mem::take(&mut self.found);
        for f in found {
            let mut z = Zlist {
                name: f.name.clone(),
                iname: f.iname.clone(),
                zname: f.zname.clone(),
                oname: f.oname.clone(),
                mark: 1,
                dosflag: f.dosflag,
                ..Zlist::default()
            };
            if let Some(u) = &f.uname {
                if !is_ascii(u) {
                    z.uname = Some(u.clone());
                }
            }
            self.display_running_stats();
            let mut m = b"  adding: ".to_vec();
            m.extend_from_slice(&z.oname);
            self.say(&m);
            let len = f.usize;
            let r = self.zipup(&mut z)?;
            if r == ZE_OPEN || r == ZE_MISS {
                o = true;
                self.end_line();
                if r == ZE_OPEN {
                    if let Some(e) = self.last_errno {
                        let l = format!("zip warning: {}\n", e.message());
                        self.stderr_raw(l.as_bytes());
                        self.log_raw(l.as_bytes());
                    }
                    self.zipwarn("could not open for reading: ", &z.oname);
                    if self.bad_open_is_error {
                        let msg = format!("was zipping {}", String::from_utf8_lossy(&z.name));
                        return Err(self.ziperr(r, &msg));
                    }
                } else {
                    self.zipwarn("file and directory with the same name: ", &z.oname);
                }
                self.files_so_far += 1;
                self.bytes_so_far += len;
                self.bad_files_so_far += 1;
                self.bad_bytes_so_far += len;
            } else {
                self.files_so_far += 1;
                self.good_bytes_so_far += z.len;
                self.bytes_so_far += len;
                self.zfiles.push(z);
            }
        }
        self.key = None;
        self.keys = None;

        if self.noisy && self.bad_files_so_far > 0 {
            let l = format!(
                "\nzip warning: Not all files were readable\n  files/entries read:  {} ({} bytes)  skipped:  {} ({} bytes)\n",
                self.files_total - self.bad_files_so_far,
                super::zipup::write_num_string(self.good_bytes_so_far),
                self.bad_files_so_far,
                super::zipup::write_num_string(self.bad_bytes_so_far)
            );
            self.mesg_raw(l.as_bytes());
        }
        if self.logfile.is_some() && self.bad_files_so_far > 0 {
            let l = format!(
                "\nzip warning: Not all files were readable\n  files/entries read:  {} ({} bytes)  skipped:  {} ({} bytes)",
                self.files_total - self.bad_files_so_far,
                super::zipup::write_num_string(self.good_bytes_so_far),
                self.bad_files_so_far,
                super::zipup::write_num_string(self.bad_bytes_so_far)
            );
            self.log_raw(l.as_bytes());
        }

        self.read_comments()?;

        if self.display_globaldots {
            self.mesg_raw(b"\n");
            self.mesg_line_started = false;
        }

        // O diretório central e o fim.
        if self.show_sd {
            self.sd("Writing central directory");
        }
        let mut k: u64 = 0;
        let c = self.tempzn;
        let (mut n, mut t) = (0u64, 0u64);
        let nz = self.zfiles.len();
        for i in 0..nz {
            if self.zfiles[i].mark != 0 || !(self.diff_mode || self.filesync) {
                let mut z = std::mem::take(&mut self.zfiles[i]);
                let res = self.putcentral(&mut z);
                self.tempzn += 4 + CENHEAD as u64 + z.iname.len() as u64 + z.cextra.len() as u64 + z.comment.len() as u64;
                n += z.len;
                t += z.siz;
                self.zfiles[i] = z;
                res?;
                k += 1;
            }
        }
        if k == 0 {
            self.zipwarn("zip file empty", "");
        }
        if self.verbose > 0 {
            let l = format!("total bytes={}, compressed={} -> {}% savings\n", n, t, percent(n, t));
            self.mesg_raw(l.as_bytes());
        }
        if self.logall {
            let l = format!("total bytes={}, compressed={} -> {}% savings\n", n, t, percent(n, t));
            self.log_raw(l.as_bytes());
        }
        let t = self.tempzn - c;
        if self.show_sd {
            self.sd("Writing end of central directory");
        }
        let zc = self.zcomment.clone();
        self.putend(k, t, c, &zc)?;
        self.close_out(if d { ZE_WRITE } else { ZE_TEMP })?;
        if let Some(f) = self.in_file.take() {
            f.close();
        }

        let tz = self.tempzip.clone().unwrap_or_default();
        if self.test {
            self.check_zipfile(&tz)?;
        }
        if !to_stdout && !d {
            if self.show_sd {
                self.sd("Replacing old zip file");
            }
            let op = self.out_path.clone();
            let r = self.replace(&op, &tz);
            if r != ZE_OK {
                self.zipwarn("new zip file left as: ", &tz);
                self.tempzip = None;
                return Err(self.ziperr(r, "was replacing the original zip file"));
            }
        }
        self.tempzip = None;
        if self.zip_attributes != 0 && !to_stdout {
            let op = self.out_path.clone();
            let _ = sys::current().fchmodat(Fd::CWD, &op, self.zip_attributes & 0o7777, sysabi::AtFlags::empty());
        }
        if self.logfile.is_some() {
            let mut l = format!("\nTotal {} entries (", self.files_total);
            if self.good_bytes_so_far != self.bytes_total {
                l.push_str(&format!("planned {} bytes, actual {} bytes)", display_num(self.bytes_total), display_num(self.good_bytes_so_far)));
            } else {
                l.push_str(&format!("{} bytes)", display_num(self.bytes_total)));
            }
            l.push_str(&format!("\nDone {}\n", crate::tz::format(self.now_sec(), 0, &self.tz, "%a %b %e %H:%M:%S %Y")));
            self.log_raw(l.as_bytes());
        }
        self.finish(if o { ZE_OPEN } else { ZE_OK })
    }

    /// Os comentários: um por entrada (`-c`) e o do zip (`-z`).
    fn read_comments(&mut self) -> R<()> {
        let comment_fd = if self.comment_stdin { Fd::STDIN } else { Fd::STDERR };
        if self.show_sd {
            self.sd("Get comment if any");
        }
        if self.comadd {
            for i in 0..self.zfiles.len() {
                if self.zfiles[i].mark != 0 {
                    if self.noisy {
                        let mut l = b"Enter comment for ".to_vec();
                        l.extend_from_slice(&self.zfiles[i].oname);
                        l.extend_from_slice(b":\n");
                        self.mesg_raw(&l);
                    }
                    if let Some(mut e) = self.fgets(comment_fd, MAXCOM + 1) {
                        if e.last() == Some(&b'\n') {
                            e.pop();
                        }
                        self.zfiles[i].comment = e;
                    }
                }
            }
        }
        if self.zipedit {
            if self.noisy && !self.zcomment.is_empty() {
                self.mesg_raw(b"current zip file comment is:\n");
                let zc = self.zcomment.clone();
                self.mesg_raw(&zc);
                if zc.last() != Some(&b'\n') {
                    self.mesg_raw(b"\n");
                }
            }
            self.zcomment = Vec::new();
            if self.noisy {
                self.mesg_raw(b"enter new zip file comment (end with .):\n");
            }
            while let Some(mut e) = self.fgets(comment_fd, MAXCOM + 1) {
                if e == b".\n" {
                    break;
                }
                if e.last() == Some(&b'\n') {
                    e.pop();
                }
                if !self.zcomment.is_empty() {
                    self.zcomment.extend_from_slice(b"\r\n");
                    self.zcomment.extend_from_slice(&e);
                } else if !e.is_empty() {
                    self.zcomment = e;
                } else {
                    self.zcomment = b"\r\n".to_vec();
                }
            }
        }
        let _ = display_name;
        Ok(())
    }
}
