//! Codificador progressivo (`jcphuff.c`) com o roteiro do `jpeg_simple_progression` e a
//! otimização de Huffman que o `jcmaster.c` sempre liga no modo progressivo: cada varredura passa
//! uma vez contando símbolos e outra escrevendo, com as tabelas geradas para ela.

use crate::decode::NATURAL_ORDER;
use crate::encode::{emit_dht, emit_dri, emit_sos, frame_header, gen_optimal, nbits, BitWriter, EncodeError, Prepared};

const MAX_CORR_BITS: usize = 1000;
const MAX_COEF_BITS: u32 = 10;

struct Scan {
    comps: Vec<usize>,
    ss: usize,
    se: usize,
    ah: u32,
    al: u32,
}

/// `jpeg_simple_progression`.
fn script(p: &Prepared, ycc: bool) -> Vec<Scan> {
    let n = p.comps.len();
    let s = |comps: Vec<usize>, ss, se, ah, al| Scan { comps, ss, se, ah, al };
    let all: Vec<usize> = (0..n).collect();
    let dc = |ah, al| -> Vec<Scan> {
        if n <= 4 { vec![s(all.clone(), 0, 0, ah, al)] } else { (0..n).map(|c| s(vec![c], 0, 0, ah, al)).collect() }
    };
    let each = |ss, se, ah, al| -> Vec<Scan> { (0..n).map(|c| s(vec![c], ss, se, ah, al)).collect() };
    let mut v = Vec::new();
    if n == 3 && ycc {
        v.extend(dc(0, 1));
        v.push(s(vec![0], 1, 5, 0, 2));
        v.push(s(vec![2], 1, 63, 0, 1));
        v.push(s(vec![1], 1, 63, 0, 1));
        v.push(s(vec![0], 6, 63, 0, 2));
        v.push(s(vec![0], 1, 63, 2, 1));
        v.extend(dc(1, 0));
        v.push(s(vec![2], 1, 63, 1, 0));
        v.push(s(vec![1], 1, 63, 1, 0));
        v.push(s(vec![0], 1, 63, 1, 0));
    } else {
        v.extend(dc(0, 1));
        v.extend(each(1, 5, 0, 2));
        v.extend(each(6, 63, 0, 2));
        v.extend(each(1, 63, 2, 1));
        v.extend(dc(1, 0));
        v.extend(each(1, 63, 1, 0));
    }
    v
}

/// O estado do `phuff_entropy_encoder`, em modo de contagem ou de escrita.
struct Phuff<'a> {
    gather: bool,
    counts: Vec<[i64; 257]>,
    codes: Vec<Option<([u32; 256], [u8; 256])>>,
    w: &'a mut BitWriter,
    eobrun: usize,
    /// Bits de correção pendentes (o `bit_buffer` até `BE`).
    pending: Vec<u8>,
    ac_tbl: usize,
    last_dc: Vec<i32>,
}

impl Phuff<'_> {
    fn symbol(&mut self, tbl: usize, sym: usize) {
        if self.gather {
            self.counts[tbl][sym] += 1;
        } else if let Some((code, size)) = &self.codes[tbl] {
            self.w.put(code[sym], u32::from(size[sym]));
        }
    }

    fn bits(&mut self, v: u32, n: u32) {
        if !self.gather {
            self.w.put(v, n);
        }
    }

    fn buffered(&mut self, bits: &[u8]) {
        if self.gather {
            return;
        }
        for &b in bits {
            self.w.put(u32::from(b), 1);
        }
    }

    fn emit_eobrun(&mut self) -> Result<(), EncodeError> {
        if self.eobrun > 0 {
            let n = (usize::BITS - self.eobrun.leading_zeros()) - 1;
            if n > 14 {
                return Err(EncodeError::BadCoefficient);
            }
            self.symbol(self.ac_tbl, (n as usize) << 4);
            if n != 0 {
                self.bits(self.eobrun as u32, n);
            }
            self.eobrun = 0;
            let pending = std::mem::take(&mut self.pending);
            self.buffered(&pending);
        }
        Ok(())
    }
}

fn run_scan(p: &Prepared, sc: &Scan, ph: &mut Phuff, mcus: &[Vec<(usize, usize)>], restart: usize) -> Result<(), EncodeError> {
    let mut togo = restart;
    let mut next_rst = 0u8;
    for m in mcus {
        if restart != 0 {
            if togo == 0 {
                // `emit_restart`.
                ph.emit_eobrun()?;
                if !ph.gather {
                    ph.w.flush();
                    ph.w.marker(0xD0 + next_rst);
                }
                if sc.ss == 0 {
                    ph.last_dc.iter_mut().for_each(|v| *v = 0);
                } else {
                    ph.eobrun = 0;
                    ph.pending.clear();
                }
                next_rst = (next_rst + 1) & 7;
                togo = restart;
            }
            togo -= 1;
        }
        for &(k, bi) in m {
            let c = &p.comps[sc.comps[k]];
            let block = &c.coefs[bi];
            if sc.ss == 0 {
                if sc.ah == 0 {
                    let t2 = i32::from(block[0]) >> sc.al;
                    let diff = t2 - ph.last_dc[k];
                    ph.last_dc[k] = t2;
                    let n = nbits(diff);
                    if n > MAX_COEF_BITS + 1 {
                        return Err(EncodeError::BadCoefficient);
                    }
                    ph.symbol(c.td, n as usize);
                    if n != 0 {
                        let v = if diff < 0 { (diff - 1) as u32 } else { diff as u32 };
                        ph.bits(v & ((1u32 << n) - 1), n);
                    }
                } else {
                    ph.bits(((i32::from(block[0]) >> sc.al) & 1) as u32, 1);
                }
            } else if sc.ah == 0 {
                ac_first(ph, block, sc)?;
            } else {
                ac_refine(ph, block, sc)?;
            }
        }
    }
    ph.emit_eobrun()?;
    if !ph.gather {
        ph.w.flush();
    }
    Ok(())
}

