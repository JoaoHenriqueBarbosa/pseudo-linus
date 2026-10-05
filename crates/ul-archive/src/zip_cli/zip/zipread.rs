//! Leitura do arquivo zip existente (zipfile.c): fim do diretório central, Zip64, entradas,
//! nomes Unicode e `readlocal`.

use sysabi::Errno;

use super::consts::*;
use super::crypt::crc32_update;
use super::extra::*;
use super::in_scan::{find_next_signature, find_signature};
use super::names::{display_name, ex2in};
use super::state::{R, Zip, Zlist};

impl Zip {
    /// Abre o zip antigo se ainda não está aberto.
    pub fn ensure_in_file(&mut self) -> Result<(), Errno> {
        if self.in_file.is_none() {
            match super::out::InFile::open(&self.in_path) {
                Ok(f) => self.in_file = Some(f),
                Err(e) => {
                    self.last_errno = Some(e);
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// `read_Unicode_Path_entry` / `read_Unicode_Path_local_entry`: lê o campo extra 0x7075, confere
    /// o crc do nome local e devolve o nome UTF-8 (ou `None`).
    fn unicode_path_from(&mut self, extra: &[u8], iname: &[u8], name: &[u8], oname: &[u8]) -> R<Option<Vec<u8>>> {
        let Some(mut p) = get_extra_field(UTF8_PATH_EF_TAG, extra) else { return Ok(None) };
        p += 2;
        let elen = sh(extra, p) as usize;
        p += 2;
        let version = extra.get(p).copied().unwrap_or(0);
        p += 1;
        if version > 1 {
            self.zipwarn("Unicode Path Extra Field version > 1 - skipping", oname);
            return Ok(None);
        }
        let iname_chksum = lg(extra, p);
        p += 4;
        let chksum = crc32_update(0, iname);
        if chksum != iname_chksum {
            if self.unicode_mismatch == 1 {
                self.zipwarn("Unicode does not match path - ignoring Unicode: ", oname);
            } else if self.unicode_mismatch == 0 {
                let mut msg = b"Unicode does not match path:  ".to_vec();
                msg.extend_from_slice(oname);
                msg.extend_from_slice(b"\n                     Likely entry name changed but Unicode not updated\n                     Use -UN=i to ignore errors or n for no Unicode paths");
                self.zipwarn(msg, "");
                return Err(self.ziperr(ZE_FORM, "Unicode path error"));
            }
            return Ok(None);
        }
        let ulen = elen.saturating_sub(5);
        if ulen == 0 {
            Ok(Some(name.to_vec()))
        } else {
            let end = (p + ulen).min(extra.len());
            Ok(Some(extra[p.min(end)..end].to_vec()))
        }
    }

    pub fn read_unicode_path_local_entry(&mut self, z: &mut Zlist) -> R<()> {
        let (extra, iname, name, oname) = (z.extra.clone(), z.iname.clone(), z.name.clone(), z.oname.clone());
        z.uname = self.unicode_path_from(&extra, &iname, &name, &oname)?;
        Ok(())
    }

    /// `readlocal`: o cabeçalho local de `z` no zip antigo, conferido com o central.
    pub fn readlocal(&mut self, z: &Zlist) -> R<Result<Zlist, i32>> {
        if self.ensure_in_file().is_err() {
            let p = self.in_path.clone();
            return Err(self.ziperr(ZE_OPEN, &String::from_utf8_lossy(&p)));
        }
        let head = match self.in_file.as_ref().map(|f| f.read_at(z.off, 4 + LOCHEAD)) {
            Some(Ok(h)) => h,
            Some(Err(e)) => {
                self.last_errno = Some(e);
                self.zipwarn("reading archive fseek: ", e.message());
                return Ok(Err(ZE_READ));
            }
            None => return Ok(Err(ZE_READ)),
        };
        if head.len() < 4 || &head[..4] != b"PK\x03\x04" {
            if let Some(f) = self.in_file.take() {
                f.close();
            }
            self.zipwarn("Did not find entry for ", &z.iname);
            return Ok(Err(ZE_FORM));
        }
        if head.len() < 4 + LOCHEAD {
            self.zipwarn("reading local entry: ", self.strerror());
            return Ok(Err(ZE_EOF));
        }
        let buf = &head[4..];
        let mut locz = Zlist { ver: sh(buf, 0), lflg: sh(buf, 2), how: sh(buf, 4), tim: lg(buf, 6) as u64, crc: lg(buf, 10) as u64, ..Zlist::default() };
        let nam = sh(buf, 22) as usize;
        let ext = sh(buf, 24) as usize;
        let var = match self.in_file.as_ref().map(|f| f.read_at(z.off + 4 + LOCHEAD as u64, nam + ext)) {
            Some(Ok(v)) => v,
            _ => return Ok(Err(ZE_READ)),
        };
        if var.len() < nam + ext {
            return Ok(Err(ZE_EOF));
        }
        locz.iname = var[..nam].to_vec();
        locz.extra = var[nam..].to_vec();
        locz.name = locz.iname.clone();
        if self.unicode_mismatch != 3 {
            self.read_unicode_path_local_entry(&mut locz)?;
        }
        self.zip64_entry = Zip::adjust_zip_local_entry(&mut locz);
        if locz.ver != z.ver {
            let msg = format!("Local Version Needed ({}) does not match CD ({}): ", locz.ver, z.ver);
            self.zipwarn(msg, &z.iname);
        }
        if locz.lflg != z.flg {
            self.zipwarn("Local Entry Flag does not match CD: ", &z.iname);
        }
        if locz.crc != z.crc {
            self.zipwarn("Local Entry CRC does not match CD: ", &z.iname);
        }
        locz.len = z.len;
        locz.siz = z.siz;
        Ok(Ok(locz))
    }

    /// `readzipfile`: lê o diretório central do zip existente, se há, e ordena as entradas por nome
    /// (`zsort`) e por nome Unicode (`zusort`) para `zsearch`.
    pub fn readzipfile(&mut self) -> R<i32> {
        self.zipbeg = 0;
        self.zfiles.clear();
        self.zsort.clear();
        self.zusort.clear();
        self.zcomment.clear();
        self.zipfile_exists = false;
        let mut retval = ZE_OK;
        let mut readable = !self.zipfile.is_empty() && self.zipfile != b"-";
        if readable {
            match sysabi::sys::open(&self.zipfile, sysabi::OFlags::RDONLY | sysabi::OFlags::CLOEXEC, 0) {
                Ok(fd) => {
                    let _ = sysabi::sys::close(fd);
                }
                Err(e) => {
                    self.last_errno = Some(e);
                    readable = false;
                }
            }
        }
        if !readable {
            if !self.zip_to_stdout && self.fix != 2 && self.in_path != self.out_path {
                let zf = self.zipfile.clone();
                return Err(self.ziperr(ZE_OPEN, &String::from_utf8_lossy(&zf)));
            }
        } else {
            self.zipfile_exists = true;
        }
        if readable {
            retval = self.scanzipf_regnew()?;
        }
        if self.fix != 2 && readable && !self.zfiles.is_empty() {
            let mut zs: Vec<usize> = (0..self.zfiles.len()).collect();
            zs.sort_by(|&a, &b| self.zfiles[a].iname.cmp(&self.zfiles[b].iname));
            let mut zu: Vec<usize> = (0..self.zfiles.len()).collect();
            zu.sort_by(|&a, &b| {
                let (x, y) = (&self.zfiles[a], &self.zfiles[b]);
                x.zuname.as_deref().unwrap_or(&x.iname).cmp(y.zuname.as_deref().unwrap_or(&y.iname))
            });
            self.zsort = zs;
            self.zusort = zu;
        }
        Ok(retval)
    }

    /// `scanzipf_regnew`: o diretório central de um zip de um disco só.
    fn scanzipf_regnew(&mut self) -> R<i32> {
        let inf = match super::out::InFile::open(&self.in_path) {
            Ok(f) => f,
            Err(e) => {
                self.last_errno = Some(e);
                self.zipwarn("could not open input archive", self.in_path.clone());
                return Ok(ZE_OPEN);
            }
        };
        let size = inf.size;
        // O fim do diretório central está nos últimos 64K + 22 bytes; olha os últimos 128K.
        let tail_start = size.saturating_sub(0x20000);
        let tail = match inf.read_at(tail_start, (size - tail_start) as usize) {
            Ok(t) => t,
            Err(e) => {
                self.last_errno = Some(e);
                self.zipwarn("unable to seek in input file ", self.in_path.clone());
                inf.close();
                return Ok(ZE_READ);
            }
        };
        let mut pos = 0usize;
        let mut eocdr_pos: Option<usize> = None;
        while find_signature(&tail, &mut pos, b"PK\x05\x06") {
            eocdr_pos = Some(pos);
        }
        let Some(eocdr_rel) = eocdr_pos else {
            inf.close();
            if self.fix == 1 {
                self.zipwarn("bad archive - missing end signature", "");
                self.zipwarn("(If downloaded, was binary mode used?  If not, the", "");
                self.zipwarn(" archive may be scrambled and not recoverable)", "");
                self.zipwarn("Can't use -F to fix (try -FF)", "");
            } else {
                self.zipwarn("missing end signature--probably not a zip file (did you", "");
                self.zipwarn("remember to use binary mode when you transferred it?)", "");
                self.zipwarn("(if you are trying to read a damaged archive try -F)", "");
            }
            return Ok(ZE_FORM);
        };
        // `eocdr_offset` do C: a posição logo depois da assinatura.
        let eocdr_offset = tail_start + eocdr_rel as u64;
        let mut scbuf = tail[eocdr_rel..(eocdr_rel + ENDHEAD).min(tail.len())].to_vec();
        scbuf.resize(ENDHEAD, 0);
        let eocdr_disk = sh(&scbuf, 0) as u64;
        let mut total_disks = eocdr_disk + 1;
        let mut in_cd_start_disk = sh(&scbuf, 2) as u64;
        let mut in_cd_start_offset = lg(&scbuf, 12) as u64;
        let mut cd_total_entries = sh(&scbuf, 6) as u64;
        let cd_total_size = lg(&scbuf, 8) as u64;
        let zcomlen = sh(&scbuf, 16) as usize;
        if zcomlen > 0 {
            let cstart = eocdr_rel + ENDHEAD;
            if cstart + zcomlen > tail.len() {
                inf.close();
                return Ok(ZE_EOF);
            }
            self.zcomment = tail[cstart..cstart + zcomlen].to_vec();
        }
        if cd_total_entries == 0 {
            inf.close();
            return Ok(ZE_OK);
        }
        if total_disks != 1 {
            if self.adjust {
                self.zipwarn("Adjusting split archives not yet supported", "");
                inf.close();
                return Ok(ZE_FORM);
            }
            self.zipwarn("split archives are not supported: ", self.in_path.clone());
            inf.close();
            return Ok(ZE_FORM);
        }
        if self.fix == 1 && self.in_path == self.out_path {
            inf.close();
            self.zipwarn("must use --out when fixing an archive", "");
            return Ok(ZE_PARMS);
        }
        let mut adjust_offset: u64 = 0;
        // -A: acha o começo real do diretório central quando há um prefixo (sfx) antes do zip.
        if self.adjust {
            if in_cd_start_offset != 0xFFFF_FFFF && cd_total_size != 0xFFFF_FFFF {
                let back = cd_total_size + 24 + 56;
                if eocdr_offset < back {
                    inf.close();
                    self.zipwarn("reading archive fseek: ", Errno::EINVAL.message());
                    return Ok(ZE_FORM);
                }
                let cd_start = eocdr_offset - back;
                let region = inf.read_at(cd_start, (size - cd_start) as usize).unwrap_or_default();
                let mut p = 0usize;
                if find_signature(&region, &mut p, b"PK\x01\x02") {
                    adjust_offset = cd_start + p as u64 - 4 - in_cd_start_offset;
                } else {
                    self.zipwarn("central dir not where expected - could not adjust offsets", "");
                    self.zipwarn("(try -FF)", "");
                    inf.close();
                    return Ok(ZE_FORM);
                }
            } else {
                self.zipwarn("Adjusting a Zip64 archive is not supported", "");
                inf.close();
                return Ok(ZE_FORM);
            }
            if self.noisy {
                let m = if adjust_offset != 0 {
                    format!("Zip entry offsets appear off by {} bytes - correcting...", adjust_offset)
                } else {
                    "Zip entry offsets do not need adjusting".to_string()
                };
                self.zipmessage(m, "");
            }
        }

        // O localizador do Zip64 EOCD fica 20 bytes antes do EOCD.
        if eocdr_offset >= 24 {
            let loc = inf.read_at(eocdr_offset - 24, 4 + EC64LOC).unwrap_or_default();
            if loc.len() >= 4 && &loc[..4] == b"PK\x06\x07" {
                let z64eocdl_offset = eocdr_offset - 24;
                if loc.len() < 4 + EC64LOC {
                    self.zipwarn("reading archive: ", self.strerror());
                    inf.close();
                    return Ok(ZE_READ);
                }
                let z64eocdr_disk = lg(&loc, 4) as u64;
                let mut z64eocdr_offset = llg(&loc, 8) + adjust_offset;
                total_disks = lg(&loc, 16) as u64;
                if z64eocdr_disk != total_disks - 1 {
                    self.zipwarn("split archives are not supported: ", self.in_path.clone());
                    inf.close();
                    return Ok(ZE_FORM);
                }
                let mut rec = inf.read_at(z64eocdr_offset, 4).unwrap_or_default();
                if rec.len() < 4 || &rec[..4] != b"PK\x06\x06" {
                    // Não estava onde devia: calcula a partir do localizador.
                    let start = z64eocdl_offset.saturating_sub(24 + 56);
                    let region = inf.read_at(start, (size - start) as usize).unwrap_or_default();
                    let mut p = 0usize;
                    let mut found = false;
                    if let Some(sig) = find_next_signature(&region, &mut p) {
                        if &sig == b"PK\x06\x06" {
                            found = true;
                        }
                    }
                    if found {
                        let adj = start + p as u64 - 4;
                        adjust_offset = adj.wrapping_sub(z64eocdr_offset);
                        z64eocdr_offset = adj;
                        self.zipwarn("Zip64 EOCDR not found where expected - compensating", "");
                        self.zipwarn("(try -A to adjust offsets)", "");
                        rec = inf.read_at(z64eocdr_offset, 4).unwrap_or_default();
                        let _ = &rec;
                    } else {
                        inf.close();
                        self.zipwarn("Zip64 End Of Central Directory Record not found:  ", self.in_path.clone());
                        return Ok(ZE_FORM);
                    }
                }
                let body = inf.read_at(z64eocdr_offset + 4, EC64REC).unwrap_or_default();
                if body.len() < EC64REC {
                    self.zipwarn("Zip64 EOCD Record bad or truncated", "");
                    inf.close();
                    return Ok(ZE_FORM);
                }
                let version_needed = sh(&body, 10);
                in_cd_start_disk = lg(&body, 16) as u64;
                cd_total_entries = llg(&body, 28);
                in_cd_start_offset = llg(&body, 44) + adjust_offset;
                if version_needed > 46 {
                    let major = version_needed / 10;
                    let minor = version_needed - major * 10;
                    self.zipwarn(format!("This archive requires version {}.{}", major, minor), "");
                    self.zipwarn("Zip currently only supports up to version 4.6 archives", "");
                    self.zipwarn("(up to 4.5 if bzip2 is not compiled in)", "");
                    self.zipwarn("Try -F to attempt to read anyway", "");
                    inf.close();
                    return Ok(ZE_FORM);
                }
            }
        }
        let _ = in_cd_start_disk;

        in_cd_start_offset += adjust_offset;
        self.cenbeg = in_cd_start_offset;
        let mut zipbegset = false;
        self.zipbeg = 0;
        let region = match inf.read_at(in_cd_start_offset, size.saturating_sub(in_cd_start_offset) as usize) {
            Ok(r) => r,
            Err(e) => {
                self.last_errno = Some(e);
                self.zipwarn("unable to seek in input file ", self.in_path.clone());
                inf.close();
                return Ok(ZE_READ);
            }
        };
        inf.close();
        let mut rp = 0usize;
        let mut zlist: Vec<Zlist> = Vec::new();
        while let Some(sig) = find_next_signature(&region, &mut rp) {
            if &sig == b"PK\x05\x06" || &sig == b"PK\x06\x06" {
                break;
            } else if &sig != b"PK\x01\x02" {
                let m = format!("unexpected signature on disk {} at {}\n", 0, in_cd_start_offset + rp as u64 - 4);
                self.zipwarn(m, "");
                self.zipwarn("archive not in correct format: ", self.in_path.clone());
                self.zipwarn("(try -F to attempt recovery)", "");
                return Ok(ZE_FORM);
            }
            if rp + CENHEAD > region.len() {
                self.zipwarn("reading central directory: ", self.strerror());
                return Ok(ZE_EOF);
            }
            let sb = &region[rp..rp + CENHEAD];
            rp += CENHEAD;
            let nam = sh(sb, 24) as usize;
            let cext = sh(sb, 26) as usize;
            let com = sh(sb, 28) as usize;
            let mut z = Zlist {
                vem: sh(sb, 0),
                ver: sh(sb, 2),
                flg: sh(sb, 4),
                how: sh(sb, 6),
                tim: lg(sb, 8) as u64,
                crc: lg(sb, 12) as u64,
                siz: lg(sb, 16) as u64,
                len: lg(sb, 20) as u64,
                dsk: sh(sb, 30) as u64,
                att: sh(sb, 32),
                atx: lg(sb, 34) as u64,
                off: lg(sb, 38) as u64,
                dosflag: (sh(sb, 0) & 0xff00) == 0,
                ..Zlist::default()
            };
            if nam == 0 {
                let m = (zlist.len() + 1).to_string();
                self.zipwarn("zero-length name for entry #", m);
                return Ok(ZE_FORM);
            }
            if rp + nam + cext + com > region.len() {
                return Ok(ZE_EOF);
            }
            z.iname = region[rp..rp + nam].to_vec();
            z.cextra = region[rp + nam..rp + nam + cext].to_vec();
            z.comment = region[rp + nam + cext..rp + nam + cext + com].to_vec();
            rp += nam + cext + com;
            if self.unicode_mismatch != 3 {
                if z.flg & UTF8_BIT != 0 {
                    z.uname = Some(z.iname.clone());
                    if std::str::from_utf8(&z.iname).is_err() {
                        let u = z.uname.clone().unwrap_or_default();
                        self.zipwarn("illegal UTF-8 name: ", u);
                    }
                } else {
                    let (cx, ini, nm) = (z.cextra.clone(), z.iname.clone(), z.name.clone());
                    let od = display_name(&ini);
                    z.uname = self.unicode_path_from(&cx, &ini, &nm, &od)?;
                }
            }
            Zip::adjust_zip_central_entry(&mut z);
            if self.adjust {
                z.off += adjust_offset;
            }
            if z.dsk == 0 && (!zipbegset || z.off < self.zipbeg) {
                self.zipbeg = z.off;
                zipbegset = true;
            }
            z.mark = 0;
            z.trash = false;
            z.zname = z.iname.clone();
            z.name = z.zname.clone();
            z.oname = display_name(&z.iname);
            if self.unicode_mismatch != 3 {
                if let Some(u) = z.uname.clone() {
                    let name = if std::str::from_utf8(&u).is_ok() { u.clone() } else { z.iname.clone() };
                    z.zuname = Some(name.clone());
                    z.ouname = Some(name);
                }
            }
            zlist.push(z);
        }
        if zlist.len() as u64 != cd_total_entries {
            let m = format!("expected {} entries but found {}", cd_total_entries, zlist.len());
            self.zipwarn(m, "");
            return Ok(ZE_FORM);
        }
        self.zfiles = zlist;
        let _ = ex2in;
        Ok(ZE_OK)
    }
}
