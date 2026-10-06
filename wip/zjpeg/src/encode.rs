//! Codificador JPEG sequencial do libjpeg-turbo 2.1.5 (o que o `JpegEncode.c` do Pillow pede):
//! conversão RGB → YCbCr (`jccolor.c`), subamostragem (`jcsample.c`) com o preenchimento das
//! bordas do `jcprep.c`, DCT direta inteira (`jfdctint.c`) com a quantização por recíproco do
//! `jcdctmgr.c` (na variante de 16 bits que o Debian compila com SIMD), blocos fictícios do
//! `jccoefct.c`, Huffman padrão ou otimizado (`jchuff.c`) e os marcadores do `jcmarker.c`.

use crate::decode::NATURAL_ORDER;

/// Espaço de cor da entrada (`in_color_space`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputSpace {
    Grayscale,
    Rgb,
    YCbCr,
    Cmyk,
}

/// Parâmetros do `jpeg_set_defaults` mais o que o Pillow muda depois.
#[derive(Clone, Debug)]
pub struct EncodeOptions {
    pub width: usize,
    pub height: usize,
    pub input: InputSpace,
    /// `quality` do Pillow; `None` mantém a qualidade padrão (75).
    pub quality: Option<i32>,
    /// Tabelas de quantização próprias (`qtables`), em ordem natural.
    pub qtables: Option<Vec<[u32; 64]>>,
    /// -1 = padrão do libjpeg (4:2:0), 0 = 4:4:4, 1 = 4:2:2, 2 = 4:2:0.
    pub subsampling: i32,
    pub progressive: bool,
    pub optimize: bool,
    pub keep_rgb: bool,
    pub restart_interval: usize,
    pub restart_in_rows: usize,
    pub dpi: Option<(u16, u16)>,
    /// Bytes já formatados que o Pillow põe depois do cabeçalho: APP1 de EXIF (só o corpo),
    /// os marcadores `extra` (ICC e afins, com marcador e tamanho) e o comentário (só o corpo).
    pub exif: Vec<u8>,
    pub extra: Vec<u8>,
    pub comment: Option<Vec<u8>>,
}

