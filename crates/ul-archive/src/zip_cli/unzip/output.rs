//! A saída de um membro: o `flush` do fileio.c (CRC, conversão de fim de linha do `-a`, escrita no
//! arquivo ou no stdout do `-c`/`-p`), a pergunta de disco cheio, e a abertura e o fechamento do
//! arquivo de saída (`open_outfile` do fileio.c, `close_outfile` do unix.c).

use sysabi::{AtFlags, Fd, OFlags};

use super::fileio::fnfilter;
use super::unix::{perror, set_times, strerror, Slink};
use super::{sys, Uz, PK_DISK, PK_OK};

const CR: u8 = b'\r';
const LF: u8 = b'\n';
const CTRLZ: u8 = 0x1a;

impl Uz {
    /// Os bytes descomprimidos de um pedaço do membro (`flush`): soma no CRC e, fora do teste,
    /// escreve, convertendo os fins de linha no modo texto (CR/LF, CR sozinho e LF viram LF; os ^Z
    /// somem).
    pub fn flush(&mut self, raw: &[u8]) -> i32 {
        self.x.crc.update(raw);
        if self.o.tflag != 0 || raw.is_empty() {
            return PK_OK;
        }
        if self.x.disk_full != 0 {
            return PK_DISK;
        }
        if !self.pinfo.textmode {
            return self.write_out(raw);
        }
        if self.x.newfile {
            self.x.did_cr_last = false;
            self.x.newfile = false;
        }
        let mut p = 0;
        if raw[0] == LF && self.x.did_cr_last {
            p = 1;
        }
        self.x.did_cr_last = false;
        let mut out = Vec::with_capacity(raw.len());
        while p < raw.len() {
            match raw[p] {
                CR => {
                    out.push(LF);
                    if p == raw.len() - 1 {
                        self.x.did_cr_last = true;
                    } else if raw[p + 1] == LF {
                        p += 1;
                    }
                }
                LF => out.push(LF),
                CTRLZ => {}
                c => out.push(c),
            }
            p += 1;
        }
        if out.is_empty() {
            return PK_OK;
        }
        self.write_out(&out)
    }

    /// Escreve no arquivo de saída (o `write` cru, sem stdio) ou, com `-c`/`-p`, pelo `Info` sem
    /// flags, que mantém o controle de começo de linha.
    fn write_out(&mut self, data: &[u8]) -> i32 {
        if self.o.cflag {
            self.info(0, data);
            return PK_OK;
        }
        let Some(fd) = self.x.outfile else { return PK_OK };
        // O `WriteError` compara o retorno de um único `write` com o tamanho.
        match sys().write(fd, data) {
            Ok(n) if n == data.len() => PK_OK,
            _ => self.disk_error(),
        }
    }

    /// `"NOME:  write error (disk full?).  Continue? (y/n/^C) "`: com "y" segue pros outros membros,
    /// senão para. No fim do stdin vale a resposta anterior, como no `fgets` que não toca no buffer.
    fn disk_error(&mut self) -> i32 {
        let mut m = fnfilter(&self.filename);
        m.extend_from_slice(b":  write error (disk full?).  Continue? (y/n/^C) ");
        self.info(0x4a1, m);
        if let Some(a) = self.fgets(10) {
            self.x.answerbuf = a;
        }
        self.x.disk_full = if self.x.answerbuf.first() == Some(&b'y') { 1 } else { 2 };
        PK_DISK
    }

