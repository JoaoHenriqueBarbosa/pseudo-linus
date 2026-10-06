//! Leitura do fluxo JPEG: marcadores (`jdmarker.c`), tabelas de Huffman (`jdhuff.c`), decodificação
//! sequencial (`decode_mcu`) e progressiva (`jdphuff.c`), guardando os coeficientes de todos os
//! blocos de cada componente, como o `whole_image` do `jdcoefct.c` no modo de várias varreduras.
//!
//! Dados entrópicos corrompidos seguem o libjpeg: um marcador no meio dos bits faz o resto da
//! varredura ler zeros (`JWRN_HIT_MARKER`), e os MCUs seguintes ficam sem decodificar até o próximo
//! marcador de reinício.

use crate::{ColorSpace, Error, Warning};

pub const NATURAL_ORDER: [usize; 64 + 16] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21,
    28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61,
    54, 47, 55, 62, 63, // extra entries for safety in decoder
    63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63,
];

#[derive(Clone)]
pub struct Component {
    pub id: u8,
    pub h_samp: usize,
    pub v_samp: usize,
    pub quant_tbl: usize,
    pub dc_tbl: usize,
    pub ac_tbl: usize,
    pub width_in_blocks: usize,
    pub height_in_blocks: usize,
    pub downsampled_width: usize,
    pub downsampled_height: usize,
    /// Blocos por linha e linhas de blocos no armazenamento, arredondados ao múltiplo do fator.
    pub blocks_w: usize,
    pub blocks_h: usize,
    pub coefs: Vec<[i16; 64]>,
    /// A tabela de quantização travada no início da primeira varredura da componente.
    pub quant: Option<[u16; 64]>,
}

/// `d_derived_tbl` reduzido ao necessário para decodificar bit a bit (`jpeg_huff_decode`).
#[derive(Clone)]
struct Huff {
    maxcode: [i64; 18],
    valoffset: [i64; 18],
    huffval: [u8; 256],
}

fn make_derived(bits: &[u8; 17], huffval: &[u8; 256], is_dc: bool) -> Result<Huff, Error> {
    let mut huffsize = [0u8; 257];
    let mut p = 0usize;
    for l in 1..=16 {
        let n = usize::from(bits[l]);
        if p + n > 256 {
            return Err(Error::BadHuffTable);
        }
        for _ in 0..n {
            huffsize[p] = l as u8;
            p += 1;
        }
    }
    huffsize[p] = 0;
    let numsymbols = p;
    let mut huffcode = [0u32; 257];
    let mut code = 0u32;
    let mut si = u32::from(huffsize[0]);
    let mut p = 0usize;
    while huffsize[p] != 0 {
        while u32::from(huffsize[p]) == si {
            huffcode[p] = code;
            code += 1;
            p += 1;
        }
        if i64::from(code) >= (1i64 << si) {
            return Err(Error::BadHuffTable);
        }
        code <<= 1;
        si += 1;
    }
    let mut h = Huff { maxcode: [0; 18], valoffset: [0; 18], huffval: *huffval };
    let mut p = 0usize;
    for l in 1..=16 {
        if bits[l] != 0 {
            h.valoffset[l] = p as i64 - i64::from(huffcode[p]);
            p += usize::from(bits[l]);
            h.maxcode[l] = i64::from(huffcode[p - 1]);
        } else {
            h.maxcode[l] = -1;
        }
    }
    h.valoffset[17] = 0;
    h.maxcode[17] = 0xFFFFF;
    if is_dc {
        for &sym in &huffval[..numsymbols] {
            if sym > 15 {
                return Err(Error::BadHuffTable);
            }
        }
    }
    Ok(h)
}

/// O leitor de bits do `jdhuff.c` (`bitread_working_state`).
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    get_buffer: u64,
    bits_left: i32,
    /// `unread_marker`: um marcador achado no meio dos dados entrópicos.
    marker: Option<u8>,
    insufficient: bool,
}

