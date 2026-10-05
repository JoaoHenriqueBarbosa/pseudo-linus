//! Escrita do arquivo zip (zipfile.c): cabeçalho local, descritor de dados, cabeçalho central,
//! registro de fim e `zipcopy`, que copia uma entrada do zip antigo.

use super::consts::*;
use super::crypt::crc32_update;
use super::extra::*;
use super::state::{R, Zip, Zlist};

impl Zip {
    /// O nome gravado no cabeçalho: o UTF-8 se o bit 11 está ligado e há nome Unicode.
    fn name_on_disk(z: &Zlist, use_uname: bool) -> Vec<u8> {
        if use_uname { z.uname.clone().unwrap_or_default() } else { z.iname.clone() }
    }

    /// `putlocal`: monta e escreve o cabeçalho local de `z` (ou o reescreve no lugar, com
    /// `PUTLOCAL_REWRITE`, depois que os tamanhos e o crc se firmaram).
    pub fn putlocal(&mut self, z: &mut Zlist, rewrite: i32) -> R<()> {
        let streaming_in = z.name == b"-";
        let mut was_zip64;
        if rewrite == PUTLOCAL_WRITE {
            self.zip64_entry = false;
            was_zip64 = false;
            if z.siz > ZIP_UWORD32_MAX || z.len > ZIP_UWORD32_MAX || self.force_zip64 == 1 || (self.force_zip64 != 0 && streaming_in) {
                if self.force_zip64 == 0 {
                    self.zipwarn("Entry too big:", &z.oname);
                    return Err(self.ziperr(ZE_BIG, "Large entry support disabled with -fz- but needed"));
                }
                self.zip64_entry = true;
                if z.ver < ZIP64_MIN_VER {
                    z.ver = ZIP64_MIN_VER;
                }
                was_zip64 = true;
            }
        } else {
            was_zip64 = self.zip64_entry;
            self.zip64_entry = false;
            if z.siz > ZIP_UWORD32_MAX || z.len > ZIP_UWORD32_MAX || self.force_zip64 == 1 || (self.force_zip64 != 0 && streaming_in) {
                self.zip64_entry = true;
            }
            if self.force_zip64 == 0 && self.zip64_entry {
                self.zipwarn("Entry too big:", &z.oname);
                return Err(self.ziperr(ZE_BIG, "Large entry support disabled with -fz- but entry needs"));
            }
            if !was_zip64 && self.zip64_entry {
                self.zipwarn("Entry too big:", &z.oname);
                if self.force_zip64 == 0 {
                    return Err(self.ziperr(ZE_BIG, "Compressed/stored entry unexpectedly large - do not use -fz-"));
                } else {
                    return Err(self.ziperr(ZE_BIG, "Poor compression resulted in unexpectedly large entry - try -fz"));
                }
            }
            if self.zip64_entry {
                self.zip64_archive = true;
                if z.ver < ZIP64_MIN_VER {
                    z.ver = ZIP64_MIN_VER;
                }
            } else {
                self.zip64_entry = false;
            }
            if was_zip64 && !self.zip64_entry {
                z.ver = 20;
            }
        }
        if self.zip64_entry || was_zip64 {
            Zip::add_local_zip64_extra_field(z);
        }

        let mut use_uname = false;
        if z.uname.is_some() {
            if self.utf8_force || self.using_utf8 {
                z.lflg |= UTF8_BIT;
                z.flg |= UTF8_BIT;
            }
            if z.flg & UTF8_BIT != 0 {
                use_uname = true;
            } else {
                let (ini, un) = (z.iname.clone(), z.uname.clone().unwrap_or_default());
                add_unicode_path_extra(&mut z.extra, &ini, &un);
            }
        } else {
            z.flg &= !UTF8_BIT;
            z.lflg &= !UTF8_BIT;
        }
        let name = Zip::name_on_disk(z, use_uname);

        let mut b: Vec<u8> = Vec::with_capacity(LOCHEAD + 4 + name.len() + z.extra.len());
        put_lg(&mut b, LOCSIG);
        put_sh(&mut b, z.ver);
        put_sh(&mut b, z.lflg);
        put_sh(&mut b, z.how);
        put_lg(&mut b, z.tim as u32);
        put_lg(&mut b, z.crc as u32);
        if self.zip64_entry {
            put_lg(&mut b, 0xFFFF_FFFF);
            put_lg(&mut b, 0xFFFF_FFFF);
        } else {
            put_lg(&mut b, z.siz as u32);
            put_lg(&mut b, z.len as u32);
        }
        put_sh(&mut b, name.len() as u16);
        put_sh(&mut b, z.extra.len() as u16);
        b.extend_from_slice(&name);
        b.extend_from_slice(&z.extra);

        if rewrite == PUTLOCAL_REWRITE {
            let ok = match self.y.as_mut() {
                Some(y) => y.write_at(z.off, &b),
                None => false,
            };
            if !ok {
                self.last_errno = self.y.as_ref().and_then(|y| y.err);
                return Err(self.ziperr(ZE_TEMP, &String::from_utf8_lossy(&self.tempzip.clone().unwrap_or_default())));
            }
        } else {
            self.bfwrite(&b, BFWRITE_LOCALHEADER)?;
        }
        Ok(())
    }

