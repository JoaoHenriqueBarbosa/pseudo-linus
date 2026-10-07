//! Codecs de imagem da libImaging do Pillow 11.1.0: `RawDecode.c`/`RawEncode.c` (bytes crus por
//! linha, com stride e de baixo para cima) e `ZipDecode.c`/`ZipEncode.c` (o deflate do PNG, com os
//! filtros de linha e o entrelaçamento Adam7). A máquina de estados segue o `ImagingCodecState`:
//! `setimage` define a região, `decode` consome bytes e `encode` enche um buffer de saída.
//!
//! O deflate é o do zlib 1.3.1 traduzido (`zdeflate`), então o PNG sai idêntico byte a byte ao do
//! Pillow; o inflate é o do `flate2`.
//!
//! Portado da libImaging (MIT-CMU: Copyright © 1996-1997 Fredrik Lundh, © 1997 Secret Labs AB).

use flate2::{Decompress, FlushDecompress, Status};
use zdeflate::{Deflate, Flush, Status as ZStatus, Strategy};

use super::image::Image;
use super::pack::Shuffle;

pub const CODEC_END: i32 = 1;
pub const CODEC_BROKEN: i32 = -2;
pub const CODEC_UNKNOWN: i32 = -3;
pub const CODEC_CONFIG: i32 = -8;

/// `ImagingCodecStateInstance`.
pub struct CodecState {
    pub count: i32,
    pub state: i32,
    pub errcode: i32,
    pub x: i32,
    pub y: i32,
    pub ystep: i32,
    pub xsize: i32,
    pub ysize: i32,
    pub xoff: i32,
    pub yoff: i32,
    pub shuffle: Shuffle,
    pub bits: i32,
    pub bytes: i32,
}

impl CodecState {
    /// Erro do codec: guarda o código e devolve o -1 que o chamador retorna.
    fn fail(&mut self, code: i32) -> i32 {
        self.errcode = code;
        -1
    }

    pub fn new(shuffle: Shuffle, bits: i32) -> CodecState {
        CodecState {
            count: 0,
            state: 0,
            errcode: 0,
            x: 0,
            y: 0,
            ystep: 0,
            xsize: 0,
            ysize: 0,
            xoff: 0,
            yoff: 0,
            shuffle,
            bits,
            bytes: 0,
        }
    }

    /// O `_setimage` comum: região do tile e tamanho de uma linha empacotada.
    pub fn setimage(&mut self, im: &Image, ext: (i32, i32, i32, i32), recompute_bytes: bool) -> Result<(), String> {
        let (x0, y0, x1, y1) = ext;
        if x0 == 0 && x1 == 0 {
            self.xsize = im.xsize;
            self.ysize = im.ysize;
        } else {
            self.xoff = x0;
            self.yoff = y0;
            self.xsize = x1 - x0;
            self.ysize = y1 - y0;
        }
        if self.xsize <= 0 || self.xsize + self.xoff > im.xsize || self.ysize <= 0 || self.ysize + self.yoff > im.ysize {
            return Err("tile cannot extend outside image".into());
        }
        if self.bits > 0 && (recompute_bytes || self.bytes == 0) {
            self.bytes = (self.bits * self.xsize + 7) / 8;
        }
        Ok(())
    }

    /// Onde começa, no armazenamento, a linha `y` do tile.
    pub fn row(&self, im: &Image, y: i32) -> usize {
        im.offset(self.xoff, y + self.yoff)
    }
}

// ---- raw ----

pub struct RawDecoder {
    pub stride: i32,
    skip: i32,
}

impl RawDecoder {
    pub fn new(stride: i32) -> RawDecoder {
        RawDecoder { stride, skip: 0 }
    }