impl<'a> BitReader<'a> {
    fn fill(&mut self, nbits: i32, warnings: &mut Vec<Warning>) {
        while self.bits_left < 57 {
            let c = if self.marker.is_some() || self.pos >= self.data.len() {
                None
            } else {
                let c = self.data[self.pos];
                if c == 0xFF {
                    // Pula enchimento de 0xFF; 0xFF 0x00 é um 0xFF dos dados.
                    let mut q = self.pos + 1;
                    while q < self.data.len() && self.data[q] == 0xFF {
                        q += 1;
                    }
                    if q >= self.data.len() {
                        self.pos = q;
                        None
                    } else if self.data[q] == 0 {
                        self.pos = q + 1;
                        Some(0xFF)
                    } else {
                        self.marker = Some(self.data[q]);
                        self.pos = q + 1;
                        None
                    }
                } else {
                    self.pos += 1;
                    Some(c)
                }
            };
            match c {
                Some(c) => {
                    self.get_buffer = (self.get_buffer << 8) | u64::from(c);
                    self.bits_left += 8;
                }
                None => {
                    if nbits > self.bits_left {
                        if !self.insufficient {
                            warnings.push(Warning::HitMarker);
                            self.insufficient = true;
                        }
                        // Enche de zeros, como o original faz depois do marcador.
                        let add = 57 - self.bits_left;
                        self.get_buffer <<= add;
                        self.bits_left = 57;
                    }
                    return;
                }
            }
        }
    }

    fn get_bits(&mut self, n: i32, w: &mut Vec<Warning>) -> i64 {
        if n == 0 {
            return 0;
        }
        if self.bits_left < n {
            self.fill(n, w);
        }
        self.bits_left -= n;
        ((self.get_buffer >> self.bits_left) & ((1u64 << n) - 1)) as i64
    }

    fn get_bit(&mut self, w: &mut Vec<Warning>) -> i64 {
        self.get_bits(1, w)
    }

    fn decode(&mut self, h: &Huff, w: &mut Vec<Warning>) -> u8 {
        let mut l = 1usize;
        let mut code = self.get_bit(w);
        while l <= 16 && code > h.maxcode[l] {
            code = (code << 1) | self.get_bit(w);
            l += 1;
        }
        if l > 16 {
            w.push(Warning::HuffBadCode);
            return 0;
        }
        h.huffval[(code + h.valoffset[l]) as usize & 0xFF]
    }
}

fn extend(x: i64, s: i32) -> i64 {
    if s == 0 {
        return 0;
    }
    if x < (1 << (s - 1)) { x + ((-1i64) << s) + 1 } else { x }
}

pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub progressive: bool,
    pub comps: Vec<Component>,
    pub max_h: usize,
    pub max_v: usize,
    pub jfif: bool,
    pub adobe: Option<u8>,
    /// `coef_bits` do progressivo: o `Al` da última varredura de cada coeficiente (-1 = nenhuma).
    pub coef_bits: Vec<[i32; 64]>,
    pub prev_coef_bits: Vec<[i32; 64]>,
    pub scans: usize,
    pub warnings: Vec<Warning>,
}

impl Frame {
    pub fn default_color_space(&self) -> ColorSpace {
        match self.comps.len() {
            1 => ColorSpace::Grayscale,
            3 => {
                if self.jfif {
                    ColorSpace::YCbCr
                } else if let Some(t) = self.adobe {
                    if t == 0 { ColorSpace::Rgb } else { ColorSpace::YCbCr }
                } else {
                    let ids = (self.comps[0].id, self.comps[1].id, self.comps[2].id);
                    if ids == (82, 71, 66) { ColorSpace::Rgb } else { ColorSpace::YCbCr }
                }
            }
            4 => match self.adobe {
                Some(0) => ColorSpace::Cmyk,
                Some(_) => ColorSpace::Ycck,
                None => ColorSpace::Cmyk,
            },
            _ => ColorSpace::Unknown,
        }
    }
}

struct Parser<'a> {
    data: &'a [u8],
    pos: usize,
    qt: [Option<[u16; 64]>; 4],
    dc: [Option<Huff>; 4],
    ac: [Option<Huff>; 4],
    restart_interval: usize,
}