    /// `putextended`: o descritor de dados.
    pub fn putextended(&mut self, z: &Zlist) -> R<()> {
        let mut b: Vec<u8> = Vec::new();
        put_lg(&mut b, EXTLOCSIG);
        put_lg(&mut b, z.crc as u32);
        if self.zip64_entry {
            put_llg(&mut b, z.siz);
            put_llg(&mut b, z.len);
        } else {
            put_lg(&mut b, z.siz as u32);
            put_lg(&mut b, z.len as u32);
        }
        self.bfwrite(&b, BFWRITE_HEADER)?;
        Ok(())
    }

    /// `putcentral`: o cabeçalho central de `z`.
    pub fn putcentral(&mut self, z: &mut Zlist) -> R<()> {
        let mut use_uname = false;
        if z.uname.is_some() {
            if self.utf8_force {
                z.flg |= UTF8_BIT;
            }
            if z.flg & UTF8_BIT != 0 {
                use_uname = true;
            } else {
                let (ini, un) = (z.iname.clone(), z.uname.clone().unwrap_or_default());
                add_unicode_path_extra(&mut z.cextra, &ini, &un);
            }
        } else {
            z.flg &= !UTF8_BIT;
            z.lflg &= !UTF8_BIT;
        }
        let off = z.off;
        if z.siz > ZIP_UWORD32_MAX || z.len > ZIP_UWORD32_MAX || z.off > ZIP_UWORD32_MAX || z.dsk > ZIP_UWORD16_MAX || self.force_zip64 == 1 {
            if let Err(code) = self.add_central_zip64_extra_field(z) {
                let tz = self.tempzip.clone().unwrap_or_default();
                return Err(self.ziperr(code, &String::from_utf8_lossy(&tz)));
            }
        }
        let name = Zip::name_on_disk(z, use_uname);
        let mut b: Vec<u8> = Vec::with_capacity(CENHEAD + 4 + name.len() + z.cextra.len() + z.comment.len());
        put_lg(&mut b, CENSIG);
        put_sh(&mut b, z.vem);
        put_sh(&mut b, z.ver);
        put_sh(&mut b, z.flg);
        put_sh(&mut b, z.how);
        put_lg(&mut b, z.tim as u32);
        put_lg(&mut b, z.crc as u32);
        put_lg(&mut b, if z.siz > ZIP_UWORD32_MAX { 0xFFFF_FFFF } else { z.siz as u32 });
        put_lg(&mut b, if z.len > ZIP_UWORD32_MAX || self.force_zip64 == 1 { 0xFFFF_FFFF } else { z.len as u32 });
        put_sh(&mut b, name.len() as u16);
        put_sh(&mut b, z.cextra.len() as u16);
        put_sh(&mut b, z.comment.len() as u16);
        put_sh(&mut b, if z.dsk > ZIP_UWORD16_MAX { 0xFFFF } else { z.dsk as u16 });
        put_sh(&mut b, z.att);
        put_lg(&mut b, z.atx as u32);
        put_lg(&mut b, if off > ZIP_UWORD32_MAX { 0xFFFF_FFFF } else { off as u32 });
        b.extend_from_slice(&name);
        b.extend_from_slice(&z.cextra);
        b.extend_from_slice(&z.comment);
        self.bfwrite(&b, BFWRITE_CENTRALHEADER)?;
        Ok(())
    }