    /// Abre o arquivo de saída (`open_outfile`). Um que já existe sai do caminho antes: com `-B`
    /// vira backup ("~", e depois "~1", "~2"... se o backup também existir), senão é apagado. O
    /// arquivo nasce com 0600 (o `umask(0077)` do C) e com leitura, pra que o link simbólico possa
    /// ser relido. `true` é falha.
    pub fn open_outfile(&mut self) -> bool {
        use super::fileio::FILNAMSIZ;
        let s = sys();
        let exists = sysabi::sys::stat(&self.filename).is_ok() || sysabi::sys::lstat(&self.filename).is_ok();
        if exists {
            if self.o.b_flag {
                let suffix = b"~";
                let mut flen = self.filename.len();
                let mut tlen = flen + suffix.len() + 6;
                let mut tname = self.filename.clone();
                if tlen >= FILNAMSIZ {
                    tlen = FILNAMSIZ - 1 - suffix.len();
                    tname.truncate(tlen);
                    flen = flen.min(tlen);
                    tlen = FILNAMSIZ;
                }
                tname.truncate(flen);
                tname.extend_from_slice(suffix);
                if self.x.overwrite_mode == super::extract::Overwrite::Always {
                    if sysabi::sys::stat(&tname).is_ok() {
                        let _ = s.unlinkat(Fd::CWD, &tname, AtFlags::empty());
                    }
                } else {
                    let maxtail: u32 = match tlen as i64 - flen as i64 - suffix.len() as i64 - 1 {
                        4 => 9999,
                        3 => 999,
                        2 => 99,
                        1 => 9,
                        0 => 0,
                        _ => 99999,
                    };
                    let base = tname.len();
                    let mut i = 0;
                    while i < maxtail && sysabi::sys::stat(&tname).is_ok() {
                        i += 1;
                        tname.truncate(base);
                        tname.extend_from_slice(i.to_string().as_bytes());
                    }
                }
                if let Err(e) = s.renameat2(Fd::CWD, &self.filename, Fd::CWD, &tname, sysabi::RenameFlags::empty()) {
                    let mut m = b"error:  cannot rename old ".to_vec();
                    m.extend(fnfilter(&self.filename));
                    m.extend_from_slice(format!("\n        {}\n", strerror(e)).as_bytes());
                    self.info(0x401, m);
                    return true;
                }
            } else if let Err(e) = s.unlinkat(Fd::CWD, &self.filename, AtFlags::empty()) {
                let mut m = b"error:  cannot delete old ".to_vec();
                m.extend(fnfilter(&self.filename));
                m.extend_from_slice(format!("\n        {}\n", strerror(e)).as_bytes());
                self.info(0x401, m);
                return true;
            }
        }
        let saved = s.umask(0o077);
        let r = s.openat(Fd::CWD, &self.filename, OFlags::RDWR | OFlags::CREAT | OFlags::TRUNC, 0o666);
        s.umask(saved);
        match r {
            Ok(fd) => {
                self.x.outfile = Some(fd);
                false
            }
            Err(e) => {
                let mut m = b"error:  cannot create ".to_vec();
                m.extend(fnfilter(&self.filename));
                m.extend_from_slice(format!("\n        {}\n", strerror(e)).as_bytes());
                self.info(0x401, m);
                true
            }
        }
    }

    /// Fecha o arquivo de saída (`close_outfile` do unix.c). Um link simbólico tem os dados relidos
    /// e vira entrada adiada (o arquivo fica como marcador); um arquivo comum ganha dono (`-X`),
    /// permissões e datas.
    pub fn close_outfile(&mut self) {
        let s = sys();
        let Some(fd) = self.x.outfile.take() else { return };
        let (mtime, atime, uidgid) = self.get_extattribs();
        if self.x.symlnk {
            let ucsize = self.lrec.ucsize as usize;
            let mut target = vec![0u8; ucsize];
            let mut got = 0;
            while got < ucsize {
                match s.pread(fd, &mut target[got..], got as u64) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => got += n,
                }
            }
            let _ = s.close(fd);
            if got != ucsize {
                let mut m = b"warning:  symbolic link (".to_vec();
                m.extend(fnfilter(&self.filename));
                m.extend_from_slice(b") failed\n");
                self.info(0x201, m);
                return;
            }
            if self.o.qflag == 0 {
                let shown = &target[..target.iter().position(|&c| c == 0).unwrap_or(target.len())];
                let mut m = b"-> ".to_vec();
                m.extend(fnfilter(shown));
                m.push(b' ');
                self.info(0, m);
            }
            self.x.slinks.push(Slink { fname: self.filename.clone(), target, perms: self.pinfo.file_attr, uidgid });
            return;
        }
        if let Some((uid, gid)) = uidgid {
            if uid <= u64::from(u32::MAX) && gid <= u64::from(u32::MAX) {
                if let Err(e) = s.fchownat(fd, b"", Some(uid as u32), Some(gid as u32), AtFlags::EMPTY_PATH) {
                    let m = if self.o.qflag != 0 {
                        let mut m = format!("warning:  cannot set UID {uid} and/or GID {gid} for ").into_bytes();
                        m.extend(fnfilter(&self.filename));
                        m.extend_from_slice(format!("\n          {}\n", strerror(e)).as_bytes());
                        m
                    } else {
                        format!(" (warning) cannot set UID {uid} and/or GID {gid}\n          {}", strerror(e)).into_bytes()
                    };
                    self.info(0x201, m);
                }
            }
        }
        if let Err(e) = s.fchmod(fd, self.filtattr(self.pinfo.file_attr)) {
            perror("fchmod (file attributes) error", e);
        }
        let _ = s.close(fd);
        if self.o.d_flag <= 1 {
            if let Err(e) = set_times(&self.filename, atime, mtime, AtFlags::empty()) {
                let m = if self.o.qflag != 0 {
                    let mut m = b"warning:  cannot set modif./access times for ".to_vec();
                    m.extend(fnfilter(&self.filename));
                    m.extend_from_slice(format!("\n          {}\n", strerror(e)).as_bytes());
                    m
                } else {
                    format!(" (warning) cannot set modif./access times\n          {}", strerror(e)).into_bytes()
                };
                self.info(0x201, m);
            }
        }
    }
}