impl<'a> Parser<'a> {
    fn byte(&mut self) -> Result<u8, Error> {
        let b = *self.data.get(self.pos).ok_or(Error::Truncated)?;
        self.pos += 1;
        Ok(b)
    }

    fn u16(&mut self) -> Result<usize, Error> {
        Ok((usize::from(self.byte()?) << 8) | usize::from(self.byte()?))
    }

    /// `next_marker`: pula lixo até um 0xFF seguido de algo que não seja 0xFF.
    fn next_marker(&mut self, warnings: &mut Vec<Warning>) -> Result<u8, Error> {
        let mut discarded = 0usize;
        loop {
            let mut c = self.byte()?;
            while c != 0xFF {
                discarded += 1;
                c = self.byte()?;
            }
            loop {
                c = self.byte()?;
                if c != 0xFF {
                    break;
                }
            }
            if c != 0 {
                if discarded != 0 {
                    warnings.push(Warning::ExtraneousData(discarded, c));
                }
                return Ok(c);
            }
            discarded += 2;
        }
    }
}

/// Lê o arquivo inteiro e devolve o quadro com os coeficientes de todas as componentes.
pub fn read(data: &[u8]) -> Result<Frame, Error> {
    let mut p = Parser { data, pos: 0, qt: [None; 4], dc: Default::default(), ac: Default::default(), restart_interval: 0 };
    let c1 = p.byte()?;
    let c2 = p.byte()?;
    if c1 != 0xFF || c2 != 0xD8 {
        return Err(Error::NotJpeg(c1, c2));
    }
    let mut frame: Option<Frame> = None;
    let mut warnings = Vec::new();
    let mut jfif = false;
    let mut adobe = None;
    loop {
        let m = p.next_marker(&mut warnings)?;
        match m {
            0xC0 | 0xC1 | 0xC2 => {
                if frame.is_some() {
                    return Err(Error::SofDuplicate);
                }
                frame = Some(read_sof(&mut p, m == 0xC2, jfif, adobe)?);
            }
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return Err(Error::SofUnsupported(m)),
            0xC4 => read_dht(&mut p)?,
            0xDB => read_dqt(&mut p)?,
            0xDD => {
                let len = p.u16()?;
                if len != 4 {
                    return Err(Error::BadLength);
                }
                p.restart_interval = p.u16()?;
            }
            0xDA => {
                let f = frame.as_mut().ok_or(Error::SosNoSof)?;
                read_scan(&mut p, f, &mut warnings)?;
            }
            0xD9 => {
                let mut f = frame.ok_or(Error::NoImage)?;
                if f.scans == 0 {
                    return Err(Error::NoImage);
                }
                f.warnings = warnings;
                return Ok(f);
            }
            0xE0 | 0xEE => {
                let len = p.u16()?;
                if len < 2 {
                    return Err(Error::BadLength);
                }
                let body = data.get(p.pos..p.pos + len - 2).ok_or(Error::Truncated)?;
                if m == 0xE0 && body.len() >= 5 && &body[..5] == b"JFIF\0" {
                    jfif = true;
                }
                if m == 0xEE && body.len() >= 12 && &body[..5] == b"Adobe" {
                    adobe = Some(body[11]);
                }
                if let Some(f) = frame.as_mut() {
                    f.jfif |= jfif;
                    f.adobe = adobe.or(f.adobe);
                }
                p.pos += len - 2;
            }
            0xD0..=0xD7 | 0x01 => {}
            _ => {
                let len = p.u16()?;
                if len < 2 {
                    return Err(Error::BadLength);
                }
                p.pos += len - 2;
                if p.pos > data.len() {
                    return Err(Error::Truncated);
                }
            }
        }
    }
}