    /// `putend`: o fim do diretório central (e os registros Zip64 se precisam). `n` entradas, `s`
    /// bytes do diretório, `c` o deslocamento dele e `comment` o comentário do zip.
    pub fn putend(&mut self, n: u64, s: u64, c: u64, comment: &[u8]) -> R<()> {
        let mut b: Vec<u8> = Vec::new();
        let zip64_eocd_disk = self.current_disk;
        let zip64_eocd_offset = self.bytes_this_split;
        if n > ZIP_UWORD16_MAX || s > ZIP_UWORD32_MAX || c > ZIP_UWORD32_MAX || self.zip64_archive {
            put_lg(&mut b, ZIP64_CENTRAL_DIR_TAIL_SIG);
            put_llg(&mut b, ZIP64_CENTRAL_DIR_TAIL_SIZE);
            put_sh(&mut b, VEM_UNIX);
            put_sh(&mut b, ZIP64_MIN_VER);
            put_lg(&mut b, self.current_disk as u32);
            put_lg(&mut b, self.cd_start_disk as u64 as u32);
            put_llg(&mut b, self.cd_entries_this_disk);
            put_llg(&mut b, n);
            put_llg(&mut b, s);
            put_llg(&mut b, self.cd_start_offset);
            put_lg(&mut b, ZIP64_CENTRAL_DIR_TAIL_END_SIG);
            put_lg(&mut b, zip64_eocd_disk as u32);
            put_llg(&mut b, zip64_eocd_offset);
            put_lg(&mut b, (self.current_disk + 1) as u32);
        }
        put_lg(&mut b, ENDSIG);
        put_sh(&mut b, if self.current_disk < 0xFFFF { self.current_disk as u16 } else { 0xFFFF });
        if self.cd_start_disk == -1 {
            self.cd_start_disk = 0;
        }
        put_sh(&mut b, if (self.cd_start_disk as u64) < 0xFFFF { self.cd_start_disk as u16 } else { 0xFFFF });
        put_sh(&mut b, if self.cd_entries_this_disk < 0xFFFF { self.cd_entries_this_disk as u16 } else { 0xFFFF });
        put_sh(&mut b, if self.total_cd_entries < 0xFFFF { self.total_cd_entries as u16 } else { 0xFFFF });
        put_lg(&mut b, if s > ZIP_UWORD32_MAX { 0xFFFF_FFFF } else { s as u32 });
        put_lg(&mut b, if self.force_zip64 == 1 || self.cd_start_offset > ZIP_UWORD32_MAX { 0xFFFF_FFFF } else { self.cd_start_offset as u32 });
        put_sh(&mut b, comment.len() as u16);
        b.extend_from_slice(comment);
        self.bfwrite(&b, BFWRITE_HEADER)?;
        Ok(())
    }

    /// `bfcopy`: copia `n` bytes do zip antigo, a partir de `pos`, para a saída como dados.
    pub fn bfcopy(&mut self, mut pos: u64, n: u64) -> R<i32> {
        let mut m: u64 = 0;
        while m < n {
            let brd = (n - m).min(CBSZ as u64) as usize;
            let chunk = match self.in_file.as_ref().map(|f| f.read_at(pos, brd)) {
                Some(Ok(c)) => c,
                Some(Err(e)) => {
                    self.last_errno = Some(e);
                    return Ok(ZE_READ);
                }
                None => return Ok(ZE_READ),
            };
            if chunk.is_empty() {
                break;
            }
            self.bfwrite(&chunk, BFWRITE_DATA)?;
            pos += chunk.len() as u64;
            m += chunk.len() as u64;
        }
        Ok(ZE_OK)
    }