    /// `ImagingRawDecode`.
    pub fn decode(&mut self, im: &mut Image, st: &mut CodecState, buf: &[u8]) -> i32 {
        const LINE: i32 = 1;
        const SKIP: i32 = 2;
        if st.state == 0 {
            st.bytes = (st.xsize * st.bits + 7) / 8;
            if self.stride != 0 {
                self.skip = self.stride - st.bytes;
                if self.skip < 0 {
                    return st.fail(CODEC_CONFIG);
                }
            } else {
                self.skip = 0;
            }
            if st.ystep < 0 {
                st.y = st.ysize - 1;
                st.ystep = -1;
            } else {
                st.ystep = 1;
            }
            st.state = LINE;
        }
        let mut p = 0usize;
        let mut left = buf.len() as i64;
        loop {
            if st.state == SKIP {
                if left < i64::from(self.skip) {
                    return p as i32;
                }
                p += self.skip as usize;
                left -= i64::from(self.skip);
                st.state = LINE;
            }
            if left < i64::from(st.bytes) {
                return p as i32;
            }
            let o = st.row(im, st.y);
            (st.shuffle)(&mut im.data[o..], &buf[p..], st.xsize as usize);
            p += st.bytes as usize;
            left -= i64::from(st.bytes);
            st.y += st.ystep;
            if st.y < 0 || st.y >= st.ysize {
                return -1;
            }
            st.state = SKIP;
        }
    }
}

/// `ImagingRawEncode`.
pub fn raw_encode(im: &Image, st: &mut CodecState, buf: &mut [u8]) -> i32 {
    if st.state == 0 {
        if st.count > 0 {
            let bytes = st.count;
            if st.count < st.bytes {
                return st.fail(CODEC_CONFIG);
            }
            st.count = st.bytes;
            st.bytes = bytes;
        } else {
            st.count = st.bytes;
        }
        if st.ystep < 0 {
            st.y = st.ysize - 1;
            st.ystep = -1;
        } else {
            st.ystep = 1;
        }
        st.state = 1;
    }
    let mut left = buf.len() as i32;
    if left < st.bytes {
        st.errcode = CODEC_CONFIG;
        return 0;
    }
    let mut p = 0usize;
    while left >= st.bytes {
        let o = st.row(im, st.y);
        (st.shuffle)(&mut buf[p..], &im.data[o..], st.xsize as usize);
        if st.bytes > st.count {
            buf[p + st.count as usize..p + st.bytes as usize].fill(0);
        }
        p += st.bytes as usize;
        left -= st.bytes;
        st.y += st.ystep;
        if st.y < 0 || st.y >= st.ysize {
            st.errcode = CODEC_END;
            break;
        }
    }
    p as i32
}

// ---- zip ----

const OFFSET: [i32; 7] = [7, 3, 3, 1, 1, 0, 0];
const STARTING_COL: [i32; 7] = [0, 4, 0, 2, 0, 1, 0];
const STARTING_ROW: [i32; 7] = [0, 0, 4, 0, 2, 0, 1];
const COL_INCREMENT: [i32; 7] = [8, 8, 4, 4, 2, 2, 1];
const ROW_INCREMENT: [i32; 7] = [8, 8, 8, 4, 4, 2, 2];

pub struct ZipDecoder {
    interlaced: bool,
    pass: usize,
    last_output: usize,
    buffer: Vec<u8>,
    previous: Vec<u8>,
    z: Option<Decompress>,
}

impl ZipDecoder {
    pub fn new(interlaced: bool) -> ZipDecoder {
        ZipDecoder { interlaced, pass: 0, last_output: 0, buffer: Vec::new(), previous: Vec::new(), z: None }
    }

    fn row_len(st: &CodecState, pass: usize) -> i32 {
        let n = (st.xsize + OFFSET[pass]) / COL_INCREMENT[pass];
        (n * st.bits + 7) / 8
    }

    fn unfilter(&mut self, st: &CodecState, row_len: usize) -> bool {
        let bpp = ((st.bits + 7) / 8) as usize;
        let (b, prev) = (&mut self.buffer, &self.previous);
        match b[0] {
            0 => {}
            1 => {
                for i in bpp + 1..=row_len {
                    b[i] = b[i].wrapping_add(b[i - bpp]);
                }
            }
            2 => {
                for i in 1..=row_len {
                    b[i] = b[i].wrapping_add(prev[i]);
                }
            }
            3 => {
                for i in 1..=bpp.min(row_len) {
                    b[i] = b[i].wrapping_add(prev[i] / 2);
                }
                for i in bpp + 1..=row_len {
                    b[i] = b[i].wrapping_add(((u32::from(b[i - bpp]) + u32::from(prev[i])) / 2) as u8);
                }
            }
            4 => {
                for i in 1..=bpp.min(row_len) {
                    b[i] = b[i].wrapping_add(prev[i]);
                }
                for i in bpp + 1..=row_len {
                    let (a, bb, c) = (i32::from(b[i - bpp]), i32::from(prev[i]), i32::from(prev[i - bpp]));
                    let (pa, pb, pc) = ((bb - c).abs(), (a - c).abs(), (a + bb - 2 * c).abs());
                    let pred = if pa <= pb && pa <= pc { a } else if pb <= pc { bb } else { c };
                    b[i] = b[i].wrapping_add(pred as u8);
                }
            }
            _ => return false,
        }
        true
    }