fn read_sof(p: &mut Parser, progressive: bool, jfif: bool, adobe: Option<u8>) -> Result<Frame, Error> {
    let len = p.u16()?;
    let precision = p.byte()?;
    let height = p.u16()?;
    let width = p.u16()?;
    let n = usize::from(p.byte()?);
    if precision != 8 {
        return Err(Error::BadPrecision(precision));
    }
    if height == 0 || width == 0 || n == 0 {
        return Err(Error::EmptyImage);
    }
    if len != 8 + n * 3 {
        return Err(Error::BadLength);
    }
    let mut comps = Vec::with_capacity(n);
    for _ in 0..n {
        let id = p.byte()?;
        let hv = p.byte()?;
        let q = p.byte()?;
        let (h, v) = (usize::from(hv >> 4), usize::from(hv & 15));
        if !(1..=4).contains(&h) || !(1..=4).contains(&v) || q > 3 {
            return Err(Error::BadSampling);
        }
        comps.push(Component {
            id,
            h_samp: h,
            v_samp: v,
            quant_tbl: usize::from(q),
            dc_tbl: 0,
            ac_tbl: 0,
            width_in_blocks: 0,
            height_in_blocks: 0,
            downsampled_width: 0,
            downsampled_height: 0,
            blocks_w: 0,
            blocks_h: 0,
            coefs: Vec::new(),
            quant: None,
        });
    }
    let max_h = comps.iter().map(|c| c.h_samp).max().unwrap_or(1);
    let max_v = comps.iter().map(|c| c.v_samp).max().unwrap_or(1);
    let div_up = |a: usize, b: usize| a.div_ceil(b);
    for c in &mut comps {
        c.width_in_blocks = div_up(width * c.h_samp, max_h * 8);
        c.height_in_blocks = div_up(height * c.v_samp, max_v * 8);
        c.downsampled_width = div_up(width * c.h_samp, max_h);
        c.downsampled_height = div_up(height * c.v_samp, max_v);
        c.blocks_w = c.width_in_blocks.div_ceil(c.h_samp) * c.h_samp;
        c.blocks_h = c.height_in_blocks.div_ceil(c.v_samp) * c.v_samp;
        c.coefs = vec![[0i16; 64]; c.blocks_w * c.blocks_h];
    }
    Ok(Frame {
        width,
        height,
        progressive,
        coef_bits: vec![[-1; 64]; n],
        prev_coef_bits: vec![[0; 64]; n],
        comps,
        max_h,
        max_v,
        jfif,
        adobe,
        scans: 0,
        warnings: Vec::new(),
    })
}

fn read_dht(p: &mut Parser) -> Result<(), Error> {
    let len = p.u16()?;
    let mut left = len.checked_sub(2).ok_or(Error::BadLength)?;
    while left > 16 {
        let index = p.byte()?;
        let mut bits = [0u8; 17];
        let mut count = 0usize;
        for b in bits.iter_mut().skip(1) {
            *b = p.byte()?;
            count += usize::from(*b);
        }
        left -= 17;
        if count > 256 || count > left {
            return Err(Error::BadHuffTable);
        }
        let mut vals = [0u8; 256];
        for v in vals.iter_mut().take(count) {
            *v = p.byte()?;
        }
        left -= count;
        let (is_ac, id) = (index & 0x10 != 0, usize::from(index & 0x0F));
        if id > 3 {
            return Err(Error::BadHuffTable);
        }
        let h = make_derived(&bits, &vals, !is_ac)?;
        if is_ac {
            p.ac[id] = Some(h);
        } else {
            p.dc[id] = Some(h);
        }
    }
    if left != 0 {
        return Err(Error::BadLength);
    }
    Ok(())
}

fn read_dqt(p: &mut Parser) -> Result<(), Error> {
    let len = p.u16()?;
    let mut left = len.checked_sub(2).ok_or(Error::BadLength)? as i64;
    while left > 0 {
        let n = p.byte()?;
        left -= 1;
        let (prec, id) = (n >> 4, usize::from(n & 15));
        if id > 3 {
            return Err(Error::BadQuantTable);
        }
        let mut q = [1u16; 64];
        let count = if prec != 0 { 128 } else { 64 };
        let entries = if left < count { (left / if prec != 0 { 2 } else { 1 }) as usize } else { 64 };
        for &k in NATURAL_ORDER.iter().take(entries) {
            q[k] = if prec != 0 { p.u16()? as u16 } else { u16::from(p.byte()?) };
        }
        left -= (entries as i64) * if prec != 0 { 2 } else { 1 };
        p.qt[id] = Some(q);
    }
    if left != 0 {
        return Err(Error::BadLength);
    }
    Ok(())
}