fn ac_first(ph: &mut Phuff, block: &[i16; 64], sc: &Scan) -> Result<(), EncodeError> {
    let mut r = 0usize;
    for k in sc.ss..=sc.se {
        let t = i32::from(block[NATURAL_ORDER[k]]);
        if t == 0 {
            r += 1;
            continue;
        }
        let (a, t2) = if t < 0 {
            let a = (-t) >> sc.al;
            (a, !a)
        } else {
            let a = t >> sc.al;
            (a, a)
        };
        if a == 0 {
            r += 1;
            continue;
        }
        ph.emit_eobrun()?;
        while r > 15 {
            ph.symbol(ph.ac_tbl, 0xF0);
            r -= 16;
        }
        let n = nbits(a);
        if n > MAX_COEF_BITS {
            return Err(EncodeError::BadCoefficient);
        }
        ph.symbol(ph.ac_tbl, (r << 4) + n as usize);
        ph.bits(t2 as u32 & ((1u32 << n) - 1), n);
        r = 0;
    }
    if r > 0 {
        ph.eobrun += 1;
        if ph.eobrun == 0x7FFF {
            ph.emit_eobrun()?;
        }
    }
    Ok(())
}

fn ac_refine(ph: &mut Phuff, block: &[i16; 64], sc: &Scan) -> Result<(), EncodeError> {
    let mut abs = [0i32; 64];
    let mut eob = 0usize;
    for k in sc.ss..=sc.se {
        let a = i32::from(block[NATURAL_ORDER[k]]).abs() >> sc.al;
        abs[k] = a;
        if a == 1 {
            eob = k;
        }
    }
    let mut r = 0usize;
    let mut cur: Vec<u8> = Vec::new();
    for k in sc.ss..=sc.se {
        let a = abs[k];
        if a == 0 {
            r += 1;
            continue;
        }
        while r > 15 && k <= eob {
            ph.emit_eobrun()?;
            ph.symbol(ph.ac_tbl, 0xF0);
            r -= 16;
            ph.buffered(&cur);
            cur.clear();
        }
        if a > 1 {
            cur.push((a & 1) as u8);
            continue;
        }
        ph.emit_eobrun()?;
        ph.symbol(ph.ac_tbl, (r << 4) + 1);
        ph.bits(u32::from(block[NATURAL_ORDER[k]] >= 0), 1);
        ph.buffered(&cur);
        cur.clear();
        r = 0;
    }
    if r > 0 || !cur.is_empty() {
        ph.eobrun += 1;
        ph.pending.extend_from_slice(&cur);
        if ph.eobrun == 0x7FFF || ph.pending.len() > MAX_CORR_BITS - 64 + 1 {
            ph.emit_eobrun()?;
        }
    }
    Ok(())
}

pub(crate) fn encode_progressive(p: &Prepared, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    frame_header(p, true, out);
    let ycc = p.jfif && p.comps.len() == 3;
    let mut last_dri = 0usize;
    for sc in script(p, ycc) {
        let (mcus, per_row) = crate::encode::scan_blocks(p, &sc.comps);
        let restart = p.restart_for(per_row);
        let dc_band = sc.ss == 0;
        let mut codes: Vec<Option<([u32; 256], [u8; 256])>> = vec![None; 4];
        let mut tables = Vec::new();
        // Passada de contagem, exceto no refinamento de DC, que não usa tabela.
        if !(dc_band && sc.ah != 0) {
            let mut sink = BitWriter::new();
            let mut ph = Phuff {
                gather: true,
                counts: vec![[0i64; 257]; 4],
                codes: vec![None; 4],
                w: &mut sink,
                eobrun: 0,
                pending: Vec::new(),
                ac_tbl: p.comps[sc.comps[0]].ta,
                last_dc: vec![0; sc.comps.len()],
            };
            run_scan(p, &sc, &mut ph, &mcus, restart)?;
            let mut did = [false; 4];
            for &ci in &sc.comps {
                let tbl = if dc_band { p.comps[ci].td } else { p.comps[ci].ta };
                if !did[tbl] {
                    let t = gen_optimal(&ph.counts[tbl]);
                    codes[tbl] = Some(t.derive());
                    tables.push((tbl, t));
                    did[tbl] = true;
                }
            }
        }
        for (tbl, t) in &tables {
            emit_dht(out, if dc_band { *tbl as u8 } else { 0x10 + *tbl as u8 }, t);
        }
        if restart != last_dri {
            emit_dri(restart, out);
            last_dri = restart;
        }
        emit_sos(p, &sc.comps, sc.ss as u8, sc.se as u8, sc.ah as u8, sc.al as u8, out);
        let mut w = BitWriter::new();
        let mut ph = Phuff {
            gather: false,
            counts: vec![[0i64; 257]; 4],
            codes,
            w: &mut w,
            eobrun: 0,
            pending: Vec::new(),
            ac_tbl: p.comps[sc.comps[0]].ta,
            last_dc: vec![0; sc.comps.len()],
        };
        run_scan(p, &sc, &mut ph, &mcus, restart)?;
        out.extend_from_slice(&w.out);
    }
    Ok(())
}