    /// `zipcopy`: copia a entrada `z` do zip antigo para a saída, refazendo o cabeçalho local.
    pub fn zipcopy(&mut self, z: &mut Zlist) -> R<i32> {
        if self.ensure_in_file().is_err() {
            let p = self.in_path.clone();
            return Err(self.ziperr(ZE_OPEN, &String::from_utf8_lossy(&p)));
        }
        let start_offset = z.off;
        let head = match self.in_file.as_ref().map(|f| f.read_at(start_offset, 4 + LOCHEAD)) {
            Some(Ok(h)) => h,
            Some(Err(e)) => {
                self.last_errno = Some(e);
                self.zipwarn("reading archive fseek: ", e.message());
                return Ok(ZE_READ);
            }
            None => return Ok(ZE_READ),
        };
        if head.len() < 4 || &head[..4] != b"PK\x03\x04" {
            if let Some(f) = self.in_file.take() {
                f.close();
            }
            self.zipwarn("Did not find entry for ", &z.iname);
            return Ok(ZE_FORM);
        }
        if head.len() < 4 + LOCHEAD {
            self.zipwarn("reading local entry: ", self.strerror());
            return Ok(ZE_EOF);
        }
        let buf = &head[4..];
        let mut localz = Zlist {
            ver: sh(buf, 0),
            lflg: sh(buf, 2),
            how: sh(buf, 4),
            tim: lg(buf, 6) as u64,
            crc: lg(buf, 10) as u64,
            ..Zlist::default()
        };
        let nam = sh(buf, 22) as usize;
        let ext = sh(buf, 24) as usize;
        let var = match self.in_file.as_ref().map(|f| f.read_at(start_offset + 4 + LOCHEAD as u64, nam + ext)) {
            Some(Ok(v)) => v,
            _ => return Ok(ZE_READ),
        };
        if var.len() < nam + ext {
            return Ok(ZE_EOF);
        }
        localz.iname = var[..nam].to_vec();
        localz.extra = var[nam..nam + ext].to_vec();
        localz.name = localz.iname.clone();
        self.zip64_entry = Zip::adjust_zip_local_entry(&mut localz);
        localz.vem = z.vem;
        if self.unicode_mismatch != 3 {
            if z.flg & UTF8_BIT != 0 {
                localz.uname = Some(localz.iname.clone());
            } else {
                self.read_unicode_path_local_entry(&mut localz)?;
            }
        }
        if localz.ver != z.ver {
            self.zipwarn("Local Version Needed To Extract does not match CD: ", &z.iname);
        }
        if localz.lflg != z.flg {
            self.zipwarn("Local Entry Flag does not match CD: ", &z.iname);
        }
        if z.flg & 8 == 0 && localz.crc != z.crc {
            self.zipwarn("Local Entry CRC does not match CD: ", &z.iname);
        }
        if localz.iname != z.iname {
            self.zipwarn("Local Entry name does not match CD: ", &z.iname);
        }
        localz.len = z.len;
        localz.siz = z.siz;
        z.dsk = self.current_disk;
        z.off = self.bytes_this_split;
        localz.flg = z.flg;
        if z.flg & 1 == 0 {
            z.flg &= !8;
            localz.flg = z.flg;
            localz.lflg &= !8;
            z.lflg = localz.lflg;
        } else {
            z.lflg = localz.lflg;
        }
        let e: u64 = if z.lflg & 8 != 0 {
            if self.zip64_entry { 24 } else { 16 }
        } else {
            0
        };
        let n = 4 + LOCHEAD as u64 + localz.iname.len() as u64 + localz.extra.len() as u64 + e + z.siz;
        self.tempzn += n;
        localz.crc = z.crc;
        let data_pos = start_offset + 4 + LOCHEAD as u64 + (nam + ext) as u64;
        self.putlocal(&mut localz, PUTLOCAL_WRITE)?;
        let r = self.bfcopy(data_pos, localz.siz)?;
        if z.flg & 8 != 0 {
            self.putextended(&localz)?;
        }
        Ok(r)
    }
}

/// O campo extra "Unicode Path" (0x7075): versão 1, crc-32 do nome local e o nome em UTF-8.
/// Substitui um bloco existente.
pub fn add_unicode_path_extra(extra: &mut Vec<u8>, iname: &[u8], uname: &[u8]) {
    if let Some(p) = get_extra_field(UTF8_PATH_EF_TAG, extra) {
        let old = sh(extra, p + 2) as usize + ZIP_EF_HEADER_SIZE;
        let end = (p + old).min(extra.len());
        extra.drain(p..end);
    }
    let len = ZIP_EF_HEADER_SIZE + 1 + 4 + uname.len();
    put_sh(extra, UTF8_PATH_EF_TAG);
    put_sh(extra, (len - ZIP_EF_HEADER_SIZE) as u16);
    extra.push(1);
    put_lg(extra, crc32_update(0, iname));
    extra.extend_from_slice(uname);
}