struct Scan {
    comps: Vec<usize>,
    ss: usize,
    se: usize,
    ah: i32,
    al: i32,
}

fn read_scan(p: &mut Parser, f: &mut Frame, warnings: &mut Vec<Warning>) -> Result<(), Error> {
    let len = p.u16()?;
    let n = usize::from(p.byte()?);
    if len != n * 2 + 6 || !(1..=4).contains(&n) {
        return Err(Error::BadLength);
    }
    let mut comps = Vec::with_capacity(n);
    for _ in 0..n {
        let id = p.byte()?;
        let t = p.byte()?;
        let ci = f.comps.iter().position(|c| c.id == id).ok_or(Error::BadComponentId(id))?;
        if comps.contains(&ci) {
            return Err(Error::BadComponentId(id));
        }
        f.comps[ci].dc_tbl = usize::from(t >> 4) & 3;
        f.comps[ci].ac_tbl = usize::from(t & 15) & 3;
        comps.push(ci);
    }
    let ss = usize::from(p.byte()?);
    let se = usize::from(p.byte()?);
    let a = p.byte()?;
    let scan = Scan { comps, ss, se, ah: i32::from(a >> 4), al: i32::from(a & 15) };
    // `latch_quant_tables`: a tabela de quantização vale a do início da primeira varredura.
    for &ci in &scan.comps {
        if f.comps[ci].quant.is_none() {
            let q = p.qt[f.comps[ci].quant_tbl].ok_or(Error::NoQuantTable(f.comps[ci].quant_tbl))?;
            f.comps[ci].quant = Some(q);
        }
    }
    f.scans += 1;
    if f.progressive {
        start_progressive(f, &scan, warnings)?;
    } else if ss != 0 || se != 63 || scan.ah != 0 || scan.al != 0 {
        warnings.push(Warning::NotSequential);
    }
    decode_scan(p, f, &scan, warnings)
}

fn start_progressive(f: &mut Frame, s: &Scan, warnings: &mut Vec<Warning>) -> Result<(), Error> {
    let is_dc = s.ss == 0;
    let mut bad = if is_dc { s.se != 0 } else { s.ss > s.se || s.se >= 64 || s.comps.len() != 1 };
    if s.ah != 0 && s.al != s.ah - 1 {
        bad = true;
    }
    if s.al > 13 {
        bad = true;
    }
    if bad {
        return Err(Error::BadProgression(s.ss, s.se, s.ah, s.al));
    }
    for &ci in &s.comps {
        if !is_dc && f.coef_bits[ci][0] < 0 {
            warnings.push(Warning::BogusProgression(ci, 0));
        }
        for k in s.ss.min(1)..=s.se.max(9) {
            f.prev_coef_bits[ci][k] = if f.scans > 1 { f.coef_bits[ci][k] } else { 0 };
        }
        for k in s.ss..=s.se {
            let expected = f.coef_bits[ci][k].max(0);
            if s.ah != expected {
                warnings.push(Warning::BogusProgression(ci, k));
            }
            f.coef_bits[ci][k] = s.al;
        }
    }
    Ok(())
}

