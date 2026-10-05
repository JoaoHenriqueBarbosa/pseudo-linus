//! Campos extras (zipfile.c e unix.c): busca de blocos, Zip64, horário UT e UID/GID do Unix.

use super::consts::*;
use super::state::{IzTimes, Zip, Zlist};

pub fn sh(b: &[u8], off: usize) -> u16 {
    (b.get(off).copied().unwrap_or(0) as u16) | ((b.get(off + 1).copied().unwrap_or(0) as u16) << 8)
}

pub fn lg(b: &[u8], off: usize) -> u32 {
    (sh(b, off) as u32) | ((sh(b, off + 2) as u32) << 16)
}

pub fn llg(b: &[u8], off: usize) -> u64 {
    (lg(b, off) as u64) | ((lg(b, off + 4) as u64) << 32)
}

pub fn put_sh(out: &mut Vec<u8>, v: u16) {
    out.push((v & 0xff) as u8);
    out.push((v >> 8) as u8);
}

pub fn put_lg(out: &mut Vec<u8>, v: u32) {
    put_sh(out, (v & 0xffff) as u16);
    put_sh(out, (v >> 16) as u16);
}

pub fn put_llg(out: &mut Vec<u8>, v: u64) {
    put_lg(out, (v & 0xffff_ffff) as u32);
    put_lg(out, (v >> 32) as u32);
}

/// `get_extra_field`: o deslocamento do cabeçalho do bloco com a etiqueta, se existir.
pub fn get_extra_field(tag: u16, extra: &[u8]) -> Option<usize> {
    if extra.len() < ZIP_EF_HEADER_SIZE {
        return None;
    }
    let mut p = 0usize;
    while p < extra.len() - ZIP_EF_HEADER_SIZE {
        let t = sh(extra, p);
        let size = sh(extra, p + 2) as usize;
        if t == tag {
            return Some(p);
        }
        p += size + ZIP_EF_HEADER_SIZE;
    }
    None
}

/// `copy_nondup_extra_fields`: os blocos antigos que não estão nos novos, seguidos de todos os novos.
pub fn copy_nondup_extra_fields(old: &[u8], new: &[u8]) -> Vec<u8> {
    if old.is_empty() {
        return new.to_vec();
    }
    let mut out = Vec::new();
    let mut p = 0usize;
    while p < old.len() {
        let tag = sh(old, p);
        let blocksize = sh(old, p + 2) as usize;
        if get_extra_field(tag, new).is_none() {
            let end = (p + blocksize + 4).min(old.len());
            out.extend_from_slice(&old[p..end]);
        }
        p += blocksize + 4;
    }
    out.extend_from_slice(new);
    out
}

/// `ef_scan_ut_time`: procura horários Unix nos campos extras (UT ou o antigo UX). Devolve as
/// flags do UT (simuladas para o UX), ou 0.
pub fn ef_scan_ut_time(ef: &[u8], is_cent: bool, z_utim: &mut IzTimes) -> i32 {
    let mut flags: i32 = 0;
    let mut have_new_type_eb = false;
    let mut pos = 0usize;
    let mut left = ef.len();
    if left == 0 {
        return 0;
    }
    while left >= EB_HEADSIZE {
        let eb_id = sh(ef, pos);
        let eb_len = sh(ef, pos + 2) as usize;
        if eb_len > left - EB_HEADSIZE {
            break;
        }
        let data = pos + EB_HEADSIZE;
        match eb_id {
            EF_TIME => {
                flags &= !0x00ff;
                have_new_type_eb = true;
                if eb_len >= EB_UT_MINLEN {
                    let mut eb_idx = 1usize;
                    flags |= (ef[data] as i32) & 0x00ff;
                    if flags & EB_UT_FL_MTIME != 0 {
                        if eb_idx + 4 <= eb_len {
                            z_utim.mtime = lg(ef, data + eb_idx) as i64;
                            eb_idx += 4;
                        } else {
                            flags &= !EB_UT_FL_MTIME;
                        }
                    }
                    if !is_cent {
                        if flags & EB_UT_FL_ATIME != 0 {
                            if eb_idx + 4 <= eb_len {
                                z_utim.atime = lg(ef, data + eb_idx) as i64;
                                eb_idx += 4;
                            } else {
                                flags &= !EB_UT_FL_ATIME;
                            }
                        }
                        if flags & EB_UT_FL_CTIME != 0 {
                            if eb_idx + 4 <= eb_len {
                                z_utim.ctime = lg(ef, data + eb_idx) as i64;
                            } else {
                                flags &= !EB_UT_FL_CTIME;
                            }
                        }
                    }
                }
            }
            EF_IZUNIX2 => {
                if !have_new_type_eb {
                    flags &= !0x00ff;
                    have_new_type_eb = true;
                }
            }
            EF_IZUNIX => {
                if eb_len >= EB_UX_MINLEN && !have_new_type_eb {
                    z_utim.atime = lg(ef, data) as i64;
                    z_utim.mtime = lg(ef, data + 4) as i64;
                    flags |= EB_UT_FL_MTIME | EB_UT_FL_ATIME;
                }
            }
            _ => {}
        }
        pos += eb_len + EB_HEADSIZE;
        left -= eb_len + EB_HEADSIZE;
    }
    flags
}