impl EncodeOptions {
    pub fn new(width: usize, height: usize, input: InputSpace) -> EncodeOptions {
        EncodeOptions {
            width,
            height,
            input,
            quality: None,
            qtables: None,
            subsampling: -1,
            progressive: false,
            optimize: false,
            keep_rgb: false,
            restart_interval: 0,
            restart_in_rows: 0,
            dpi: None,
            exif: Vec::new(),
            extra: Vec::new(),
            comment: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// `IMAGING_CODEC_CONFIG`: combinação de parâmetros que o Pillow recusa.
    Config,
    ImageTooBig,
    BadCoefficient,
}

const STD_LUMINANCE: [u32; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56, 14, 17, 22, 29,
    51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113, 92, 49, 64, 78, 87, 103, 121, 120,
    101, 72, 92, 95, 98, 112, 100, 103, 99,
];
const STD_CHROMINANCE: [u32; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99, 47, 66, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99,
];

const BITS_DC_LUM: [u8; 17] = [0, 0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const VAL_DC: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const BITS_DC_CHR: [u8; 17] = [0, 0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const BITS_AC_LUM: [u8; 17] = [0, 0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
const VAL_AC_LUM: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14,
    0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09,
    0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a,
    0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65,
    0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88,
    0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9,
    0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca,
    0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea,
    0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
];
const BITS_AC_CHR: [u8; 17] = [0, 0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
const VAL_AC_CHR: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13, 0x22, 0x32,
    0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0, 0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16,
    0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39,
    0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64,
    0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86,
    0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8,
    0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9,
    0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
];

/// `JHUFF_TBL`: contagem por comprimento e símbolos.
#[derive(Clone)]
pub(crate) struct HuffTable {
    pub bits: [u8; 17],
    pub vals: Vec<u8>,
}

impl HuffTable {
    fn std(bits: &[u8; 17], vals: &[u8]) -> HuffTable {
        HuffTable { bits: *bits, vals: vals.to_vec() }
    }

    /// `jpeg_make_c_derived_tbl`: código e tamanho por símbolo.
    pub(crate) fn derive(&self) -> ([u32; 256], [u8; 256]) {
        let mut code = [0u32; 256];
        let mut size = [0u8; 256];
        let mut c = 0u32;
        let mut p = 0usize;
        for l in 1..=16usize {
            for _ in 0..self.bits[l] {
                let s = usize::from(self.vals[p]);
                code[s] = c;
                size[s] = l as u8;
                c += 1;
                p += 1;
            }
            c <<= 1;
        }
        (code, size)
    }
}

/// `jpeg_gen_optimal_table` (seção K.2 do padrão), com os desempates do libjpeg.
pub(crate) fn gen_optimal(freq_in: &[i64; 257]) -> HuffTable {
    const MAX_CLEN: usize = 32;
    let mut freq = *freq_in;
    let mut bits = [0u8; MAX_CLEN + 1];
    let mut codesize = [0usize; 257];
    let mut others = [-1i32; 257];
    freq[256] = 1;
    loop {
        let mut c1: i32 = -1;
        let mut v = 1_000_000_000i64;
        for i in 0..=256 {
            if freq[i] != 0 && freq[i] <= v {
                v = freq[i];
                c1 = i as i32;
            }
        }
        let mut c2: i32 = -1;
        v = 1_000_000_000;
        for i in 0..=256 {
            if freq[i] != 0 && freq[i] <= v && i as i32 != c1 {
                v = freq[i];
                c2 = i as i32;
            }
        }
        if c2 < 0 {
            break;
        }
        let (mut a, mut b) = (c1 as usize, c2 as usize);
        freq[a] += freq[b];
        freq[b] = 0;
        codesize[a] += 1;
        while others[a] >= 0 {
            a = others[a] as usize;
            codesize[a] += 1;
        }
        others[a] = c2;
        codesize[b] += 1;
        while others[b] >= 0 {
            b = others[b] as usize;
            codesize[b] += 1;
        }
    }
    for &cs in &codesize {
        if cs != 0 {
            bits[cs] += 1;
        }
    }
    let mut i = MAX_CLEN;
    while i > 16 {
        while bits[i] > 0 {
            let mut j = i - 2;
            while bits[j] == 0 {
                j -= 1;
            }
            bits[i] -= 2;
            bits[i - 1] += 1;
            bits[j + 1] += 2;
            bits[j] -= 1;
        }
        i -= 1;
    }
    while bits[i] == 0 {
        i -= 1;
    }
    bits[i] -= 1;
    let mut vals = Vec::new();
    for l in 1..=MAX_CLEN {
        for (s, &cs) in codesize.iter().enumerate().take(256) {
            if cs == l {
                vals.push(s as u8);
            }
        }
    }
    let mut out = [0u8; 17];
    out.copy_from_slice(&bits[..17]);
    HuffTable { bits: out, vals }
}

/// Escritor de bits do `jchuff.c`: MSB primeiro, 0xFF seguido de 0x00.
pub(crate) struct BitWriter {
    pub out: Vec<u8>,
    acc: u64,
    nbits: u32,
}

impl BitWriter {
    pub(crate) fn new() -> BitWriter {
        BitWriter { out: Vec::new(), acc: 0, nbits: 0 }
    }

    pub(crate) fn put(&mut self, code: u32, size: u32) {
        if size == 0 {
            return;
        }
        self.acc = (self.acc << size) | u64::from(code & ((1u32 << size) - 1));
        self.nbits += size;
        while self.nbits >= 8 {
            let b = (self.acc >> (self.nbits - 8)) as u8;
            self.out.push(b);
            if b == 0xFF {
                self.out.push(0);
            }
            self.nbits -= 8;
        }
        self.acc &= (1u64 << self.nbits) - 1;
    }

    /// `flush_bits`: completa o último byte com uns.
    pub(crate) fn flush(&mut self) {
        if self.nbits > 0 {
            self.put(0x7F, 7);
        }
        self.acc = 0;
        self.nbits = 0;
    }

    pub(crate) fn marker(&mut self, m: u8) {
        self.out.push(0xFF);
        self.out.push(m);
    }
}

/// Quantas casas binárias o valor absoluto precisa.
pub(crate) fn nbits(v: i32) -> u32 {
    32 - v.unsigned_abs().leading_zeros()
}

const MAX_COEF_BITS: u32 = 10;

// ---- DCT direta e quantização ----

const CONST_BITS: i32 = 13;
const PASS1_BITS: i32 = 2;

fn descale(x: i64, n: i32) -> i64 {
    (x + (1 << (n - 1))) >> n
}

/// `jpeg_fdct_islow` sobre `DCTELEM` de 16 bits (a variante compilada com SIMD no Debian).
fn fdct_islow(data: &mut [i16; 64]) {
    const F0298: i64 = 2446;
    const F0390: i64 = 3196;
    const F0541: i64 = 4433;
    const F0765: i64 = 6270;
    const F0899: i64 = 7373;
    const F1175: i64 = 9633;
    const F1501: i64 = 12299;
    const F1847: i64 = 15137;
    const F1961: i64 = 16069;
    const F2053: i64 = 16819;
    const F2562: i64 = 20995;
    const F3072: i64 = 25172;
    for pass in 0..2 {
        for i in 0..8 {
            let idx = |k: usize| if pass == 0 { i * 8 + k } else { k * 8 + i };
            let d = |k: usize| i64::from(data[idx(k)]);
            let tmp0 = d(0) + d(7);
            let tmp7 = d(0) - d(7);
            let tmp1 = d(1) + d(6);
            let tmp6 = d(1) - d(6);
            let tmp2 = d(2) + d(5);
            let tmp5 = d(2) - d(5);
            let tmp3 = d(3) + d(4);
            let tmp4 = d(3) - d(4);
            let tmp10 = tmp0 + tmp3;
            let tmp13 = tmp0 - tmp3;
            let tmp11 = tmp1 + tmp2;
            let tmp12 = tmp1 - tmp2;
            let (s_even, s_odd) = if pass == 0 { (None, CONST_BITS - PASS1_BITS) } else { (Some(PASS1_BITS), CONST_BITS + PASS1_BITS) };
            let mut o = [0i64; 8];
            match s_even {
                None => {
                    o[0] = (tmp10 + tmp11) << PASS1_BITS;
                    o[4] = (tmp10 - tmp11) << PASS1_BITS;
                }
                Some(s) => {
                    o[0] = descale(tmp10 + tmp11, s);
                    o[4] = descale(tmp10 - tmp11, s);
                }
            }
            let z1 = (tmp12 + tmp13) * F0541;
            o[2] = descale(z1 + tmp13 * F0765, s_odd);
            o[6] = descale(z1 + tmp12 * -F1847, s_odd);
            let z1 = tmp4 + tmp7;
            let z2 = tmp5 + tmp6;
            let z3 = tmp4 + tmp6;
            let z4 = tmp5 + tmp7;
            let z5 = (z3 + z4) * F1175;
            let t4 = tmp4 * F0298;
            let t5 = tmp5 * F2053;
            let t6 = tmp6 * F3072;
            let t7 = tmp7 * F1501;
            let z1 = z1 * -F0899;
            let z2 = z2 * -F2562;
            let z3 = z3 * -F1961 + z5;
            let z4 = z4 * -F0390 + z5;
            o[7] = descale(t4 + z1 + z3, s_odd);
            o[5] = descale(t5 + z2 + z4, s_odd);
            o[3] = descale(t6 + z2 + z3, s_odd);
            o[1] = descale(t7 + z1 + z4, s_odd);
            for k in 0..8 {
                data[idx(k)] = o[k] as i16;
            }
        }
    }
}

/// `compute_reciprocal` com `DCTELEM` de 16 bits: (recíproco, correção, deslocamento).
fn reciprocal(divisor: u32) -> (u32, u32, u32) {
    if divisor == 1 {
        return (1, 0, 0);
    }
    let b = 31 - divisor.leading_zeros();
    let mut r = 16 + b;
    let mut fq = (1u64 << r) / u64::from(divisor);
    let fr = (1u64 << r) % u64::from(divisor);
    let mut c = divisor / 2;
    if fr == 0 {
        fq >>= 1;
        r -= 1;
    } else if fr <= u64::from(divisor / 2) {
        c += 1;
    } else {
        fq += 1;
    }
    (fq as u32 & 0xFFFF, c & 0xFFFF, r - 16)
}

fn quantize(ws: &[i16; 64], div: &[(u32, u32, u32); 64]) -> [i16; 64] {
    let mut out = [0i16; 64];
    for i in 0..64 {
        let (recip, corr, shift) = div[i];
        let t = i32::from(ws[i]);
        let a = t.unsigned_abs() & 0xFFFF;
        // `divisor == 1`: identidade (recíproco 1, deslocamento -16 no original).
        let q = if recip == 1 && corr == 0 && shift == 0 {
            a
        } else {
            let product = (a + corr).wrapping_mul(recip);
            product >> (shift + 16)
        };
        let q = q as i16;
        out[i] = if t < 0 { q.wrapping_neg() } else { q };
    }
    out
}

// ---- cor e amostragem ----

fn fix(x: f64) -> i64 {
    (x * 65536.0 + 0.5) as i64
}

fn rgb_to_ycc(r: u8, g: u8, b: u8) -> [u8; 3] {
    let (r, g, b) = (i64::from(r), i64::from(g), i64::from(b));
    let half = 1i64 << 15;
    let cbcr = 128i64 << 16;
    let y = (fix(0.29900) * r + fix(0.58700) * g + fix(0.11400) * b + half) >> 16;
    let cb = (-fix(0.16874) * r - fix(0.33126) * g + fix(0.50000) * b + cbcr + half - 1) >> 16;
    let cr = (fix(0.50000) * r - fix(0.41869) * g - fix(0.08131) * b + cbcr + half - 1) >> 16;
    [y as u8, cb as u8, cr as u8]
}

pub(crate) struct Comp {
    pub id: u8,
    pub h: usize,
    pub v: usize,
    pub tq: usize,
    pub td: usize,
    pub ta: usize,
    pub wib: usize,
    pub hib: usize,
    /// Blocos no armazenamento: arredondados ao múltiplo do fator, com os fictícios.
    pub bw: usize,
    pub bh: usize,
    pub coefs: Vec<[i16; 64]>,
}

pub(crate) struct Prepared {
    pub comps: Vec<Comp>,
    pub max_h: usize,
    pub max_v: usize,
    pub width: usize,
    pub height: usize,
    pub qtables: Vec<Option<[u16; 64]>>,
    pub jfif: bool,
    pub adobe: Option<u8>,
    pub restart_interval: usize,
    /// `restart_in_rows`: o intervalo em linhas de MCU, recalculado a cada varredura.
    pub restart_in_rows: usize,
}

impl Prepared {
    /// `per_scan_setup`: o intervalo de reinício efetivo da varredura com estes MCUs por linha.
    pub(crate) fn restart_for(&self, mcus_per_row: usize) -> usize {
        if self.restart_in_rows > 0 { (self.restart_in_rows * mcus_per_row).min(65535) } else { self.restart_interval }
    }
}

fn add_quant(basic: &[u32; 64], scale: i64, force_baseline: bool) -> [u16; 64] {
    let mut q = [0u16; 64];
    for i in 0..64 {
        let mut t = (i64::from(basic[i]) * scale + 50) / 100;
        if t <= 0 {
            t = 1;
        }
        if t > 32767 {
            t = 32767;
        }
        if force_baseline && t > 255 {
            t = 255;
        }
        q[i] = t as u16;
    }
    q
}

fn quality_scaling(q: i32) -> i64 {
    let q = q.clamp(1, 100);
    i64::from(if q < 50 { 5000 / q } else { 200 - q * 2 })
}

/// Monta os coeficientes quantizados de todas as componentes a partir das amostras intercaladas.
pub(crate) fn prepare(o: &EncodeOptions, pixels: &[u8]) -> Result<Prepared, EncodeError> {
    if o.width == 0 || o.height == 0 || o.width > 65535 || o.height > 65535 {
        return Err(EncodeError::ImageTooBig);
    }
    // `jpeg_default_colorspace` e o `keep_rgb` do Pillow.
    let (ncomp, in_comps) = match o.input {
        InputSpace::Grayscale => (1, 1),
        InputSpace::Rgb | InputSpace::YCbCr => (3, 3),
        InputSpace::Cmyk => (4, 4),
    };
    let to_ycc = o.input == InputSpace::Rgb && !o.keep_rgb;
    let rgb_out = o.input == InputSpace::Rgb && o.keep_rgb;
    if rgb_out && !matches!(o.subsampling, -1 | 0) {
        return Err(EncodeError::Config);
    }
    let ycc_like = matches!(o.input, InputSpace::YCbCr) || to_ycc;
    let (jfif, adobe) = match o.input {
        InputSpace::Grayscale => (true, None),
        _ if ycc_like => (true, None),
        InputSpace::Rgb => (false, Some(0)),
        _ => (false, Some(0)),
    };
    // `SET_COMP`: (id, h, v, tabela de quantização, DC, AC).
    let mut spec: Vec<(u8, usize, usize, usize, usize, usize)> = match ncomp {
        1 => vec![(1, 1, 1, 0, 0, 0)],
        3 if ycc_like => vec![(1, 2, 2, 0, 0, 0), (2, 1, 1, 1, 1, 1), (3, 1, 1, 1, 1, 1)],
        3 => vec![(0x52, 1, 1, 0, 0, 0), (0x47, 1, 1, 0, 0, 0), (0x42, 1, 1, 0, 0, 0)],
        _ => vec![(0x43, 1, 1, 0, 0, 0), (0x4D, 1, 1, 0, 0, 0), (0x59, 1, 1, 0, 0, 0), (0x4B, 1, 1, 0, 0, 0)],
    };
    // Tabelas de quantização (o `jpeg_set_defaults` usa qualidade 75 com `force_baseline`).
    let mut qtables: Vec<Option<[u16; 64]>> = vec![None; 4];
    qtables[0] = Some(add_quant(&STD_LUMINANCE, quality_scaling(75), true));
    qtables[1] = Some(add_quant(&STD_CHROMINANCE, quality_scaling(75), true));
    if let Some(qt) = &o.qtables {
        let quality = i64::from(o.quality.unwrap_or(100));
        let mut last_q = 0;
        for (i, t) in qt.iter().enumerate().take(4) {
            qtables[i] = Some(add_quant(t, quality, false));
            if i < spec.len() {
                spec[i].3 = i;
            }
            last_q = i;
        }
        if qt.len() == 1 {
            qtables[1] = Some(add_quant(&qt[0], quality, false));
        }
        for s in spec.iter_mut().skip(last_q) {
            s.3 = last_q;
        }
    } else if let Some(q) = o.quality {
        let s = quality_scaling(q);
        qtables[0] = Some(add_quant(&STD_LUMINANCE, s, true));
        qtables[1] = Some(add_quant(&STD_CHROMINANCE, s, true));
    }
    // O Pillow mexe nos fatores das três primeiras componentes, qualquer que seja o espaço.
    let set = |spec: &mut Vec<(u8, usize, usize, usize, usize, usize)>, f: [(usize, usize); 3]| {
        for (i, (h, v)) in f.iter().enumerate() {
            if let Some(s) = spec.get_mut(i) {
                s.1 = *h;
                s.2 = *v;
            }
        }
    };
    match o.subsampling {
        0 => set(&mut spec, [(1, 1), (1, 1), (1, 1)]),
        1 => set(&mut spec, [(2, 1), (1, 1), (1, 1)]),
        2 => set(&mut spec, [(2, 2), (1, 1), (1, 1)]),
        _ => {}
    }
    let (w, h) = (o.width, o.height);
    let max_h = spec.iter().map(|s| s.1).max().unwrap_or(1);
    let max_v = spec.iter().map(|s| s.2).max().unwrap_or(1);
    // Planos de cor W x H.
    let mut planes = vec![vec![0u8; w * h]; ncomp];
    for p in 0..w * h {
        let px = &pixels[p * in_comps..p * in_comps + in_comps];
        if to_ycc {
            let c = rgb_to_ycc(px[0], px[1], px[2]);
            for k in 0..3 {
                planes[k][p] = c[k];
            }
        } else {
            for k in 0..ncomp {
                planes[k][p] = px[k];
            }
        }
    }
    let total_imcu = h.div_ceil(max_v * 8);
    let hp = h.div_ceil(max_v) * max_v;
    let mut comps = Vec::with_capacity(ncomp);
    for (ci, s) in spec.iter().enumerate() {
        let (cid, ch, cv) = (s.0, s.1, s.2);
        let wib = (w * ch).div_ceil(max_h * 8);
        let hib = (h * cv).div_ceil(max_v * 8);
        let out_cols = wib * 8;
        let (he, ve) = (max_h / ch, max_v / cv);
        let in_cols = out_cols * he;
        let src = |x: usize, y: usize| planes[ci][y.min(h - 1) * w + x.min(w - 1)];
        let drows = hp / max_v * cv;
        let rows_total = total_imcu * cv * 8;
        let mut ds = vec![0u8; out_cols * rows_total];
        for oy in 0..drows {
            for ox in 0..out_cols {
                let v = if he == 1 && ve == 1 {
                    u32::from(src(ox, oy))
                } else if he == 2 && ve == 1 {
                    let bias = (ox & 1) as u32;
                    (u32::from(src(ox * 2, oy)) + u32::from(src(ox * 2 + 1, oy)) + bias) >> 1
                } else if he == 2 && ve == 2 {
                    let bias = if ox & 1 == 0 { 1 } else { 2 };
                    (u32::from(src(ox * 2, oy * 2))
                        + u32::from(src(ox * 2 + 1, oy * 2))
                        + u32::from(src(ox * 2, oy * 2 + 1))
                        + u32::from(src(ox * 2 + 1, oy * 2 + 1))
                        + bias)
                        >> 2
                } else {
                    let n = (he * ve) as u32;
                    let mut sum = 0u32;
                    for vy in 0..ve {
                        for hx in 0..he {
                            sum += u32::from(src(ox * he + hx, oy * ve + vy));
                        }
                    }
                    (sum + n / 2) / n
                };
                let _ = in_cols;
                ds[oy * out_cols + ox] = v as u8;
            }
        }
        // `expand_bottom_edge`: repete a última linha subamostrada até a altura do iMCU.
        if drows > 0 {
            for oy in drows..rows_total {
                ds.copy_within((drows - 1) * out_cols..drows * out_cols, oy * out_cols);
            }
        }
        let q = qtables[s.3].ok_or(EncodeError::Config)?;
        let mut div = [(0u32, 0u32, 0u32); 64];
        for i in 0..64 {
            div[i] = reciprocal(u32::from(q[i]) << 3);
        }
        let bw = wib.div_ceil(ch) * ch;
        let bh = total_imcu * cv;
        let mut coefs = vec![[0i16; 64]; bw * bh];
        for by in 0..hib.min(bh) {
            for bx in 0..wib {
                let mut ws = [0i16; 64];
                for r in 0..8 {
                    for c in 0..8 {
                        ws[r * 8 + c] = i16::from(ds[(by * 8 + r) * out_cols + bx * 8 + c]) - 128;
                    }
                }
                fdct_islow(&mut ws);
                coefs[by * bw + bx] = quantize(&ws, &div);
            }
            // Blocos fictícios à direita: AC zero, DC do último bloco real.
            for bx in wib..bw {
                let dc = coefs[by * bw + bx - 1][0];
                coefs[by * bw + bx] = [0; 64];
                coefs[by * bw + bx][0] = dc;
            }
        }
        // Linhas fictícias embaixo: em cada MCU, o DC do último bloco da linha de cima no MCU.
        for by in hib..bh {
            for mcu in 0..bw / ch {
                let dc = coefs[(by - 1) * bw + mcu * ch + ch - 1][0];
                for k in 0..ch {
                    coefs[by * bw + mcu * ch + k] = [0; 64];
                    coefs[by * bw + mcu * ch + k][0] = dc;
                }
            }
        }
        comps.push(Comp { id: cid, h: ch, v: cv, tq: s.3, td: s.4, ta: s.5, wib, hib, bw, bh, coefs });
    }
    let mcus_per_row = if ncomp == 1 { comps[0].wib } else { w.div_ceil(max_h * 8) };
    let restart_interval = if o.restart_in_rows > 0 {
        (o.restart_in_rows * mcus_per_row).min(65535)
    } else {
        o.restart_interval
    };
    Ok(Prepared {
        comps,
        max_h,
        max_v,
        width: w,
        height: h,
        qtables,
        jfif,
        adobe,
        restart_interval,
        restart_in_rows: o.restart_in_rows,
    })
}

/// Os blocos de uma varredura na ordem dos MCUs: (componente na varredura, índice do bloco), mais
/// o número de MCUs por linha.
pub(crate) fn scan_blocks(p: &Prepared, sc: &[usize]) -> (Vec<Vec<(usize, usize)>>, usize) {
    let mut mcus = Vec::new();
    let per_row;
    if sc.len() == 1 {
        let c = &p.comps[sc[0]];
        per_row = c.wib;
        for by in 0..c.hib {
            for bx in 0..c.wib {
                mcus.push(vec![(0, by * c.bw + bx)]);
            }
        }
    } else {
        let mx = p.width.div_ceil(p.max_h * 8);
        let my = p.height.div_ceil(p.max_v * 8);
        per_row = mx;
        for y in 0..my {
            for x in 0..mx {
                let mut m = Vec::new();
                for (k, &ci) in sc.iter().enumerate() {
                    let c = &p.comps[ci];
                    for vy in 0..c.v {
                        for hx in 0..c.h {
                            m.push((k, (y * c.v + vy) * c.bw + x * c.h + hx));
                        }
                    }
                }
                mcus.push(m);
            }
        }
    }
    (mcus, per_row)
}

fn count_block(block: &[i16; 64], last_dc: i32, dc: &mut [i64; 257], ac: &mut [i64; 257]) -> Result<(), EncodeError> {
    let n = nbits(i32::from(block[0]) - last_dc);
    if n > MAX_COEF_BITS + 1 {
        return Err(EncodeError::BadCoefficient);
    }
    dc[n as usize] += 1;
    let mut r = 0;
    for k in 1..64 {
        let t = i32::from(block[NATURAL_ORDER[k]]);
        if t == 0 {
            r += 1;
        } else {
            while r > 15 {
                ac[0xF0] += 1;
                r -= 16;
            }
            let n = nbits(t);
            if n > MAX_COEF_BITS {
                return Err(EncodeError::BadCoefficient);
            }
            ac[(r << 4) + n as usize] += 1;
            r = 0;
        }
    }
    if r > 0 {
        ac[0] += 1;
    }
    Ok(())
}

fn emit_value(w: &mut BitWriter, v: i32, n: u32) {
    let bits = if v < 0 { (v - 1) as u32 } else { v as u32 };
    w.put(bits, n);
}

fn encode_block(
    w: &mut BitWriter,
    block: &[i16; 64],
    last_dc: i32,
    dc: &([u32; 256], [u8; 256]),
    ac: &([u32; 256], [u8; 256]),
) {
    let diff = i32::from(block[0]) - last_dc;
    let n = nbits(diff);
    w.put(dc.0[n as usize], u32::from(dc.1[n as usize]));
    emit_value(w, diff, n);
    let mut r = 0usize;
    for k in 1..64 {
        let t = i32::from(block[NATURAL_ORDER[k]]);
        if t == 0 {
            r += 1;
            continue;
        }
        while r > 15 {
            w.put(ac.0[0xF0], u32::from(ac.1[0xF0]));
            r -= 16;
        }
        let n = nbits(t);
        let s = (r << 4) + n as usize;
        w.put(ac.0[s], u32::from(ac.1[s]));
        emit_value(w, t, n);
        r = 0;
    }
    if r > 0 {
        w.put(ac.0[0], u32::from(ac.1[0]));
    }
}

fn emit_marker_segment(out: &mut Vec<u8>, m: u8, body: &[u8]) {
    out.extend_from_slice(&[0xFF, m]);
    out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(body);
}

pub(crate) fn emit_dht(out: &mut Vec<u8>, index: u8, t: &HuffTable) {
    let mut body = vec![index];
    body.extend_from_slice(&t.bits[1..17]);
    body.extend_from_slice(&t.vals[..t.bits[1..].iter().map(|&b| usize::from(b)).sum::<usize>()]);
    emit_marker_segment(out, 0xC4, &body);
}

/// O cabeçalho até antes dos DQT: SOI, JFIF/Adobe e os marcadores do Pillow.
pub(crate) fn file_header(o: &EncodeOptions, p: &Prepared, out: &mut Vec<u8>) {
    out.extend_from_slice(&[0xFF, 0xD8]);
    if p.jfif {
        let (unit, xd, yd) = match o.dpi {
            Some((x, y)) if x > 0 && y > 0 => (1u8, x, y),
            _ => (0, 1, 1),
        };
        let mut b = b"JFIF\0".to_vec();
        b.extend_from_slice(&[1, 1, unit]);
        b.extend_from_slice(&xd.to_be_bytes());
        b.extend_from_slice(&yd.to_be_bytes());
        b.extend_from_slice(&[0, 0]);
        emit_marker_segment(out, 0xE0, &b);
    }
    if let Some(t) = p.adobe {
        let mut b = b"Adobe".to_vec();
        b.extend_from_slice(&[0, 100, 0, 0, 0, 0, t]);
        emit_marker_segment(out, 0xEE, &b);
    }
    if !o.exif.is_empty() {
        emit_marker_segment(out, 0xE1, &o.exif);
    }
    out.extend_from_slice(&o.extra);
    if let Some(c) = &o.comment {
        emit_marker_segment(out, 0xFE, c);
    }
}

/// DQT de cada tabela usada (sem repetir) e o SOF.
pub(crate) fn frame_header(p: &Prepared, progressive: bool, out: &mut Vec<u8>) {
    let mut sent = [false; 4];
    let mut prec_any = false;
    for c in &p.comps {
        let q = p.qtables[c.tq].expect("tabela presente");
        let prec = q.iter().any(|&v| v > 255);
        prec_any |= prec;
        if sent[c.tq] {
            continue;
        }
        sent[c.tq] = true;
        let mut b = vec![c.tq as u8 + if prec { 0x10 } else { 0 }];
        for &k in NATURAL_ORDER.iter().take(64) {
            if prec {
                b.push((q[k] >> 8) as u8);
            }
            b.push(q[k] as u8);
        }
        emit_marker_segment(out, 0xDB, &b);
    }
    let baseline = !progressive && !prec_any && p.comps.iter().all(|c| c.td <= 1 && c.ta <= 1);
    let code = if progressive {
        0xC2
    } else if baseline {
        0xC0
    } else {
        0xC1
    };
    let mut b = vec![8];
    b.extend_from_slice(&(p.height as u16).to_be_bytes());
    b.extend_from_slice(&(p.width as u16).to_be_bytes());
    b.push(p.comps.len() as u8);
    for c in &p.comps {
        b.extend_from_slice(&[c.id, ((c.h << 4) + c.v) as u8, c.tq as u8]);
    }
    emit_marker_segment(out, code, &b);
}

pub(crate) fn emit_sos(p: &Prepared, sc: &[usize], ss: u8, se: u8, ah: u8, al: u8, out: &mut Vec<u8>) {
    let mut b = vec![sc.len() as u8];
    for &ci in sc {
        let c = &p.comps[ci];
        let td = if ss == 0 && ah == 0 { c.td } else { 0 };
        let ta = if se != 0 { c.ta } else { 0 };
        b.extend_from_slice(&[c.id, ((td << 4) + ta) as u8]);
    }
    b.extend_from_slice(&[ss, se, (ah << 4) + al]);
    emit_marker_segment(out, 0xDA, &b);
}

pub(crate) fn emit_dri(interval: usize, out: &mut Vec<u8>) {
    emit_marker_segment(out, 0xDD, &(interval as u16).to_be_bytes());
}

/// Codifica a imagem: `pixels` intercalados com 1, 3 ou 4 amostras por pixel.
pub fn encode(o: &EncodeOptions, pixels: &[u8]) -> Result<Vec<u8>, EncodeError> {
    let p = prepare(o, pixels)?;
    let mut out = Vec::new();
    file_header(o, &p, &mut out);
    if o.progressive {
        crate::encode_prog::encode_progressive(&p, &mut out)?;
        out.extend_from_slice(&[0xFF, 0xD9]);
        return Ok(out);
    }
    frame_header(&p, false, &mut out);
    let sc: Vec<usize> = (0..p.comps.len()).collect();
    let (mcus, _) = scan_blocks(&p, &sc);
    // Tabelas: padrão ou otimizadas por uma passada de contagem.
    let mut dc_t: Vec<Option<HuffTable>> = vec![None; 4];
    let mut ac_t: Vec<Option<HuffTable>> = vec![None; 4];
    if o.optimize {
        let mut dcf = vec![[0i64; 257]; 4];
        let mut acf = vec![[0i64; 257]; 4];
        let mut last = vec![0i32; sc.len()];
        let mut togo = p.restart_interval;
        for m in &mcus {
            if p.restart_interval != 0 {
                if togo == 0 {
                    last.iter_mut().for_each(|v| *v = 0);
                    togo = p.restart_interval;
                }
                togo -= 1;
            }
            for &(k, bi) in m {
                let c = &p.comps[sc[k]];
                let b = &c.coefs[bi];
                count_block(b, last[k], &mut dcf[c.td], &mut acf[c.ta])?;
                last[k] = i32::from(b[0]);
            }
        }
        for &ci in &sc {
            let c = &p.comps[ci];
            if dc_t[c.td].is_none() {
                dc_t[c.td] = Some(gen_optimal(&dcf[c.td]));
            }
            if ac_t[c.ta].is_none() {
                ac_t[c.ta] = Some(gen_optimal(&acf[c.ta]));
            }
        }
    } else {
        dc_t[0] = Some(HuffTable::std(&BITS_DC_LUM, &VAL_DC));
        ac_t[0] = Some(HuffTable::std(&BITS_AC_LUM, &VAL_AC_LUM));
        dc_t[1] = Some(HuffTable::std(&BITS_DC_CHR, &VAL_DC));
        ac_t[1] = Some(HuffTable::std(&BITS_AC_CHR, &VAL_AC_CHR));
    }
    // `write_scan_header`: DHT de cada tabela usada, sem repetir, na ordem das componentes.
    let mut sent_dc = [false; 4];
    let mut sent_ac = [false; 4];
    for &ci in &sc {
        let c = &p.comps[ci];
        if !sent_dc[c.td] {
            emit_dht(&mut out, c.td as u8, dc_t[c.td].as_ref().ok_or(EncodeError::Config)?);
            sent_dc[c.td] = true;
        }
        if !sent_ac[c.ta] {
            emit_dht(&mut out, 0x10 + c.ta as u8, ac_t[c.ta].as_ref().ok_or(EncodeError::Config)?);
            sent_ac[c.ta] = true;
        }
    }
    if p.restart_interval != 0 {
        emit_dri(p.restart_interval, &mut out);
    }
    emit_sos(&p, &sc, 0, 63, 0, 0, &mut out);
    let dcd: Vec<Option<_>> = dc_t.iter().map(|t| t.as_ref().map(HuffTable::derive)).collect();
    let acd: Vec<Option<_>> = ac_t.iter().map(|t| t.as_ref().map(HuffTable::derive)).collect();
    let mut w = BitWriter::new();
    let mut last = vec![0i32; sc.len()];
    let mut togo = p.restart_interval;
    let mut next_rst = 0u8;
    for m in &mcus {
        if p.restart_interval != 0 {
            if togo == 0 {
                w.flush();
                w.marker(0xD0 + next_rst);
                next_rst = (next_rst + 1) & 7;
                last.iter_mut().for_each(|v| *v = 0);
                togo = p.restart_interval;
            }
            togo -= 1;
        }
        for &(k, bi) in m {
            let c = &p.comps[sc[k]];
            let b = &c.coefs[bi];
            encode_block(&mut w, b, last[k], dcd[c.td].as_ref().unwrap(), acd[c.ta].as_ref().unwrap());
            last[k] = i32::from(b[0]);
        }
    }
    w.flush();
    out.extend_from_slice(&w.out);
    out.extend_from_slice(&[0xFF, 0xD9]);
    Ok(out)
}