fn decode_scan(p: &mut Parser, f: &mut Frame, s: &Scan, warnings: &mut Vec<Warning>) -> Result<(), Error> {
    let single = s.comps.len() == 1;
    let (mcus_x, mcus_y) = if single {
        let c = &f.comps[s.comps[0]];
        (c.width_in_blocks, c.height_in_blocks)
    } else {
        (f.width.div_ceil(f.max_h * 8), f.height.div_ceil(f.max_v * 8))
    };
    // Tabelas pedidas pela varredura (as que o `start_pass` validaria).
    let need_dc = !f.progressive || (s.ss == 0 && s.ah == 0);
    let need_ac = !f.progressive || s.ss != 0;
    for &ci in &s.comps {
        let c = &f.comps[ci];
        if need_dc && p.dc[c.dc_tbl].is_none() {
            return Err(Error::NoHuffTable(c.dc_tbl));
        }
        if need_ac && (!f.progressive || s.ss != 0) && p.ac[c.ac_tbl].is_none() && (!f.progressive || s.se != 0) {
            return Err(Error::NoHuffTable(c.ac_tbl));
        }
    }
    let dc_tbls: Vec<Option<Huff>> = s.comps.iter().map(|&ci| p.dc[f.comps[ci].dc_tbl].clone()).collect();
    let ac_tbls: Vec<Option<Huff>> = s.comps.iter().map(|&ci| p.ac[f.comps[ci].ac_tbl].clone()).collect();
    let mut br = BitReader { data: p.data, pos: p.pos, get_buffer: 0, bits_left: 0, marker: None, insufficient: false };
    let mut last_dc = vec![0i64; s.comps.len()];
    let mut eobrun = 0usize;
    let restart = p.restart_interval;
    let mut restarts_to_go = restart;
    let mut next_rst = 0u8;
    for my in 0..mcus_y {
        for mx in 0..mcus_x {
            if restart != 0 {
                if restarts_to_go == 0 {
                    // `process_restart`: descarta os bits que sobraram e espera o RSTn.
                    br.bits_left = 0;
                    if br.marker.is_none() {
                        // Procura o marcador como o `read_restart_marker`/`next_marker`.
                        let mut q = br.pos;
                        while q < br.data.len() && br.data[q] != 0xFF {
                            q += 1;
                        }
                        while q < br.data.len() && br.data[q] == 0xFF {
                            q += 1;
                        }
                        if q < br.data.len() {
                            br.marker = Some(br.data[q]);
                            br.pos = q + 1;
                        } else {
                            br.pos = q;
                        }
                    }
                    if br.marker == Some(0xD0 + next_rst) {
                        br.marker = None;
                    } else {
                        warnings.push(Warning::MustResync);
                        // Marcador errado ou ausente: segue como o `jpeg_resync_to_restart` faz
                        // no caso comum (marcador futuro: deixa para depois e segue com zeros).
                        if matches!(br.marker, Some(0xD0..=0xD7)) {
                            br.marker = None;
                        }
                    }
                    next_rst = (next_rst + 1) & 7;
                    last_dc.iter_mut().for_each(|v| *v = 0);
                    eobrun = 0;
                    br.insufficient = false;
                    restarts_to_go = restart;
                }
                restarts_to_go -= 1;
            }
            // `if (!entropy->pub.insufficient_data)`: o MCU inteiro fica sem decodificar.
            if br.insufficient {
                continue;
            }
            for (k, &ci) in s.comps.iter().enumerate() {
                let (h, v) = if single { (1, 1) } else { (f.comps[ci].h_samp, f.comps[ci].v_samp) };
                for by in 0..v {
                    for bx in 0..h {
                        let (bxx, byy) = if single { (mx, my) } else { (mx * h + bx, my * v + by) };
                        let c = &mut f.comps[ci];
                        let idx = byy * c.blocks_w + bxx;
                        let block = &mut c.coefs[idx];
                        if !f.progressive {
                            decode_block_seq(&mut br, block, dc_tbls[k].as_ref().unwrap(), ac_tbls[k].as_ref().unwrap(), &mut last_dc[k], warnings);
                        } else if s.ss == 0 {
                            if s.ah == 0 {
                                let t = dc_tbls[k].as_ref().unwrap();
                                let sym = i32::from(br.decode(t, warnings));
                                let r = br.get_bits(sym, warnings);
                                last_dc[k] += extend(r, sym);
                                block[0] = (((last_dc[k] as i32) as u32) << s.al) as i32 as i16;
                            } else if br.get_bit(warnings) != 0 {
                                block[0] |= (1i32 << s.al) as i16;
                            }
                        } else if s.ah == 0 {
                            ac_first(&mut br, block, ac_tbls[k].as_ref().unwrap(), s, &mut eobrun, warnings);
                        } else {
                            ac_refine(&mut br, block, ac_tbls[k].as_ref().unwrap(), s, &mut eobrun, warnings);
                        }
                    }
                }
            }
        }
    }
    // Depois da varredura o leitor volta ao fluxo de marcadores (o marcador lido fica pendente).
    p.pos = match br.marker {
        Some(_) => {
            // Volta para o 0xFF do marcador, para o laço principal o ler de novo.
            let mut q = br.pos - 1;
            while q > 0 && br.data[q - 1] == 0xFF {
                q -= 1;
            }
            q
        }
        None => br.pos,
    };
    Ok(())
}