/// `get_ef_ut_ztime`: primeiro o campo extra local, depois o central.
pub fn get_ef_ut_ztime(z: &Zlist, z_utim: &mut IzTimes) -> i32 {
    let mut r = ef_scan_ut_time(&z.extra, false, z_utim);
    if r == 0 && !z.cextra.is_empty() && z.cextra != z.extra {
        r = ef_scan_ut_time(&z.cextra, true, z_utim);
    }
    r
}

impl Zip {
    /// `adjust_zip_central_entry`: aplica o campo Zip64 do cabeçalho central a `len`, `siz`, `off`
    /// e `dsk`. Devolve se há campo Zip64.
    pub fn adjust_zip_central_entry(z: &mut Zlist) -> bool {
        let Some(mut p) = get_extra_field(ZIP64_EF_TAG, &z.cextra) else { return false };
        p += ZIP_EF_HEADER_SIZE;
        if z.len == ZIP_UWORD32_MAX {
            z.len = llg(&z.cextra, p);
            p += 8;
        }
        if z.siz == ZIP_UWORD32_MAX {
            z.siz = llg(&z.cextra, p);
            p += 8;
        }
        if z.off == ZIP_UWORD32_MAX {
            z.off = llg(&z.cextra, p);
            p += 8;
        }
        if z.dsk == ZIP_UWORD16_MAX {
            z.dsk = lg(&z.cextra, p) as u64;
        }
        true
    }

    /// `adjust_zip_local_entry`: o mesmo para o cabeçalho local (só `len` e `siz`).
    pub fn adjust_zip_local_entry(z: &mut Zlist) -> bool {
        let Some(mut p) = get_extra_field(ZIP64_EF_TAG, &z.extra) else { return false };
        p += ZIP_EF_HEADER_SIZE;
        if z.len == ZIP_UWORD32_MAX {
            z.len = llg(&z.extra, p);
            p += 8;
        }
        if z.siz == ZIP_UWORD32_MAX {
            z.siz = llg(&z.extra, p);
        }
        true
    }

    /// `add_central_zip64_extra_field`: o bloco Zip64 do cabeçalho central, com só os campos que
    /// estouram, no fim do campo extra (o antigo, se havia, sai). Devolve `Err(ZE_BIG)` se o `-fz-`
    /// proíbe e o Zip64 é necessário.
    pub fn add_central_zip64_extra_field(&mut self, z: &mut Zlist) -> Result<(), i32> {
        let mut efsize = ZIP_EF_HEADER_SIZE;
        let mut used_zip64 = false;
        if z.len > ZIP_UWORD32_MAX || self.force_zip64 == 1 {
            efsize += 8;
            used_zip64 = true;
        }
        if z.siz > ZIP_UWORD32_MAX {
            efsize += 8;
            used_zip64 = true;
        }
        if z.off > ZIP_UWORD32_MAX {
            efsize += 8;
            used_zip64 = true;
        }
        if z.dsk > ZIP_UWORD16_MAX {
            efsize += 4;
            used_zip64 = true;
        }
        if used_zip64 && self.force_zip64 == 0 {
            self.zipwarn("Large entry support disabled using -fz- but needed", "");
            return Err(ZE_BIG);
        }
        if z.cextra.is_empty() && efsize == ZIP_EF_HEADER_SIZE {
            return Ok(());
        }
        if let Some(p) = get_extra_field(ZIP64_EF_TAG, &z.cextra) {
            let old = sh(&z.cextra, p + 2) as usize + ZIP_EF_HEADER_SIZE;
            let end = (p + old).min(z.cextra.len());
            z.cextra.drain(p..end);
        }
        let mut ef = Vec::with_capacity(efsize);
        put_sh(&mut ef, ZIP64_EF_TAG);
        put_sh(&mut ef, (efsize - ZIP_EF_HEADER_SIZE) as u16);
        if z.len > ZIP_UWORD32_MAX || self.force_zip64 == 1 {
            put_llg(&mut ef, z.len);
        }
        if z.siz > ZIP_UWORD32_MAX {
            put_llg(&mut ef, z.siz);
        }
        if z.off > ZIP_UWORD32_MAX {
            put_llg(&mut ef, z.off);
        }
        if z.dsk > ZIP_UWORD16_MAX {
            put_lg(&mut ef, z.dsk as u32);
        }
        z.cextra.extend_from_slice(&ef);
        Ok(())
    }

    /// `add_local_zip64_extra_field`: o bloco Zip64 do cabeçalho local (tamanho original e
    /// comprimido, 16 bytes), que o zip reescreve quando os tamanhos se firmam.
    pub fn add_local_zip64_extra_field(z: &mut Zlist) {
        if let Some(p) = get_extra_field(ZIP64_EF_TAG, &z.extra) {
            let blocksize = sh(&z.extra, p + 2) as usize;
            if blocksize == 16 {
                let mut vals = Vec::new();
                put_llg(&mut vals, z.len);
                put_llg(&mut vals, z.siz);
                z.extra[p + 4..p + 20].copy_from_slice(&vals);
                return;
            }
            let end = (p + blocksize + ZIP_EF_HEADER_SIZE).min(z.extra.len());
            z.extra.drain(p..end);
        }
        put_sh(&mut z.extra, ZIP64_EF_TAG);
        put_sh(&mut z.extra, 16);
        put_llg(&mut z.extra, z.len);
        put_llg(&mut z.extra, z.siz);
    }
}