    /// `ImagingZipDecode`.
    pub fn decode(&mut self, im: &mut Image, st: &mut CodecState, buf: &[u8]) -> i32 {
        const PREFIX: usize = 1;
        if st.state == 0 {
            let n = st.bytes as usize + 1;
            self.buffer = vec![0; n];
            self.previous = vec![0; n];
            self.last_output = 0;
            self.z = Some(Decompress::new(true));
            if self.interlaced {
                self.pass = 0;
                st.y = STARTING_ROW[0];
            }
            st.state = 1;
        }
        let mut row_len = if self.interlaced { Self::row_len(st, self.pass) } else { st.bytes } as usize;
        let mut pos = 0usize;
        while pos < buf.len() {
            let Some(z) = self.z.as_mut() else {
                return -1;
            };
            let want = row_len + PREFIX;
            let (in0, out0) = (z.total_in(), z.total_out());
            let res = z.decompress(&buf[pos..], &mut self.buffer[self.last_output..want], FlushDecompress::None);
            pos += (z.total_in() - in0) as usize;
            let produced = (z.total_out() - out0) as usize;
            let ended = match res {
                Ok(Status::StreamEnd) => true,
                Ok(Status::Ok) => false,
                Ok(Status::BufError) => {
                    self.z = None;
                    return st.fail(CODEC_CONFIG);
                }
                Err(_) => {
                    self.z = None;
                    return st.fail(CODEC_BROKEN);
                }
            };
            let n = self.last_output + produced;
            if n < want {
                self.last_output = n;
                if ended {
                    // O fluxo acabou antes da linha: o C sairia no próximo `inflate` sem progresso.
                    self.z = None;
                    return -1;
                }
                break;
            }
            if !self.unfilter(st, row_len) {
                self.z = None;
                return st.fail(CODEC_UNKNOWN);
            }
            if self.interlaced {
                let pass = self.pass;
                let mut col = STARTING_COL[pass];
                let step = ((st.bits + 7) / 8) as usize;
                if st.bits >= 8 {
                    let mut i = 0;
                    while i < row_len {
                        let o = im.offset(col, st.y);
                        (st.shuffle)(&mut im.data[o..], &self.buffer[PREFIX + i..], 1);
                        col += COL_INCREMENT[pass];
                        i += step;
                    }
                } else {
                    let row_bits = ((st.xsize + OFFSET[pass]) / COL_INCREMENT[pass]) * st.bits;
                    let mut i = 0;
                    while i < row_bits {
                        let byte = [self.buffer[PREFIX + (i / 8) as usize] << (i % 8)];
                        let o = im.offset(col, st.y);
                        (st.shuffle)(&mut im.data[o..], &byte, 1);
                        col += COL_INCREMENT[pass];
                        i += st.bits;
                    }
                }
                st.y += ROW_INCREMENT[self.pass];
                while st.y >= st.ysize || row_len == 0 {
                    self.pass += 1;
                    if self.pass == 7 {
                        st.y = st.ysize;
                        break;
                    }
                    st.y = STARTING_ROW[self.pass];
                    row_len = Self::row_len(st, self.pass) as usize;
                    self.buffer.fill(0);
                }
            } else {
                let o = st.row(im, st.y);
                (st.shuffle)(&mut im.data[o..], &self.buffer[PREFIX..], st.xsize as usize);
                st.y += 1;
            }
            self.last_output = 0;
            if st.y >= st.ysize || ended {
                self.z = None;
                return -1;
            }
            std::mem::swap(&mut self.buffer, &mut self.previous);
        }
        buf.len() as i32
    }
}