fn decode_block_seq(br: &mut BitReader, block: &mut [i16; 64], dc: &Huff, ac: &Huff, last: &mut i64, w: &mut Vec<Warning>) {
    let s = i32::from(br.decode(dc, w));
    if s != 0 {
        let r = br.get_bits(s, w);
        *last += extend(r, s);
    } else {
        *last += 0;
    }
    block[0] = *last as i16;
    let mut k = 1usize;
    while k < 64 {
        let rs = br.decode(ac, w);
        let r = usize::from(rs >> 4);
        let s = i32::from(rs & 15);
        if s != 0 {
            k += r;
            let v = br.get_bits(s, w);
            block[NATURAL_ORDER[k]] = extend(v, s) as i16;
        } else {
            if r != 15 {
                break;
            }
            k += 15;
        }
        k += 1;
    }
}

fn ac_first(br: &mut BitReader, block: &mut [i16; 64], t: &Huff, s: &Scan, eobrun: &mut usize, w: &mut Vec<Warning>) {
    if *eobrun > 0 {
        *eobrun -= 1;
        return;
    }
    let mut k = s.ss;
    while k <= s.se {
        let rs = br.decode(t, w);
        let r = usize::from(rs >> 4);
        let sz = i32::from(rs & 15);
        if sz != 0 {
            k += r;
            let v = br.get_bits(sz, w);
            let v = extend(v, sz);
            block[NATURAL_ORDER[k]] = (((v as i32) as u32) << s.al) as i32 as i16;
        } else if r == 15 {
            k += 15;
        } else {
            let mut run = 1usize << r;
            if r != 0 {
                run += br.get_bits(r as i32, w) as usize;
            }
            *eobrun = run - 1;
            break;
        }
        k += 1;
    }
}

fn ac_refine(br: &mut BitReader, block: &mut [i16; 64], t: &Huff, s: &Scan, eobrun: &mut usize, w: &mut Vec<Warning>) {
    let p1: i32 = 1 << s.al;
    let m1: i32 = (-1i32) << s.al;
    let mut k = s.ss;
    if *eobrun == 0 {
        while k <= s.se {
            let rs = br.decode(t, w);
            let mut r = i32::from(rs >> 4);
            let sz = rs & 15;
            let mut val = 0i32;
            if sz != 0 {
                if sz != 1 {
                    w.push(Warning::HuffBadCode);
                }
                val = if br.get_bit(w) != 0 { p1 } else { m1 };
            } else if r != 15 {
                *eobrun = 1 << r;
                if r != 0 {
                    *eobrun += br.get_bits(r, w) as usize;
                }
                break;
            }
            loop {
                let pos = NATURAL_ORDER[k];
                let coef = &mut block[pos];
                if *coef != 0 {
                    if br.get_bit(w) != 0 && (i32::from(*coef) & p1) == 0 {
                        *coef = if *coef >= 0 { (i32::from(*coef) + p1) as i16 } else { (i32::from(*coef) + m1) as i16 };
                    }
                } else {
                    r -= 1;
                    if r < 0 {
                        break;
                    }
                }
                k += 1;
                if k > s.se {
                    break;
                }
            }
            if val != 0 {
                // Sem testar `k <= Se`, como o original: as entradas extras da ordem natural cobrem.
                block[NATURAL_ORDER[k]] = val as i16;
            }
            k += 1;
        }
    }
    if *eobrun > 0 {
        while k <= s.se {
            let coef = &mut block[NATURAL_ORDER[k]];
            if *coef != 0 && br.get_bit(w) != 0 && (i32::from(*coef) & p1) == 0 {
                *coef = if *coef >= 0 { (i32::from(*coef) + p1) as i16 } else { (i32::from(*coef) + m1) as i16 };
            }
            k += 1;
        }
        *eobrun -= 1;
    }
}