pub struct ZipEncoder {
    pub palette: bool,
    pub optimize: bool,
    pub compress_level: i32,
    pub compress_type: i32,
    pub dictionary: Option<Vec<u8>>,
    z: Option<Deflate>,
    buffer: Vec<u8>,
    previous: Vec<u8>,
    /// Entrada ainda não consumida pelo deflate (o `next_in`/`avail_in` do original).
    pending: Vec<u8>,
    pending_pos: usize,
}

/// `(v < 128) ? v : 256 - v`.
fn dist(v: u8) -> i32 {
    if v < 128 { i32::from(v) } else { 256 - i32::from(v) }
}

impl ZipEncoder {
    pub fn new(palette: bool, optimize: bool, compress_level: i32, compress_type: i32, dictionary: Option<Vec<u8>>) -> ZipEncoder {
        ZipEncoder {
            palette,
            optimize,
            compress_level,
            compress_type,
            dictionary,
            z: None,
            buffer: Vec::new(),
            previous: Vec::new(),
            pending: Vec::new(),
            pending_pos: 0,
        }
    }

    /// Escolhe o filtro da linha em `self.buffer` (heurística da libpng) e devolve a linha filtrada.
    fn filter(&self, st: &CodecState) -> Vec<u8> {
        let n = st.bytes as usize;
        let bpp = ((st.bits + 7) / 8) as usize;
        let (b, prev) = (&self.buffer, &self.previous);
        let mut output = b[..=n].to_vec();
        let mut sum: i32 = (1..=n).map(|i| dist(b[i])).sum();
        if sum > 0 {
            let mut up = vec![2u8; n + 1];
            let mut s = 0;
            for i in 1..=n {
                up[i] = b[i].wrapping_sub(prev[i]);
                s += dist(up[i]);
            }
            if s < sum {
                output = up;
                sum = s;
            }
        }
        if sum > 0 {
            let mut prior = vec![1u8; n + 1];
            let mut s = 0;
            for i in 1..=n {
                prior[i] = if i <= bpp { b[i] } else { b[i].wrapping_sub(b[i - bpp]) };
                s += dist(prior[i]);
            }
            if s < sum {
                output = prior;
                sum = s;
            }
        }
        if self.optimize && sum > 0 {
            let mut avg = vec![3u8; n + 1];
            let mut s = 0;
            for i in 1..=n {
                avg[i] = if i <= bpp {
                    b[i].wrapping_sub(prev[i] / 2)
                } else {
                    b[i].wrapping_sub(((u32::from(b[i - bpp]) + u32::from(prev[i])) / 2) as u8)
                };
                s += dist(avg[i]);
            }
            if s < sum {
                output = avg;
                sum = s;
            }
        }
        if sum > 0 {
            let mut paeth = vec![4u8; n + 1];
            let mut s = 0;
            for i in 1..=n {
                paeth[i] = if i <= bpp {
                    b[i].wrapping_sub(prev[i])
                } else {
                    let (a, bb, c) = (i32::from(b[i - bpp]), i32::from(prev[i]), i32::from(prev[i - bpp]));
                    let (pa, pb, pc) = ((bb - c).abs(), (a - c).abs(), (a + bb - 2 * c).abs());
                    let pred = if pa <= pb && pa <= pc { a } else if pb <= pc { bb } else { c };
                    b[i].wrapping_sub(pred as u8)
                };
                s += dist(paeth[i]);
            }
            if s < sum {
                output = paeth;
            }
        }
        output
    }

    /// `deflate(Z_NO_FLUSH)` sobre a entrada pendente, escrevendo em `out[*w..]`.
    fn feed(&mut self, out: &mut [u8], w: &mut usize) -> Result<(), i32> {
        let z = self.z.as_mut().ok_or(CODEC_CONFIG)?;
        let p = z.deflate(&self.pending[self.pending_pos..], &mut out[*w..], Flush::None);
        self.pending_pos += p.consumed;
        *w += p.produced;
        match p.status {
            Ok(_) | Err(zdeflate::Error::Buf) => Ok(()),
            Err(zdeflate::Error::Stream) => Err(CODEC_CONFIG),
        }
    }

    /// `ImagingZipEncode`.
    pub fn encode(&mut self, im: &Image, st: &mut CodecState, out: &mut [u8]) -> i32 {
        let bytes = out.len();
        if st.state == 0 {
            let n = st.bytes as usize + 1;
            self.buffer = vec![0; n];
            self.previous = vec![0; n];
            let level = if self.optimize { 9 } else { self.compress_level };
            let strategy = match self.compress_type {
                -1 if !self.palette => Strategy::Filtered,
                -1 | 0 => Strategy::Default,
                1 => Strategy::Filtered,
                2 => Strategy::HuffmanOnly,
                3 => Strategy::Rle,
                4 => Strategy::Fixed,
                _ => {
                    return st.fail(CODEC_CONFIG);
                }
            };
            match Deflate::new(level, 15, 9, strategy) {
                Ok(z) => self.z = Some(z),
                Err(_) => {
                    return st.fail(CODEC_CONFIG);
                }
            }
            if self.dictionary.as_ref().is_some_and(|d| !d.is_empty()) {
                // O zdeflate não implementa `deflateSetDictionary`; nenhum plugin embutido passa dicionário.
                return st.fail(CODEC_CONFIG);
            }
            st.state = 1;
        }
        let mut w = 0usize;
        if self.pending_pos < self.pending.len() {
            if let Err(e) = self.feed(out, &mut w) {
                self.z = None;
                return st.fail(e);
            }
        }
        if st.state == 1 {
            while w < bytes {
                if st.y >= st.ysize {
                    st.state = 2;
                    break;
                }
                let o = st.row(im, st.y);
                (st.shuffle)(&mut self.buffer[1..], &im.data[o..], st.xsize as usize);
                self.buffer[0] = 0;
                st.y += 1;
                let line = if self.palette { self.buffer.clone() } else { self.filter(st) };
                self.pending = line;
                self.pending_pos = 0;
                if let Err(e) = self.feed(out, &mut w) {
                    self.z = None;
                    return st.fail(e);
                }
                std::mem::swap(&mut self.buffer, &mut self.previous);
            }
            if w == bytes {
                return w as i32;
            }
        }
        if st.state == 2 {
            while w < bytes {
                let Some(z) = self.z.as_mut() else {
                    break;
                };
                let p = z.deflate(&self.pending[self.pending_pos..], &mut out[w..], Flush::Finish);
                self.pending_pos += p.consumed;
                w += p.produced;
                if matches!(p.status, Ok(ZStatus::StreamEnd)) {
                    self.z = None;
                    st.errcode = CODEC_END;
                    break;
                }
            }
        }
        w as i32
    }
}

// ---- jpeg ----

/// `ImagingJpegEncode` sobre o `zjpeg`: na primeira chamada empacota a imagem inteira e codifica;
/// as chamadas seguintes entregam o fluxo em blocos do tamanho do buffer.
pub struct JpegEncoder {
    pub opts: Option<zjpeg::EncodeOptions>,
    pub rawmode: String,
    out: Vec<u8>,
    sent: usize,
}

impl JpegEncoder {
    pub fn new(opts: zjpeg::EncodeOptions, rawmode: &str) -> JpegEncoder {
        JpegEncoder { opts: Some(opts), rawmode: rawmode.into(), out: Vec::new(), sent: 0 }
    }

    pub fn encode(&mut self, im: &Image, st: &mut CodecState, buf: &mut [u8]) -> i32 {
        if let Some(mut o) = self.opts.take() {
            o.width = st.xsize.max(0) as usize;
            o.height = st.ysize.max(0) as usize;
            o.input = match st.bits {
                8 => zjpeg::InputSpace::Grayscale,
                24 if im.mode == "YCbCr" => zjpeg::InputSpace::YCbCr,
                24 => zjpeg::InputSpace::Rgb,
                // `JCS_EXT_RGBX` com o rawmode `RGBX`, CMYK nos demais casos de 32 bits.
                32 if self.rawmode == "RGBX" => zjpeg::InputSpace::Rgb,
                32 => zjpeg::InputSpace::Cmyk,
                _ => {
                    return st.fail(CODEC_CONFIG);
                }
            };
            let rgbx = self.rawmode == "RGBX";
            let line_bytes = (st.xsize as usize) * (st.bits as usize / 8);
            let comps = if rgbx { 3 } else { st.bits as usize / 8 };
            let mut line = vec![0u8; line_bytes];
            let mut pixels = Vec::with_capacity(o.width * o.height * comps);
            for y in 0..st.ysize {
                let off = st.row(im, y);
                (st.shuffle)(&mut line, &im.data[off..], st.xsize as usize);
                if rgbx {
                    for px in line.chunks(4) {
                        pixels.extend_from_slice(&px[..3]);
                    }
                } else {
                    pixels.extend_from_slice(&line);
                }
            }
            match zjpeg::encode(&o, &pixels) {
                Ok(b) => self.out = b,
                Err(zjpeg::EncodeError::Config) => {
                    return st.fail(CODEC_CONFIG);
                }
                Err(_) => {
                    return st.fail(CODEC_BROKEN);
                }
            }
        }
        let n = (self.out.len() - self.sent).min(buf.len());
        buf[..n].copy_from_slice(&self.out[self.sent..self.sent + n]);
        self.sent += n;
        if self.sent >= self.out.len() {
            st.errcode = CODEC_END;
        }
        n as i32
    }
}

/// `ImagingJpegDecode` sobre o `zjpeg`. O libjpeg do original suspende quando faltam bytes e
/// retoma na chamada seguinte; aqui os blocos se acumulam e a decodificação roda quando chega um
/// `FF D9` (fim de imagem), repetindo se o arquivo ainda estiver incompleto naquele ponto.
pub struct JpegDecoder {
    rawmode: String,
    jpegmode: String,
    scale: i32,
    draft: bool,
    data: Vec<u8>,
}

impl JpegDecoder {
    pub fn new(rawmode: &str, jpegmode: &str, scale: i32, draft: bool) -> JpegDecoder {
        JpegDecoder { rawmode: rawmode.into(), jpegmode: jpegmode.into(), scale, draft, data: Vec::new() }
    }

    fn options(&self) -> zjpeg::Options {
        use zjpeg::ColorSpace as C;
        let mut o = zjpeg::Options::default();
        o.jpeg_color_space = match self.jpegmode.as_str() {
            "L" => Some(C::Grayscale),
            "RGB" => Some(C::Rgb),
            "CMYK" => Some(C::Cmyk),
            "YCbCr" => Some(C::YCbCr),
            "YCbCrK" => Some(C::Ycck),
            _ => None,
        };
        o.out_color_space = Some(match self.rawmode.as_str() {
            "L" => C::Grayscale,
            "RGB" | "RGBX" => C::Rgb,
            "CMYK" | "CMYK;I" => C::Cmyk,
            "YCbCr" => C::YCbCr,
            "YCbCrK" => C::Ycck,
            _ => {
                // Conversões desligadas: o que estiver no arquivo sai como está.
                o.jpeg_color_space = Some(C::Unknown);
                C::Unknown
            }
        });
        if self.scale > 1 {
            o.scale_denom = self.scale as usize;
        }
        if self.draft {
            o.fancy_upsampling = false;
        }
        o
    }

    pub fn decode(&mut self, im: &mut Image, st: &mut CodecState, buf: &[u8]) -> i32 {
        // O fim de imagem pode ter chegado partido entre o bloco anterior e este.
        let tail = self.data.last() == Some(&0xFF) && buf.first() == Some(&0xD9);
        self.data.extend_from_slice(buf);
        if !tail && !buf.windows(2).any(|w| w == [0xFF, 0xD9]) {
            return buf.len() as i32;
        }
        let d = match zjpeg::decode(&self.data, &self.options()) {
            Ok(d) => d,
            Err(zjpeg::Error::Truncated) => return buf.len() as i32,
            Err(_) => {
                return st.fail(CODEC_BROKEN);
            }
        };
        // O `RGBX` das extensões do libjpeg-turbo: quatro bytes por pixel com o quarto em 255.
        let rgbx = self.rawmode == "RGBX";
        let comps = d.components;
        let mut line = vec![0u8; d.width * if rgbx { 4 } else { comps }];
        let rows = (st.ysize.max(0) as usize).min(d.height);
        for y in 0..rows {
            let src = &d.data[y * d.width * comps..(y + 1) * d.width * comps];
            if rgbx {
                for x in 0..d.width {
                    line[x * 4..x * 4 + 3].copy_from_slice(&src[x * 3..x * 3 + 3]);
                    line[x * 4 + 3] = 255;
                }
            } else {
                line.copy_from_slice(src);
            }
            let o = st.row(im, y as i32);
            (st.shuffle)(&mut im.data[o..], &line, st.xsize as usize);
        }
        st.y = rows as i32;
        -1
    }
}
