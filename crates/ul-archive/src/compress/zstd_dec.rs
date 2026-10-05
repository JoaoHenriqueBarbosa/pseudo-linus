//! Decodificador zstd (RFC 8878), porte do `lib/decompress` do zstd 1.5.7.
//!
//! O CLI precisa de mais que os bytes descomprimidos: precisa do comportamento observável do C.
//! O `fileio.c` só grava o que cada chamada de `ZSTD_decompressStream` produziu sem erro, então a
//! saída parcial de um arquivo corrompido ou truncado depende de como o C fatia a entrada e a saída
//! entre chamadas, de quando ele toma o atalho de passada única (frame inteiro na entrada e tamanho
//! conhecido que cabe na saída) e de qual erro cada verificação levanta. Por isso a máquina de estados
//! aqui segue a do `ZSTD_decompressStream` e do `ZSTD_decompressContinue`, e os decodificadores de
//! literais, Huffman, FSE e sequências seguem as verificações do C na mesma ordem.
//!
//! Simplificação consciente: o histórico das cópias é uma janela linear (dicionário mais a saída
//! recente do frame) em vez do buffer circular com `extDict` do C. Pra streams válidos o resultado é
//! idêntico; num stream corrompido com offset além da janela o C às vezes alcança mais bytes, e aqui o
//! erro sai mais cedo, com o mesmo nome (`Data corruption detected`).

/// Número mágico de um frame zstd.
pub const MAGIC: u32 = 0xFD2F_B528;
/// Número mágico de um dicionário no formato do `zstd --train`.
pub const MAGIC_DICTIONARY: u32 = 0xEC30_A437;
/// Primeiro dos 16 números mágicos de frame pulável.
pub const MAGIC_SKIPPABLE_START: u32 = 0x184D_2A50;
/// Máscara que reduz os 16 números mágicos puláveis ao primeiro.
pub const MAGIC_SKIPPABLE_MASK: u32 = 0xFFFF_FFF0;
/// Tamanho máximo de um bloco.
pub const BLOCKSIZE_MAX: usize = 1 << 17;
/// `ZSTD_DStreamInSize()`: o tamanho dos pedaços que o `fileio` lê.
pub const DSTREAM_IN_SIZE: usize = BLOCKSIZE_MAX + BLOCK_HEADER_SIZE;
/// `ZSTD_DStreamOutSize()`: o buffer de saída de cada chamada.
pub const DSTREAM_OUT_SIZE: usize = BLOCKSIZE_MAX;
/// Maior cabeçalho de frame possível.
pub const FRAMEHEADERSIZE_MAX: usize = 18;
/// Tamanho desconhecido do conteúdo.
pub const CONTENTSIZE_UNKNOWN: u64 = u64::MAX;

const FRAMEHEADERSIZE_PREFIX: usize = 5;
const FRAMEHEADERSIZE_MIN: usize = 6;
const SKIPPABLEHEADERSIZE: usize = 8;
const BLOCK_HEADER_SIZE: usize = 3;
const WINDOWLOG_MAX: u32 = 31;
const WINDOWLOG_ABSOLUTEMIN: u32 = 10;
const WILDCOPY_OVERLENGTH: usize = 32;
const MIN_CBLOCK_SIZE: usize = 2;
const MIN_LITERALS_FOR_4_STREAMS: usize = 6;
const LONGNBSEQ: usize = 0x7F00;
const HUF_TABLELOG_MAX: u32 = 12;
const FSE_TABLELOG_ABSOLUTE_MAX: i32 = 15;
const FSE_MIN_TABLELOG: i32 = 5;
const NO_FORWARD_PROGRESS_MAX: u32 = 16;
const REP_START: [u64; 3] = [1, 4, 8];

const MAX_LL: usize = 35;
const MAX_ML: usize = 52;
const MAX_OFF: usize = 31;
const LL_FSE_LOG: u32 = 9;
const ML_FSE_LOG: u32 = 9;
const OFF_FSE_LOG: u32 = 8;

const LL_BITS: [u8; MAX_LL + 1] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 3, 4, 6, 7, 8, 9, 10, 11, 12, 13, 14,
    15, 16,
];
const LL_BASE: [u32; MAX_LL + 1] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 20, 22, 24, 28, 32, 40, 48, 64, 0x80, 0x100,
    0x200, 0x400, 0x800, 0x1000, 0x2000, 0x4000, 0x8000, 0x10000,
];
const LL_DEFAULT_NORM: [i16; MAX_LL + 1] = [
    4, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 2, 1, 1, 1, 1, 1, -1, -1,
    -1, -1,
];
const ML_BITS: [u8; MAX_ML + 1] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1,
    2, 2, 3, 3, 4, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
];
const ML_BASE: [u32; MAX_ML + 1] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
    31, 32, 33, 34, 35, 37, 39, 41, 43, 47, 51, 59, 67, 83, 99, 0x83, 0x103, 0x203, 0x403, 0x803, 0x1003,
    0x2003, 0x4003, 0x8003, 0x10003,
];
const ML_DEFAULT_NORM: [i16; MAX_ML + 1] = [
    1, 4, 3, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1, -1, -1,
];
const OF_DEFAULT_NORM: [i16; 29] = [
    1, 1, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1,
];

fn of_base(code: usize) -> u32 {
    match code {
        0 => 0,
        1 => 1,
        n => (1u32 << n) - 3,
    }
}

fn of_bits(code: usize) -> u8 {
    code as u8
}

/// Os códigos de erro do zstd que o decodificador pode levantar, com o nome do `ZSTD_getErrorName`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Generic,
    PrefixUnknown,
    FrameParameterUnsupported,
    WindowTooLarge,
    Corruption,
    ChecksumWrong,
    LiteralsHeaderWrong,
    MemoryAllocation,
    TableLogTooLarge,
    MaxSymbolValueTooSmall,
    DictionaryCorrupted,
    DictionaryWrong,
    DstSizeTooSmall,
    SrcSizeWrong,
    NoProgressDestFull,
    NoProgressInputEmpty,
}

impl Error {
    pub fn name(self) -> &'static str {
        match self {
            Error::Generic => "Error (generic)",
            Error::PrefixUnknown => "Unknown frame descriptor",
            Error::FrameParameterUnsupported => "Unsupported frame parameter",
            Error::WindowTooLarge => "Frame requires too much memory for decoding",
            Error::Corruption => "Data corruption detected",
            Error::ChecksumWrong => "Restored data doesn't match checksum",
            Error::LiteralsHeaderWrong => "Header of Literals' block doesn't respect format specification",
            Error::MemoryAllocation => "Allocation error : not enough memory",
            Error::TableLogTooLarge => "tableLog requires too much memory : unsupported",
            Error::MaxSymbolValueTooSmall => "Specified maxSymbolValue is too small",
            Error::DictionaryCorrupted => "Dictionary is corrupted",
            Error::DictionaryWrong => "Dictionary mismatch",
            Error::DstSizeTooSmall => "Destination buffer is too small",
            Error::SrcSizeWrong => "Src size is incorrect",
            Error::NoProgressDestFull => {
                "Operation made no progress over multiple calls, due to output buffer being full"
            }
            Error::NoProgressInputEmpty => {
                "Operation made no progress over multiple calls, due to input being empty"
            }
        }
    }
}

type Res<T> = Result<T, Error>;

fn le16(b: &[u8]) -> u32 {
    u32::from(b[0]) | u32::from(b[1]) << 8
}

fn le24(b: &[u8]) -> u32 {
    le16(b) | u32::from(b[2]) << 16
}

/// Lê 4 bytes little endian.
pub fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn le64(b: &[u8]) -> u64 {
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

fn highbit(v: u32) -> u32 {
    31 - v.leading_zeros()
}

// XXH64, o checksum de conteúdo do frame (os 32 bits baixos vão no fim do frame).

const P1: u64 = 0x9E37_79B1_85EB_CA87;
const P2: u64 = 0xC2B2_AE3D_27D4_EB4F;
const P3: u64 = 0x1656_67B1_9E37_79F9;
const P4: u64 = 0x85EB_CA77_C2B2_AE63;
const P5: u64 = 0x27D4_EB2F_1656_67C5;

fn xxh_round(acc: u64, input: u64) -> u64 {
    acc.wrapping_add(input.wrapping_mul(P2)).rotate_left(31).wrapping_mul(P1)
}

fn xxh_merge(acc: u64, val: u64) -> u64 {
    (acc ^ xxh_round(0, val)).wrapping_mul(P1).wrapping_add(P4)
}

/// Estado incremental do XXH64.
#[derive(Clone)]
pub struct Xxh64 {
    v: [u64; 4],
    total: u64,
    buf: [u8; 32],
    len: usize,
    seed: u64,
}

impl Xxh64 {
    pub fn new(seed: u64) -> Xxh64 {
        Xxh64 {
            v: [seed.wrapping_add(P1).wrapping_add(P2), seed.wrapping_add(P2), seed, seed.wrapping_sub(P1)],
            total: 0,
            buf: [0; 32],
            len: 0,
            seed,
        }
    }

    fn stripe(&mut self, s: &[u8]) {
        for (i, v) in self.v.iter_mut().enumerate() {
            *v = xxh_round(*v, le64(&s[i * 8..]));
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total += data.len() as u64;
        if self.len > 0 {
            let take = (32 - self.len).min(data.len());
            self.buf[self.len..self.len + take].copy_from_slice(&data[..take]);
            self.len += take;
            data = &data[take..];
            if self.len < 32 {
                return;
            }
            let b = self.buf;
            self.stripe(&b);
            self.len = 0;
        }
        while data.len() >= 32 {
            let (s, rest) = data.split_at(32);
            self.stripe(s);
            data = rest;
        }
        self.buf[..data.len()].copy_from_slice(data);
        self.len = data.len();
    }

    pub fn digest(&self) -> u64 {
        let mut h = if self.total >= 32 {
            let [v1, v2, v3, v4] = self.v;
            let mut h = v1
                .rotate_left(1)
                .wrapping_add(v2.rotate_left(7))
                .wrapping_add(v3.rotate_left(12))
                .wrapping_add(v4.rotate_left(18));
            for v in self.v {
                h = xxh_merge(h, v);
            }
            h
        } else {
            self.seed.wrapping_add(P5)
        };
        h = h.wrapping_add(self.total);
        let mut rest = &self.buf[..self.len];
        while rest.len() >= 8 {
            h ^= xxh_round(0, le64(rest));
            h = h.rotate_left(27).wrapping_mul(P1).wrapping_add(P4);
            rest = &rest[8..];
        }
        if rest.len() >= 4 {
            h ^= u64::from(le32(rest)).wrapping_mul(P1);
            h = h.rotate_left(23).wrapping_mul(P2).wrapping_add(P3);
            rest = &rest[4..];
        }
        for &b in rest {
            h ^= u64::from(b).wrapping_mul(P5);
            h = h.rotate_left(11).wrapping_mul(P1);
        }
        h ^= h >> 33;
        h = h.wrapping_mul(P2);
        h ^= h >> 29;
        h = h.wrapping_mul(P3);
        h ^ (h >> 32)
    }
}

/// Bitstream lido de trás pra frente (o `BIT_DStream_t`). `pos` é o número de bits ainda não lidos;
/// bits abaixo do começo do buffer valem zero, como no deslocamento do contêiner do C, e `pos`
/// negativo é o estado de estouro.
struct BitIn<'a> {
    src: &'a [u8],
    pos: i64,
}

impl<'a> BitIn<'a> {
    fn new(src: &'a [u8]) -> Res<BitIn<'a>> {
        let Some(&last) = src.last() else { return Err(Error::SrcSizeWrong) };
        if last == 0 {
            return Err(Error::Corruption);
        }
        let pos = src.len() as i64 * 8 - 8 + i64::from(highbit(u32::from(last)));
        Ok(BitIn { src, pos })
    }

    /// Os `n` bits a partir do bit `lo` (n <= 56).
    fn bits(&self, lo: i64, n: u32) -> u64 {
        if n == 0 || lo + i64::from(n) <= 0 {
            return 0;
        }
        if lo < 0 {
            return self.bits(0, (lo + i64::from(n)) as u32) << (-lo);
        }
        let byte = (lo / 8) as usize;
        let mut word = [0u8; 8];
        let end = (byte + 8).min(self.src.len());
        if byte < end {
            word[..end - byte].copy_from_slice(&self.src[byte..end]);
        }
        (u64::from_le_bytes(word) >> (lo % 8)) & ((1u64 << n) - 1)
    }

    fn peek(&self, n: u32) -> u64 {
        self.bits(self.pos - i64::from(n), n)
    }

    fn read(&mut self, n: u32) -> u64 {
        let v = self.peek(n);
        self.pos -= i64::from(n);
        v
    }

    fn overflow(&self) -> bool {
        self.pos < 0
    }

    fn finished(&self) -> bool {
        self.pos == 0
    }
}

/// Distribuição normalizada lida do cabeçalho FSE (`FSE_readNCount`).
struct NCount {
    norm: Vec<i16>,
    table_log: u32,
    size: usize,
}

fn read_ncount(src: &[u8], max_sv: usize) -> Res<NCount> {
    if src.len() < 8 {
        let mut buf = [0u8; 8];
        buf[..src.len()].copy_from_slice(src);
        let n = read_ncount_body(&buf, max_sv)?;
        if n.size > src.len() {
            return Err(Error::Corruption);
        }
        return Ok(n);
    }
    read_ncount_body(src, max_sv)
}

fn read_ncount_body(b: &[u8], max_sv: usize) -> Res<NCount> {
    let iend = b.len() as i64;
    let rd = |p: i64| le32(&b[p as usize..]);
    let max_sv1 = max_sv + 1;
    let mut norm = vec![0i16; max_sv1];
    let mut ip: i64 = 0;
    let mut bit_stream = rd(ip);
    let mut nb_bits = (bit_stream & 0xF) as i32 + FSE_MIN_TABLELOG;
    if nb_bits > FSE_TABLELOG_ABSOLUTE_MAX {
        return Err(Error::TableLogTooLarge);
    }
    bit_stream >>= 4;
    let mut bit_count: i32 = 4;
    let table_log = nb_bits as u32;
    let mut remaining: i32 = (1 << nb_bits) + 1;
    let mut threshold: i32 = 1 << nb_bits;
    nb_bits += 1;
    let mut charnum = 0usize;
    let mut previous0 = false;
    // O avanço do ponteiro do C: anda os bytes inteiros consumidos, ou encosta nos últimos 4.
    let advance = |ip: &mut i64, bit_count: &mut i32| {
        if *ip + 7 <= iend || *ip + i64::from(*bit_count >> 3) + 4 <= iend {
            *ip += i64::from(*bit_count >> 3);
            *bit_count &= 7;
        } else {
            *bit_count -= (8 * (iend - 4 - *ip)) as i32;
            *bit_count &= 31;
            *ip = iend - 4;
        }
    };
    loop {
        if previous0 {
            let mut repeats = ((!bit_stream | 0x8000_0000).trailing_zeros() >> 1) as usize;
            while repeats >= 12 {
                charnum += 3 * 12;
                if ip + 7 <= iend {
                    ip += 3;
                } else {
                    bit_count -= (8 * (iend - 7 - ip)) as i32;
                    bit_count &= 31;
                    ip = iend - 4;
                }
                bit_stream = rd(ip) >> bit_count;
                repeats = ((!bit_stream | 0x8000_0000).trailing_zeros() >> 1) as usize;
            }
            charnum += 3 * repeats;
            bit_stream >>= 2 * repeats;
            bit_count += 2 * repeats as i32;
            charnum += (bit_stream & 3) as usize;
            bit_count += 2;
            if charnum >= max_sv1 {
                break;
            }
            advance(&mut ip, &mut bit_count);
            bit_stream = rd(ip) >> bit_count;
        }
        let max = (2 * threshold - 1) - remaining;
        let mut count: i32;
        if i64::from(bit_stream & (threshold as u32 - 1)) < i64::from(max) {
            count = (bit_stream & (threshold as u32 - 1)) as i32;
            bit_count += nb_bits - 1;
        } else {
            count = (bit_stream & (2 * threshold as u32 - 1)) as i32;
            if count >= threshold {
                count -= max;
            }
            bit_count += nb_bits;
        }
        count -= 1;
        if count >= 0 {
            remaining -= count;
        } else {
            remaining += count;
        }
        norm[charnum] = count as i16;
        charnum += 1;
        previous0 = count == 0;
        if remaining < threshold {
            if remaining <= 1 {
                break;
            }
            nb_bits = highbit(remaining as u32) as i32 + 1;
            threshold = 1 << (nb_bits - 1);
        }
        if charnum >= max_sv1 {
            break;
        }
        advance(&mut ip, &mut bit_count);
        bit_stream = rd(ip) >> bit_count;
    }
    if remaining != 1 {
        return Err(Error::Corruption);
    }
    if charnum > max_sv1 {
        return Err(Error::MaxSymbolValueTooSmall);
    }
    if bit_count > 32 {
        return Err(Error::Corruption);
    }
    ip += i64::from((bit_count + 7) >> 3);
    norm.truncate(charnum);
    Ok(NCount { norm, table_log, size: ip as usize })
}

/// Uma célula de tabela de decodificação FSE: símbolo, bits do próximo estado e base do próximo estado.
#[derive(Clone, Copy, Default)]
struct FseCell {
    symbol: u16,
    nb_bits: u8,
    next: u16,
}

/// `FSE_buildDTable` e `ZSTD_buildFSETable`: espalha os símbolos e calcula as transições.
fn build_fse(norm: &[i16], table_log: u32) -> Vec<FseCell> {
    let size = 1usize << table_log;
    let mut cells = vec![FseCell::default(); size];
    let mut next_of = vec![0u32; norm.len()];
    let mut high = size as isize - 1;
    for (s, &n) in norm.iter().enumerate() {
        if n == -1 {
            if high >= 0 {
                cells[high as usize].symbol = s as u16;
            }
            high -= 1;
            next_of[s] = 1;
        } else {
            next_of[s] = n.max(0) as u32;
        }
    }
    let step = (size >> 1) + (size >> 3) + 3;
    let mask = size - 1;
    let mut pos = 0usize;
    for (s, &n) in norm.iter().enumerate() {
        for _ in 0..n.max(0) {
            cells[pos].symbol = s as u16;
            pos = (pos + step) & mask;
            while pos as isize > high {
                pos = (pos + step) & mask;
            }
        }
    }
    for cell in cells.iter_mut() {
        let s = cell.symbol as usize;
        let ns = next_of[s];
        next_of[s] += 1;
        let nb = table_log.saturating_sub(highbit(ns.max(1)));
        cell.nb_bits = nb as u8;
        cell.next = ((ns << nb) as usize).wrapping_sub(size) as u16;
    }
    cells
}

/// Célula de tabela de sequências (`ZSTD_seqSymbol`).
#[derive(Clone, Copy, Default)]
struct SeqCell {
    base: u32,
    add_bits: u8,
    nb_bits: u8,
    next: u16,
}

#[derive(Clone, Default)]
struct SeqTable {
    log: u32,
    cells: Vec<SeqCell>,
}

fn seq_table(norm: &[i16], table_log: u32, base: &[u32], bits: &[u8]) -> SeqTable {
    let cells = build_fse(norm, table_log)
        .into_iter()
        .map(|c| SeqCell {
            base: base[c.symbol as usize],
            add_bits: bits[c.symbol as usize],
            nb_bits: c.nb_bits,
            next: c.next,
        })
        .collect();
    SeqTable { log: table_log, cells }
}

fn seq_table_rle(base: u32, add_bits: u8) -> SeqTable {
    SeqTable { log: 0, cells: vec![SeqCell { base, add_bits, nb_bits: 0, next: 0 }] }
}

/// Tabela Huffman de um símbolo por consulta: índice de `log` bits dá (símbolo, bits consumidos).
#[derive(Clone)]
struct Huf {
    log: u32,
    cells: Vec<(u8, u8)>,
}

/// Decodifica os pesos Huffman comprimidos com FSE (`FSE_decompress_wksp` com `maxLog` 6).
fn fse_weights(src: &[u8], out: &mut [u8]) -> Res<usize> {
    let nc = read_ncount(src, 255)?;
    if nc.table_log > 6 {
        return Err(Error::TableLogTooLarge);
    }
    let Some(rest) = src.get(nc.size..) else { return Err(Error::Corruption) };
    let cells = build_fse(&nc.norm, nc.table_log);
    let mut bits = BitIn::new(rest)?;
    let mut s1 = bits.read(nc.table_log) as usize;
    let mut s2 = bits.read(nc.table_log) as usize;
    if bits.overflow() {
        return Err(Error::Corruption);
    }
    let omax = out.len();
    let mut op = 0usize;
    let step = |state: &mut usize, bits: &mut BitIn| -> u8 {
        let c = cells[*state];
        *state = c.next as usize + bits.read(u32::from(c.nb_bits)) as usize;
        c.symbol as u8
    };
    loop {
        if op + 2 > omax {
            return Err(Error::DstSizeTooSmall);
        }
        out[op] = step(&mut s1, &mut bits);
        op += 1;
        if bits.overflow() {
            out[op] = step(&mut s2, &mut bits);
            op += 1;
            break;
        }
        if op + 2 > omax {
            return Err(Error::DstSizeTooSmall);
        }
        out[op] = step(&mut s2, &mut bits);
        op += 1;
        if bits.overflow() {
            out[op] = step(&mut s1, &mut bits);
            op += 1;
            break;
        }
    }
    Ok(op)
}

/// `HUF_readStats` seguido do `HUF_readDTableX1`: lê a árvore e monta a tabela. Devolve os bytes lidos.
fn read_huf(src: &[u8]) -> Res<(Huf, usize)> {
    if src.is_empty() {
        return Err(Error::SrcSizeWrong);
    }
    let mut weights = [0u8; 256];
    let mut isize = src[0] as usize;
    let osize;
    if isize >= 128 {
        osize = isize - 127;
        isize = osize.div_ceil(2);
        if isize + 1 > src.len() {
            return Err(Error::SrcSizeWrong);
        }
        if osize >= 256 {
            return Err(Error::Corruption);
        }
        for n in (0..osize).step_by(2) {
            weights[n] = src[1 + n / 2] >> 4;
            weights[n + 1] = src[1 + n / 2] & 15;
        }
    } else {
        if isize + 1 > src.len() {
            return Err(Error::SrcSizeWrong);
        }
        osize = fse_weights(&src[1..1 + isize], &mut weights[..255])?;
    }
    let mut rank = [0u32; HUF_TABLELOG_MAX as usize + 1];
    let mut total = 0u32;
    for &w in &weights[..osize] {
        if u32::from(w) > HUF_TABLELOG_MAX {
            return Err(Error::Corruption);
        }
        rank[w as usize] += 1;
        total += (1u32 << w) >> 1;
    }
    if total == 0 {
        return Err(Error::Corruption);
    }
    let log = highbit(total) + 1;
    if log > HUF_TABLELOG_MAX {
        return Err(Error::Corruption);
    }
    let rest = (1u32 << log) - total;
    let last = highbit(rest) + 1;
    if 1u32 << highbit(rest) != rest {
        return Err(Error::Corruption);
    }
    weights[osize] = last as u8;
    rank[last as usize] += 1;
    if rank[1] < 2 || rank[1] & 1 != 0 {
        return Err(Error::Corruption);
    }
    let nb_symbols = osize + 1;
    let mut cells = vec![(0u8, 0u8); 1 << log];
    let mut at = 0usize;
    for w in 1..=log {
        let length = (1usize << w) >> 1;
        let nb = (log + 1 - w) as u8;
        for (s, &sw) in weights[..nb_symbols].iter().enumerate() {
            if u32::from(sw) == w {
                cells[at..at + length].fill((s as u8, nb));
                at += length;
            }
        }
    }
    Ok((Huf { log, cells }, isize + 1))
}

fn huf_stream(h: &Huf, src: &[u8], n: usize, out: &mut Vec<u8>) -> Res<()> {
    let mut bits = BitIn::new(src)?;
    for _ in 0..n {
        let (s, nb) = h.cells[bits.peek(h.log) as usize];
        out.push(s);
        bits.pos -= i64::from(nb);
    }
    if !bits.finished() {
        return Err(Error::Corruption);
    }
    Ok(())
}

fn huf_4streams(h: &Huf, src: &[u8], n: usize, out: &mut Vec<u8>) -> Res<()> {
    if src.len() < 10 || n < 6 {
        return Err(Error::Corruption);
    }
    let l1 = le16(src) as usize;
    let l2 = le16(&src[2..]) as usize;
    let l3 = le16(&src[4..]) as usize;
    let Some(l4) = src.len().checked_sub(l1 + l2 + l3 + 6) else { return Err(Error::Corruption) };
    let seg = n.div_ceil(4);
    if 3 * seg > n {
        return Err(Error::Corruption);
    }
    let mut at = 6;
    for (i, len) in [l1, l2, l3, l4].into_iter().enumerate() {
        let count = if i < 3 { seg } else { n - 3 * seg };
        huf_stream(h, &src[at..at + len], count, out)?;
        at += len;
    }
    Ok(())
}

/// Dicionário carregado (`ZSTD_DDict`): conteúdo, id e, se for do formato do `--train`, as tabelas.
#[derive(Clone)]
pub struct Dict {
    content: Vec<u8>,
    id: u32,
    entropy: Option<Entropy>,
}

#[derive(Clone)]
struct Entropy {
    huf: Huf,
    ll: SeqTable,
    of: SeqTable,
    ml: SeqTable,
    rep: [u64; 3],
}

impl Dict {
    /// `ZSTD_createDDict`: um dicionário com o número mágico do formato é lido com as tabelas; qualquer
    /// outro conteúdo é usado cru.
    pub fn parse(bytes: &[u8]) -> Res<Dict> {
        if bytes.len() < 8 || le32(bytes) != MAGIC_DICTIONARY {
            return Ok(Dict { content: bytes.to_vec(), id: 0, entropy: None });
        }
        let id = le32(&bytes[4..]);
        let corrupt = |_| Error::DictionaryCorrupted;
        if bytes.len() <= 8 {
            return Err(Error::DictionaryCorrupted);
        }
        let mut p = 8;
        let (huf, hsize) = read_huf(&bytes[p..]).map_err(corrupt)?;
        p += hsize;
        let mut table = |max: usize, max_log: u32, base: &[u32], bits: &[u8]| -> Res<SeqTable> {
            let nc = read_ncount(&bytes[p..], max).map_err(corrupt)?;
            if nc.norm.len() > max + 1 || nc.table_log > max_log {
                return Err(Error::DictionaryCorrupted);
            }
            p += nc.size;
            Ok(seq_table(&nc.norm, nc.table_log, base, bits))
        };
        let ofb: Vec<u32> = (0..=MAX_OFF).map(of_base).collect();
        let ofbits: Vec<u8> = (0..=MAX_OFF).map(of_bits).collect();
        let of = table(MAX_OFF, OFF_FSE_LOG, &ofb, &ofbits)?;
        let ml = table(MAX_ML, ML_FSE_LOG, &ML_BASE, &ML_BITS)?;
        let ll = table(MAX_LL, LL_FSE_LOG, &LL_BASE, &LL_BITS)?;
        if p + 12 > bytes.len() {
            return Err(Error::DictionaryCorrupted);
        }
        let content_size = (bytes.len() - (p + 12)) as u64;
        let mut rep = [0u64; 3];
        for r in rep.iter_mut() {
            let v = u64::from(le32(&bytes[p..]));
            p += 4;
            if v == 0 || v > content_size {
                return Err(Error::DictionaryCorrupted);
            }
            *r = v;
        }
        Ok(Dict { content: bytes[p..].to_vec(), id, entropy: Some(Entropy { huf, ll, of, ml, rep }) })
    }

    /// O id gravado no dicionário (0 pra conteúdo cru).
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Conteúdo usado como prefixo, sem interpretar o número mágico (`ZSTD_DCtx_refPrefix`, do
    /// `--patch-from`).
    pub fn raw(bytes: &[u8]) -> Dict {
        Dict { content: bytes.to_vec(), id: 0, entropy: None }
    }
}

/// Parâmetros do cabeçalho de frame (`ZSTD_FrameHeader`).
#[derive(Clone, Copy, Default)]
pub struct FrameHeader {
    pub content_size: u64,
    pub window_size: u64,
    pub block_size_max: usize,
    pub skippable: bool,
    pub header_size: usize,
    pub dict_id: u32,
    pub checksum: bool,
}

pub fn frame_header_size(src: &[u8]) -> Res<usize> {
    if src.len() < FRAMEHEADERSIZE_PREFIX {
        return Err(Error::SrcSizeWrong);
    }
    let fhd = src[4];
    let did = [0, 1, 2, 4][(fhd & 3) as usize];
    let single = (fhd >> 5) & 1 == 1;
    let fcs_id = fhd >> 6;
    let fcs = [0, 2, 4, 8][fcs_id as usize];
    Ok(FRAMEHEADERSIZE_PREFIX + usize::from(!single) + did + fcs + usize::from(single && fcs_id == 0))
}

/// `ZSTD_getFrameHeader_advanced`: `Ok(0)` com o cabeçalho completo, `Ok(n)` quando faltam bytes (n é
/// quanto precisa ao todo).
pub fn get_frame_header(fh: &mut FrameHeader, src: &[u8]) -> Res<usize> {
    if src.len() < FRAMEHEADERSIZE_PREFIX {
        if !src.is_empty() {
            let mut h = MAGIC.to_le_bytes();
            let n = src.len().min(4);
            h[..n].copy_from_slice(&src[..n]);
            if le32(&h) != MAGIC {
                let mut h = MAGIC_SKIPPABLE_START.to_le_bytes();
                h[..n].copy_from_slice(&src[..n]);
                if le32(&h) & MAGIC_SKIPPABLE_MASK != MAGIC_SKIPPABLE_START {
                    return Err(Error::PrefixUnknown);
                }
            }
        }
        return Ok(FRAMEHEADERSIZE_PREFIX);
    }
    *fh = FrameHeader::default();
    let magic = le32(src);
    if magic != MAGIC {
        if magic & MAGIC_SKIPPABLE_MASK == MAGIC_SKIPPABLE_START {
            if src.len() < SKIPPABLEHEADERSIZE {
                return Ok(SKIPPABLEHEADERSIZE);
            }
            fh.skippable = true;
            fh.dict_id = magic - MAGIC_SKIPPABLE_START;
            fh.header_size = SKIPPABLEHEADERSIZE;
            fh.content_size = u64::from(le32(&src[4..]));
            return Ok(0);
        }
        return Err(Error::PrefixUnknown);
    }
    let fhsize = frame_header_size(src)?;
    if src.len() < fhsize {
        return Ok(fhsize);
    }
    fh.header_size = fhsize;
    let fhd = src[4];
    let mut pos = FRAMEHEADERSIZE_PREFIX;
    let single = (fhd >> 5) & 1 == 1;
    let fcs_id = fhd >> 6;
    if fhd & 0x08 != 0 {
        return Err(Error::FrameParameterUnsupported);
    }
    let mut window = 0u64;
    if !single {
        let wl = src[pos];
        pos += 1;
        let log = u32::from(wl >> 3) + WINDOWLOG_ABSOLUTEMIN;
        if log > WINDOWLOG_MAX {
            return Err(Error::WindowTooLarge);
        }
        window = 1u64 << log;
        window += (window >> 3) * u64::from(wl & 7);
    }
    let dict_id = match fhd & 3 {
        0 => 0,
        1 => {
            pos += 1;
            u32::from(src[pos - 1])
        }
        2 => {
            pos += 2;
            le16(&src[pos - 2..])
        }
        _ => {
            pos += 4;
            le32(&src[pos - 4..])
        }
    };
    let content_size = match fcs_id {
        0 => {
            if single {
                u64::from(src[pos])
            } else {
                CONTENTSIZE_UNKNOWN
            }
        }
        1 => u64::from(le16(&src[pos..])) + 256,
        2 => u64::from(le32(&src[pos..])),
        _ => le64(&src[pos..]),
    };
    if single {
        window = content_size;
    }
    fh.content_size = content_size;
    fh.window_size = window;
    fh.block_size_max = window.min(BLOCKSIZE_MAX as u64) as usize;
    fh.dict_id = dict_id;
    fh.checksum = (fhd >> 2) & 1 == 1;
    Ok(0)
}

/// Cabeçalho de bloco: (tamanho comprimido, último, tipo, tamanho regenerado do RLE).
fn block_size(src: &[u8]) -> Res<(usize, bool, u8, usize)> {
    if src.len() < BLOCK_HEADER_SIZE {
        return Err(Error::SrcSizeWrong);
    }
    let h = le24(src);
    let size = (h >> 3) as usize;
    let last = h & 1 == 1;
    let btype = ((h >> 1) & 3) as u8;
    match btype {
        BT_RLE => Ok((1, last, btype, size)),
        BT_RESERVED => Err(Error::Corruption),
        _ => Ok((size, last, btype, size)),
    }
}

const BT_RAW: u8 = 0;
const BT_RLE: u8 = 1;
const BT_COMPRESSED: u8 = 2;
const BT_RESERVED: u8 = 3;

/// `ZSTD_findFrameCompressedSize`: o tamanho do frame que começa em `src`, se ele estiver inteiro.
pub fn find_frame_compressed_size(src: &[u8]) -> Res<usize> {
    if src.len() >= SKIPPABLEHEADERSIZE && le32(src) & MAGIC_SKIPPABLE_MASK == MAGIC_SKIPPABLE_START {
        let size = le32(&src[4..]) as usize;
        let total = size.checked_add(SKIPPABLEHEADERSIZE).ok_or(Error::FrameParameterUnsupported)?;
        if total > src.len() {
            return Err(Error::SrcSizeWrong);
        }
        return Ok(total);
    }
    let mut fh = FrameHeader::default();
    if get_frame_header(&mut fh, src)? > 0 {
        return Err(Error::SrcSizeWrong);
    }
    let mut ip = fh.header_size;
    loop {
        let (csize, last, _, _) = block_size(&src[ip..])?;
        if BLOCK_HEADER_SIZE + csize > src.len() - ip {
            return Err(Error::SrcSizeWrong);
        }
        ip += BLOCK_HEADER_SIZE + csize;
        if last {
            break;
        }
    }
    if fh.checksum {
        if src.len() - ip < 4 {
            return Err(Error::SrcSizeWrong);
        }
        ip += 4;
    }
    Ok(ip)
}

/// `ZSTD_findDecompressedSize`: a soma dos tamanhos declarados dos frames. `Ok(None)` quando algum
/// frame não declara o tamanho; `Err` quando os dados não são frames válidos.
pub fn find_decompressed_size(mut src: &[u8]) -> Result<Option<u64>, Error> {
    let mut total = 0u64;
    let mut unknown = false;
    while src.len() >= FRAMEHEADERSIZE_MIN {
        let magic = le32(src);
        if magic & MAGIC_SKIPPABLE_MASK == MAGIC_SKIPPABLE_START {
            let n = find_frame_compressed_size(src)?;
            src = &src[n..];
            continue;
        }
        let mut fh = FrameHeader::default();
        if get_frame_header(&mut fh, src)? != 0 {
            return Err(Error::SrcSizeWrong);
        }
        if fh.content_size == CONTENTSIZE_UNKNOWN {
            unknown = true;
        } else {
            total = total.checked_add(fh.content_size).ok_or(Error::SrcSizeWrong)?;
        }
        let n = find_frame_compressed_size(src)?;
        src = &src[n..];
    }
    if !src.is_empty() {
        return Err(Error::SrcSizeWrong);
    }
    Ok(if unknown { None } else { Some(total) })
}

/// Descompressão de um buffer inteiro, com todos os frames (o `ZSTD_decompress_usingDict`).
pub fn decompress_all(src: &[u8], dict: Option<&Dict>) -> Result<Vec<u8>, Error> {
    let mut d = DStream::new(1 << WINDOWLOG_MAX);
    d.set_dict(dict.cloned());
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < src.len() {
        d.reset_session();
        loop {
            let hint = d.decompress_stream(&mut out, usize::MAX, src, &mut pos)?;
            if hint == 0 {
                break;
            }
            if pos >= src.len() {
                return Err(Error::SrcSizeWrong);
            }
        }
    }
    Ok(out)
}

/// Estágios do `ZSTD_decompressContinue`. Os de cabeçalho de frame e de frame pulável do C não
/// aparecem: o caminho de stream decodifica o cabeçalho direto do buffer acumulado.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    GetFrameHeaderSize,
    DecodeBlockHeader,
    DecompressBlock,
    DecompressLastBlock,
    CheckChecksum,
    SkipFrame,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StreamStage {
    Init,
    LoadHeader,
    Read,
    Load,
    Flush,
}

/// O contexto de descompressão (`ZSTD_DCtx` usado como `ZSTD_DStream`).
pub struct DStream {
    // Configuração.
    max_window: u64,
    ignore_checksum: bool,
    dict: Option<Dict>,
    // Frame.
    fp: FrameHeader,
    stage: Stage,
    expected: usize,
    btype: u8,
    rle_size: usize,
    decoded: u64,
    validate_checksum: bool,
    xxh: Xxh64,
    dict_id: u32,
    single_pass: bool,
    // Entropia.
    huf: Option<Huf>,
    lit_entropy: bool,
    fse_entropy: bool,
    ll: SeqTable,
    of: SeqTable,
    ml: SeqTable,
    rep: [u64; 3],
    default_ll: SeqTable,
    default_of: SeqTable,
    default_ml: SeqTable,
    // Histórico das cópias: dicionário mais a saída recente do frame.
    hist: Vec<u8>,
    hist_keep: usize,
    // Stream.
    sstage: StreamStage,
    header: Vec<u8>,
    in_buff: Vec<u8>,
    in_pos: usize,
    out_buff_size: u64,
    out_start: u64,
    out_end: u64,
    flush: Vec<u8>,
    flush_pos: usize,
    hostage: bool,
    no_progress: u32,
}

impl DStream {
    /// Um contexto com o limite de janela do `ZSTD_DCtx_setMaxWindowSize`.
    pub fn new(max_window: u64) -> DStream {
        let ofb: Vec<u32> = (0..=MAX_OFF).map(of_base).collect();
        let ofbits: Vec<u8> = (0..=MAX_OFF).map(of_bits).collect();
        DStream {
            max_window,
            ignore_checksum: false,
            dict: None,
            fp: FrameHeader::default(),
            stage: Stage::GetFrameHeaderSize,
            expected: FRAMEHEADERSIZE_PREFIX,
            btype: BT_RESERVED,
            rle_size: 0,
            decoded: 0,
            validate_checksum: false,
            xxh: Xxh64::new(0),
            dict_id: 0,
            single_pass: false,
            huf: None,
            lit_entropy: false,
            fse_entropy: false,
            ll: SeqTable::default(),
            of: SeqTable::default(),
            ml: SeqTable::default(),
            rep: REP_START,
            default_ll: seq_table(&LL_DEFAULT_NORM, 6, &LL_BASE, &LL_BITS),
            default_of: seq_table(&OF_DEFAULT_NORM, 5, &ofb, &ofbits),
            default_ml: seq_table(&ML_DEFAULT_NORM, 6, &ML_BASE, &ML_BITS),
            hist: Vec::new(),
            hist_keep: 0,
            sstage: StreamStage::Init,
            header: Vec::new(),
            in_buff: Vec::new(),
            in_pos: 0,
            out_buff_size: 0,
            out_start: 0,
            out_end: 0,
            flush: Vec::new(),
            flush_pos: 0,
            hostage: false,
            no_progress: 0,
        }
    }

    /// `ZSTD_d_forceIgnoreChecksum`.
    pub fn set_ignore_checksum(&mut self, on: bool) {
        self.ignore_checksum = on;
    }

    /// Referencia um dicionário pra todos os frames seguintes.
    pub fn set_dict(&mut self, dict: Option<Dict>) {
        self.dict = dict;
    }

    /// `ZSTD_DCtx_reset(ZSTD_reset_session_only)`.
    pub fn reset_session(&mut self) {
        self.sstage = StreamStage::Init;
        self.no_progress = 0;
    }

    /// `ZSTD_decompressBegin_usingDDict`.
    fn begin(&mut self) {
        self.expected = FRAMEHEADERSIZE_PREFIX;
        self.stage = Stage::GetFrameHeaderSize;
        self.decoded = 0;
        self.lit_entropy = false;
        self.fse_entropy = false;
        self.dict_id = 0;
        self.btype = BT_RESERVED;
        self.rep = REP_START;
        self.huf = None;
        self.ll = SeqTable::default();
        self.of = SeqTable::default();
        self.ml = SeqTable::default();
        self.hist.clear();
        if let Some(d) = &self.dict {
            self.hist.extend_from_slice(&d.content);
            if let Some(e) = &d.entropy {
                self.dict_id = d.id;
                self.huf = Some(e.huf.clone());
                self.ll = e.ll.clone();
                self.of = e.of.clone();
                self.ml = e.ml.clone();
                self.rep = e.rep;
                self.lit_entropy = true;
                self.fse_entropy = true;
            }
        }
    }

    /// `ZSTD_decodeFrameHeader`.
    fn decode_frame_header(&mut self, src: &[u8]) -> Res<()> {
        let mut fp = FrameHeader::default();
        if get_frame_header(&mut fp, src)? > 0 {
            return Err(Error::SrcSizeWrong);
        }
        self.fp = fp;
        if fp.dict_id != 0 && self.dict_id != fp.dict_id {
            return Err(Error::DictionaryWrong);
        }
        self.validate_checksum = fp.checksum && !self.ignore_checksum;
        if self.validate_checksum {
            self.xxh = Xxh64::new(0);
        }
        let dict_len = self.dict.as_ref().map_or(0, |d| d.content.len());
        self.hist_keep = (fp.window_size as usize).max(dict_len).saturating_add(2 * BLOCKSIZE_MAX);
        Ok(())
    }

    fn block_size_max(&self) -> usize {
        self.fp.block_size_max
    }

    fn trim_history(&mut self) {
        if self.hist.len() > self.hist_keep.saturating_mul(2) {
            let cut = self.hist.len() - self.hist_keep;
            self.hist.drain(..cut);
        }
    }

    /// `ZSTD_decodeLiteralsBlock`: devolve (bytes lidos, literais).
    fn decode_literals(&mut self, src: &[u8], cap: usize) -> Res<(usize, Vec<u8>)> {
        if src.len() < MIN_CBLOCK_SIZE {
            return Err(Error::Corruption);
        }
        let lit_type = src[0] & 3;
        let max = self.block_size_max();
        let expected_write = max.min(cap);
        let lhl = (src[0] >> 2) & 3;
        match lit_type {
            2 | 3 => {
                if lit_type == 3 && !self.lit_entropy {
                    return Err(Error::DictionaryCorrupted);
                }
                if src.len() < 5 {
                    return Err(Error::Corruption);
                }
                let lhc = le32(src) as usize;
                let (lh_size, lit_size, lit_csize, single) = match lhl {
                    0 | 1 => (3, (lhc >> 4) & 0x3FF, (lhc >> 14) & 0x3FF, lhl == 0),
                    2 => (4, (lhc >> 4) & 0x3FFF, lhc >> 18, false),
                    _ => (5, (lhc >> 4) & 0x3FFFF, (lhc >> 22) + ((src[4] as usize) << 10), false),
                };
                if lit_size > max {
                    return Err(Error::Corruption);
                }
                if !single && lit_size < MIN_LITERALS_FOR_4_STREAMS {
                    return Err(Error::LiteralsHeaderWrong);
                }
                if lit_csize + lh_size > src.len() {
                    return Err(Error::Corruption);
                }
                if expected_write < lit_size {
                    return Err(Error::DstSizeTooSmall);
                }
                let body = &src[lh_size..lh_size + lit_csize];
                let mut lits = Vec::with_capacity(lit_size);
                let result = if lit_type == 3 {
                    let Some(h) = &self.huf else { return Err(Error::DictionaryCorrupted) };
                    if single { huf_stream(h, body, lit_size, &mut lits) } else { huf_4streams(h, body, lit_size, &mut lits) }
                } else {
                    (|| {
                        if !single && (lit_size == 0 || body.is_empty()) {
                            return Err(Error::Corruption);
                        }
                        let (h, hsize) = read_huf(body)?;
                        if hsize >= body.len() {
                            return Err(Error::SrcSizeWrong);
                        }
                        let rest = &body[hsize..];
                        let r = if single {
                            huf_stream(&h, rest, lit_size, &mut lits)
                        } else {
                            huf_4streams(&h, rest, lit_size, &mut lits)
                        };
                        self.huf = Some(h);
                        r
                    })()
                };
                if result.is_err() {
                    return Err(Error::Corruption);
                }
                self.lit_entropy = true;
                Ok((lit_csize + lh_size, lits))
            }
            0 => {
                let (lh_size, lit_size) = match lhl {
                    0 | 2 => (1, (src[0] >> 3) as usize),
                    1 => (2, (le16(src) >> 4) as usize),
                    _ => {
                        if src.len() < 3 {
                            return Err(Error::Corruption);
                        }
                        (3, (le24(src) >> 4) as usize)
                    }
                };
                if lit_size > max {
                    return Err(Error::Corruption);
                }
                if expected_write < lit_size {
                    return Err(Error::DstSizeTooSmall);
                }
                if lh_size + lit_size > src.len() {
                    return Err(Error::Corruption);
                }
                Ok((lh_size + lit_size, src[lh_size..lh_size + lit_size].to_vec()))
            }
            _ => {
                let (lh_size, lit_size) = match lhl {
                    0 | 2 => (1, (src[0] >> 3) as usize),
                    1 => {
                        if src.len() < 3 {
                            return Err(Error::Corruption);
                        }
                        (2, (le16(src) >> 4) as usize)
                    }
                    _ => {
                        if src.len() < 4 {
                            return Err(Error::Corruption);
                        }
                        (3, (le24(src) >> 4) as usize)
                    }
                };
                if lit_size > max {
                    return Err(Error::Corruption);
                }
                if expected_write < lit_size {
                    return Err(Error::DstSizeTooSmall);
                }
                Ok((lh_size + 1, vec![src[lh_size]; lit_size]))
            }
        }
    }

    /// `ZSTD_buildSeqTable` pra um dos três campos.
    fn build_seq_table(&mut self, which: u8, mode: u8, src: &[u8]) -> Res<usize> {
        let (max, max_log): (usize, u32) = match which {
            0 => (MAX_LL, LL_FSE_LOG),
            1 => (MAX_OFF, OFF_FSE_LOG),
            _ => (MAX_ML, ML_FSE_LOG),
        };
        let ofb: Vec<u32>;
        let ofbits: Vec<u8>;
        let (base, bits): (&[u32], &[u8]) = match which {
            0 => (&LL_BASE, &LL_BITS),
            1 => {
                ofb = (0..=MAX_OFF).map(of_base).collect();
                ofbits = (0..=MAX_OFF).map(of_bits).collect();
                (&ofb, &ofbits)
            }
            _ => (&ML_BASE, &ML_BITS),
        };
        let (table, used) = match mode {
            1 => {
                let Some(&sym) = src.first() else { return Err(Error::SrcSizeWrong) };
                if sym as usize > max {
                    return Err(Error::Corruption);
                }
                (seq_table_rle(base[sym as usize], bits[sym as usize]), 1)
            }
            0 => {
                let t = match which {
                    0 => self.default_ll.clone(),
                    1 => self.default_of.clone(),
                    _ => self.default_ml.clone(),
                };
                (t, 0)
            }
            3 => {
                if !self.fse_entropy {
                    return Err(Error::Corruption);
                }
                return Ok(0);
            }
            _ => {
                let nc = read_ncount(src, max).map_err(|_| Error::Corruption)?;
                if nc.table_log > max_log {
                    return Err(Error::Corruption);
                }
                (seq_table(&nc.norm, nc.table_log, base, bits), nc.size)
            }
        };
        match which {
            0 => self.ll = table,
            1 => self.of = table,
            _ => self.ml = table,
        }
        Ok(used)
    }

    /// `ZSTD_decodeSeqHeaders`: devolve (bytes lidos, número de sequências).
    fn decode_seq_headers(&mut self, src: &[u8]) -> Res<(usize, usize)> {
        if src.is_empty() {
            return Err(Error::SrcSizeWrong);
        }
        let mut ip = 1;
        let mut nb_seq = src[0] as usize;
        if nb_seq > 0x7F {
            if nb_seq == 0xFF {
                if ip + 2 > src.len() {
                    return Err(Error::SrcSizeWrong);
                }
                nb_seq = le16(&src[ip..]) as usize + LONGNBSEQ;
                ip += 2;
            } else {
                if ip >= src.len() {
                    return Err(Error::SrcSizeWrong);
                }
                nb_seq = ((nb_seq - 0x80) << 8) + src[ip] as usize;
                ip += 1;
            }
        }
        if nb_seq == 0 {
            if ip != src.len() {
                return Err(Error::Corruption);
            }
            return Ok((ip, 0));
        }
        if ip + 1 > src.len() {
            return Err(Error::SrcSizeWrong);
        }
        let modes = src[ip];
        if modes & 3 != 0 {
            return Err(Error::Corruption);
        }
        ip += 1;
        for (which, mode) in [(0u8, modes >> 6), (1, (modes >> 4) & 3), (2, (modes >> 2) & 3)] {
            let used = self.build_seq_table(which, mode, &src[ip..]).map_err(|_| Error::Corruption)?;
            ip += used;
        }
        Ok((ip, nb_seq))
    }

    /// `ZSTD_decompressBlock_internal`: decodifica um bloco comprimido no fim do histórico.
    fn decompress_block(&mut self, src: &[u8], cap: usize) -> Res<usize> {
        if src.len() > self.block_size_max() {
            return Err(Error::SrcSizeWrong);
        }
        let (lit_used, lits) = self.decode_literals(src, cap)?;
        let src = &src[lit_used..];
        let (seq_used, nb_seq) = self.decode_seq_headers(src)?;
        let src = &src[seq_used..];
        if cap == 0 && nb_seq > 0 {
            return Err(Error::DstSizeTooSmall);
        }
        // Na passada única o C guarda os literais logo depois do bloco quando há folga, e aí a saída
        // do bloco fica limitada ao começo desse buffer.
        let limit = if self.single_pass
            && cap > self.block_size_max() + WILDCOPY_OVERLENGTH + lits.len() + WILDCOPY_OVERLENGTH
        {
            self.block_size_max() + WILDCOPY_OVERLENGTH
        } else {
            cap
        };
        self.trim_history();
        let start = self.hist.len();
        let r = self.exec_sequences(src, nb_seq, &lits, start.saturating_add(limit));
        if let Err(e) = r {
            self.hist.truncate(start);
            return Err(e);
        }
        Ok(self.hist.len() - start)
    }

    fn exec_sequences(&mut self, src: &[u8], nb_seq: usize, lits: &[u8], oend: usize) -> Res<()> {
        let mut lit_pos = 0usize;
        if nb_seq > 0 {
            self.fse_entropy = true;
            let mut rep = self.rep;
            let mut bits = BitIn::new(src).map_err(|_| Error::Corruption)?;
            let ll_t = std::mem::take(&mut self.ll);
            let of_t = std::mem::take(&mut self.of);
            let ml_t = std::mem::take(&mut self.ml);
            let result = (|| {
                let mut ll_s = bits.read(ll_t.log) as usize;
                let mut of_s = bits.read(of_t.log) as usize;
                let mut ml_s = bits.read(ml_t.log) as usize;
                for n in (1..=nb_seq).rev() {
                    let ll = ll_t.cells[ll_s];
                    let ml = ml_t.cells[ml_s];
                    let of = of_t.cells[of_s];
                    let offset: u64;
                    if of.add_bits > 1 {
                        offset = u64::from(of.base) + bits.read(u32::from(of.add_bits));
                        rep[2] = rep[1];
                        rep[1] = rep[0];
                        rep[0] = offset;
                    } else {
                        let ll0 = usize::from(ll.base == 0);
                        if of.add_bits == 0 {
                            offset = rep[ll0];
                            rep[1] = rep[1 - ll0];
                            rep[0] = offset;
                        } else {
                            let code = u64::from(of.base) + ll0 as u64 + bits.read(1);
                            let mut temp = if code == 3 { rep[0].wrapping_sub(1) } else { rep[code as usize] };
                            if temp == 0 {
                                temp = u64::MAX;
                            }
                            if code != 1 {
                                rep[2] = rep[1];
                            }
                            rep[1] = rep[0];
                            rep[0] = temp;
                            offset = temp;
                        }
                    }
                    let mut match_len = u64::from(ml.base);
                    if ml.add_bits > 0 {
                        match_len += bits.read(u32::from(ml.add_bits));
                    }
                    let mut lit_len = u64::from(ll.base);
                    if ll.add_bits > 0 {
                        lit_len += bits.read(u32::from(ll.add_bits));
                    }
                    if n > 1 {
                        ll_s = ll.next as usize + bits.read(u32::from(ll.nb_bits)) as usize;
                        ml_s = ml.next as usize + bits.read(u32::from(ml.nb_bits)) as usize;
                        of_s = of.next as usize + bits.read(u32::from(of.nb_bits)) as usize;
                    }
                    // `ZSTD_execSequence(End)`.
                    let op = self.hist.len();
                    if lit_len + match_len > (oend - op) as u64 {
                        return Err(Error::DstSizeTooSmall);
                    }
                    if lit_len > (lits.len() - lit_pos) as u64 {
                        return Err(Error::Corruption);
                    }
                    let lit_len = lit_len as usize;
                    self.hist.extend_from_slice(&lits[lit_pos..lit_pos + lit_len]);
                    lit_pos += lit_len;
                    let here = self.hist.len();
                    if offset > here as u64 {
                        return Err(Error::Corruption);
                    }
                    let from = here - offset as usize;
                    let match_len = match_len as usize;
                    if offset as usize >= match_len {
                        self.hist.extend_from_within(from..from + match_len);
                    } else {
                        for i in 0..match_len {
                            let b = self.hist[from + i];
                            self.hist.push(b);
                        }
                    }
                }
                if !bits.finished() {
                    return Err(Error::Corruption);
                }
                Ok(())
            })();
            self.ll = ll_t;
            self.of = of_t;
            self.ml = ml_t;
            result?;
            for (r, v) in self.rep.iter_mut().zip(rep) {
                *r = u64::from(v as u32);
            }
        }
        let last = lits.len() - lit_pos;
        if last > oend - self.hist.len() {
            return Err(Error::DstSizeTooSmall);
        }
        self.hist.extend_from_slice(&lits[lit_pos..]);
        Ok(())
    }

    /// Acrescenta um bloco cru ou RLE ao histórico; devolve quantos bytes entraram.
    fn put_block(&mut self, bytes: &[u8]) -> usize {
        self.trim_history();
        self.hist.extend_from_slice(bytes);
        bytes.len()
    }

    fn put_rle(&mut self, byte: u8, n: usize) -> usize {
        self.trim_history();
        let len = self.hist.len();
        self.hist.resize(len + n, byte);
        n
    }

    /// `ZSTD_decompressContinue` sobre o buffer de saída interno; devolve os bytes produzidos.
    fn decompress_continue(&mut self, cap: usize, src: &[u8]) -> Res<Vec<u8>> {
        match self.stage {
            Stage::GetFrameHeaderSize => Err(Error::Generic),
            Stage::DecodeBlockHeader => {
                let (csize, last, btype, orig) = block_size(src)?;
                if csize > self.block_size_max() {
                    return Err(Error::Corruption);
                }
                self.expected = csize;
                self.btype = btype;
                self.rle_size = orig;
                if csize > 0 {
                    self.stage = if last { Stage::DecompressLastBlock } else { Stage::DecompressBlock };
                    return Ok(Vec::new());
                }
                if last {
                    if self.fp.checksum {
                        self.expected = 4;
                        self.stage = Stage::CheckChecksum;
                    } else {
                        self.expected = 0;
                        self.stage = Stage::GetFrameHeaderSize;
                    }
                } else {
                    self.expected = BLOCK_HEADER_SIZE;
                    self.stage = Stage::DecodeBlockHeader;
                }
                Ok(Vec::new())
            }
            Stage::DecompressBlock | Stage::DecompressLastBlock => {
                let n = match self.btype {
                    BT_COMPRESSED => {
                        let n = self.decompress_block(src, cap)?;
                        self.expected = 0;
                        n
                    }
                    BT_RAW => {
                        if src.len() > cap {
                            return Err(Error::DstSizeTooSmall);
                        }
                        self.expected -= src.len();
                        self.put_block(src)
                    }
                    BT_RLE => {
                        if self.rle_size > cap {
                            return Err(Error::DstSizeTooSmall);
                        }
                        self.expected = 0;
                        self.put_rle(src[0], self.rle_size)
                    }
                    _ => return Err(Error::Corruption),
                };
                let produced = self.hist[self.hist.len() - n..].to_vec();
                if produced.len() > self.block_size_max() {
                    return Err(Error::Corruption);
                }
                self.decoded += produced.len() as u64;
                if self.validate_checksum {
                    self.xxh.update(&produced);
                }
                if self.expected > 0 {
                    return Ok(produced);
                }
                if self.stage == Stage::DecompressLastBlock {
                    if self.fp.content_size != CONTENTSIZE_UNKNOWN && self.decoded != self.fp.content_size {
                        return Err(Error::Corruption);
                    }
                    if self.fp.checksum {
                        self.expected = 4;
                        self.stage = Stage::CheckChecksum;
                    } else {
                        self.expected = 0;
                        self.stage = Stage::GetFrameHeaderSize;
                    }
                } else {
                    self.stage = Stage::DecodeBlockHeader;
                    self.expected = BLOCK_HEADER_SIZE;
                }
                Ok(produced)
            }
            Stage::CheckChecksum => {
                if self.validate_checksum && le32(src) != self.xxh.digest() as u32 {
                    return Err(Error::ChecksumWrong);
                }
                self.expected = 0;
                self.stage = Stage::GetFrameHeaderSize;
                Ok(Vec::new())
            }
            Stage::SkipFrame => {
                self.expected = 0;
                self.stage = Stage::GetFrameHeaderSize;
                Ok(Vec::new())
            }
        }
    }

    /// `ZSTD_nextSrcSizeToDecompressWithInputSize`.
    fn next_src_size_with_input(&self, input: usize) -> usize {
        if !matches!(self.stage, Stage::DecompressBlock | Stage::DecompressLastBlock) || self.btype != BT_RAW {
            return self.expected;
        }
        input.clamp(1, self.expected.max(1)).min(self.expected)
    }

    /// `ZSTD_decompressContinueStream`.
    fn continue_stream(&mut self, src: &[u8]) -> Res<()> {
        let skip = self.stage == Stage::SkipFrame;
        let cap = if skip { 0 } else { self.out_buff_size.saturating_sub(self.out_start) as usize };
        let produced = self.decompress_continue(cap, src)?;
        if produced.is_empty() && !skip {
            self.sstage = StreamStage::Read;
        } else {
            self.out_end = self.out_start + produced.len() as u64;
            self.flush = produced;
            self.flush_pos = 0;
            self.sstage = StreamStage::Flush;
        }
        Ok(())
    }

    /// Passada única sobre um frame inteiro (`ZSTD_decompress_usingDDict` sobre um só frame).
    fn decompress_single(&mut self, src: &[u8], cap: usize) -> Res<Vec<u8>> {
        self.begin();
        self.single_pass = true;
        let r = self.decompress_frame(src, cap);
        self.single_pass = false;
        r
    }

    fn decompress_frame(&mut self, src: &[u8], cap: usize) -> Res<Vec<u8>> {
        if src.len() < FRAMEHEADERSIZE_MIN + BLOCK_HEADER_SIZE {
            return Err(Error::SrcSizeWrong);
        }
        let fhsize = frame_header_size(src)?;
        if src.len() < fhsize + BLOCK_HEADER_SIZE {
            return Err(Error::SrcSizeWrong);
        }
        self.decode_frame_header(&src[..fhsize])?;
        let mut ip = fhsize;
        let mut out = Vec::new();
        loop {
            let (csize, last, btype, orig) = block_size(&src[ip..])?;
            ip += BLOCK_HEADER_SIZE;
            if csize > src.len() - ip {
                return Err(Error::SrcSizeWrong);
            }
            let room = cap - out.len();
            let n = match btype {
                BT_COMPRESSED => self.decompress_block(&src[ip..ip + csize], room)?,
                BT_RAW => {
                    if csize > room {
                        return Err(Error::DstSizeTooSmall);
                    }
                    self.put_block(&src[ip..ip + csize])
                }
                _ => {
                    if orig > room {
                        return Err(Error::DstSizeTooSmall);
                    }
                    self.put_rle(src[ip], orig)
                }
            };
            let produced = &self.hist[self.hist.len() - n..];
            if self.validate_checksum {
                self.xxh.update(produced);
            }
            out.extend_from_slice(produced);
            ip += csize;
            if last {
                break;
            }
        }
        if self.fp.content_size != CONTENTSIZE_UNKNOWN && out.len() as u64 != self.fp.content_size {
            return Err(Error::Corruption);
        }
        if self.fp.checksum {
            if src.len() - ip < 4 {
                return Err(Error::ChecksumWrong);
            }
            if !self.ignore_checksum && le32(&src[ip..]) != self.xxh.digest() as u32 {
                return Err(Error::ChecksumWrong);
            }
        }
        Ok(out)
    }

    /// `ZSTD_decompressStream`: consome de `input[*in_pos..]` e acrescenta em `out` até `out_cap`
    /// bytes no total. Devolve a dica de quantos bytes ler a seguir (0 no fim do frame).
    pub fn decompress_stream(
        &mut self,
        out: &mut Vec<u8>,
        out_cap: usize,
        input: &[u8],
        in_pos: &mut usize,
    ) -> Res<usize> {
        let istart = *in_pos;
        let iend = input.len();
        let mut ip = istart;
        let ostart = out.len();
        let mut more = true;
        while more {
            match self.sstage {
                StreamStage::Init | StreamStage::LoadHeader => {
                    if self.sstage == StreamStage::Init {
                        self.sstage = StreamStage::LoadHeader;
                        self.header.clear();
                        self.in_pos = 0;
                        self.out_start = 0;
                        self.out_end = 0;
                        self.hostage = false;
                    }
                    let mut fp = FrameHeader::default();
                    let hsize = get_frame_header(&mut fp, &self.header)?;
                    if hsize != 0 {
                        let to_load = hsize - self.header.len();
                        let remaining = iend - ip;
                        if to_load > remaining {
                            self.header.extend_from_slice(&input[ip..iend]);
                            *in_pos = iend;
                            let mut fp2 = FrameHeader::default();
                            get_frame_header(&mut fp2, &self.header)?;
                            return Ok(FRAMEHEADERSIZE_MIN.max(hsize) - self.header.len() + BLOCK_HEADER_SIZE);
                        }
                        self.header.extend_from_slice(&input[ip..ip + to_load]);
                        ip += to_load;
                        continue;
                    }
                    self.fp = fp;
                    let op = out.len();
                    if fp.content_size != CONTENTSIZE_UNKNOWN
                        && !fp.skippable
                        && (out_cap - op) as u64 >= fp.content_size
                        && let Ok(csize) = find_frame_compressed_size(&input[istart..iend])
                    {
                        let data = self.decompress_single(&input[istart..istart + csize], out_cap - op)?;
                        out.extend_from_slice(&data);
                        ip = istart + csize;
                        self.expected = 0;
                        self.stage = Stage::GetFrameHeaderSize;
                        self.sstage = StreamStage::Init;
                        break;
                    }
                    self.begin();
                    let header = std::mem::take(&mut self.header);
                    let magic = le32(&header);
                    if magic & MAGIC_SKIPPABLE_MASK == MAGIC_SKIPPABLE_START {
                        self.fp = fp;
                        self.expected = le32(&header[4..]) as usize;
                        self.stage = Stage::SkipFrame;
                    } else {
                        let r = self.decode_frame_header(&header);
                        self.header = header;
                        r?;
                        self.expected = BLOCK_HEADER_SIZE;
                        self.stage = Stage::DecodeBlockHeader;
                    }
                    self.fp.window_size = self.fp.window_size.max(1 << WINDOWLOG_ABSOLUTEMIN);
                    if self.fp.window_size > self.max_window {
                        return Err(Error::WindowTooLarge);
                    }
                    let block = self.fp.block_size_max.min(self.fp.window_size.min(BLOCKSIZE_MAX as u64) as usize);
                    let needed_rb = self.fp.window_size + 2 * block as u64 + 2 * WILDCOPY_OVERLENGTH as u64;
                    self.out_buff_size = self.fp.content_size.min(needed_rb);
                    self.sstage = StreamStage::Read;
                }
                StreamStage::Read => {
                    let needed = self.next_src_size_with_input(iend - ip);
                    if needed == 0 {
                        self.sstage = StreamStage::Init;
                        break;
                    }
                    if iend - ip >= needed {
                        self.continue_stream(&input[ip..ip + needed])?;
                        ip += needed;
                        continue;
                    }
                    if ip == iend {
                        break;
                    }
                    self.sstage = StreamStage::Load;
                }
                StreamStage::Load => {
                    let needed = self.expected;
                    let to_load = needed - self.in_pos;
                    let skip = self.stage == Stage::SkipFrame;
                    let loaded = to_load.min(iend - ip);
                    if !skip {
                        self.in_buff.truncate(self.in_pos);
                        self.in_buff.extend_from_slice(&input[ip..ip + loaded]);
                    }
                    ip += loaded;
                    self.in_pos += loaded;
                    if loaded < to_load {
                        more = false;
                        continue;
                    }
                    self.in_pos = 0;
                    let buf = if skip { Vec::new() } else { std::mem::take(&mut self.in_buff) };
                    let r = self.continue_stream(&buf);
                    self.in_buff = buf;
                    r?;
                }
                StreamStage::Flush => {
                    let pending = self.flush.len() - self.flush_pos;
                    let room = out_cap - out.len();
                    let n = pending.min(room);
                    out.extend_from_slice(&self.flush[self.flush_pos..self.flush_pos + n]);
                    self.flush_pos += n;
                    self.out_start += n as u64;
                    if n == pending {
                        self.sstage = StreamStage::Read;
                        if self.out_buff_size < self.fp.content_size
                            && self.out_start + self.fp.block_size_max as u64 > self.out_buff_size
                        {
                            self.out_start = 0;
                            self.out_end = 0;
                        }
                        continue;
                    }
                    more = false;
                }
            }
        }
        *in_pos = ip;
        if ip == istart && out.len() == ostart {
            self.no_progress += 1;
            if self.no_progress >= NO_FORWARD_PROGRESS_MAX {
                if out.len() == out_cap {
                    return Err(Error::NoProgressDestFull);
                }
                if ip == iend {
                    return Err(Error::NoProgressInputEmpty);
                }
            }
        } else {
            self.no_progress = 0;
        }
        let next = self.expected;
        if next == 0 {
            if self.out_end == self.out_start {
                if self.hostage {
                    if *in_pos >= iend {
                        self.sstage = StreamStage::Read;
                        return Ok(1);
                    }
                    *in_pos += 1;
                }
                return Ok(0);
            }
            if !self.hostage {
                *in_pos -= 1;
                self.hostage = true;
            }
            return Ok(1);
        }
        let block_hint = if self.stage == Stage::DecompressBlock { BLOCK_HEADER_SIZE } else { 0 };
        Ok(next + block_hint - self.in_pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xxh64_known_values() {
        assert_eq!(Xxh64::new(0).digest(), 0xEF46_DB37_51D8_E999);
        let mut h = Xxh64::new(0);
        h.update(b"abc");
        assert_eq!(h.digest(), 0x44BC_2CF5_AD77_0999);
        let data: Vec<u8> = (0..100u8).collect();
        let mut a = Xxh64::new(0);
        a.update(&data);
        let mut b = Xxh64::new(0);
        for c in data.chunks(7) {
            b.update(c);
        }
        assert_eq!(a.digest(), b.digest());
    }

    #[test]
    fn default_tables_have_expected_sizes() {
        let d = DStream::new(1 << 27);
        assert_eq!(d.default_ll.cells.len(), 64);
        assert_eq!(d.default_ml.cells.len(), 64);
        assert_eq!(d.default_of.cells.len(), 32);
    }

    fn encode(data: &[u8], level: i32) -> Vec<u8> {
        use std::io::Write;
        use structured_zstd::encoding::{CompressionLevel, StreamingEncoder};
        let mut e = StreamingEncoder::new(Vec::new(), CompressionLevel::from_level(level));
        e.set_content_checksum(true).unwrap();
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    /// Descomprime como o `fileio`: pedaços de entrada de `DSTREAM_IN_SIZE`, saída de `DSTREAM_OUT_SIZE`.
    fn decode(src: &[u8]) -> Res<Vec<u8>> {
        decode_with(src, None)
    }

    fn decode_with(src: &[u8], dict: Option<Dict>) -> Res<Vec<u8>> {
        let mut d = DStream::new(1 << 27);
        d.set_dict(dict);
        let mut out = Vec::new();
        let mut loaded: Vec<u8> = Vec::new();
        let mut chunks = src.chunks(DSTREAM_IN_SIZE);
        let mut pos = 0usize;
        loop {
            if pos == loaded.len() || loaded.len() - pos < 4 {
                match chunks.next() {
                    Some(c) => {
                        loaded.drain(..pos);
                        pos = 0;
                        loaded.extend_from_slice(c);
                    }
                    None if pos == loaded.len() => return Ok(out),
                    None => {}
                }
            }
            d.reset_session();
            loop {
                let mut buf = Vec::new();
                let hint = d.decompress_stream(&mut buf, DSTREAM_OUT_SIZE, &loaded, &mut pos)?;
                out.extend_from_slice(&buf);
                if hint == 0 {
                    break;
                }
                if loaded.len() - pos < hint.min(DSTREAM_IN_SIZE) {
                    match chunks.next() {
                        Some(c) => {
                            loaded.drain(..pos);
                            pos = 0;
                            loaded.extend_from_slice(c);
                        }
                        None => return Err(Error::SrcSizeWrong),
                    }
                }
            }
        }
    }

    fn sample(n: usize) -> Vec<u8> {
        let words = ["alpha ", "beta ", "gamma\n", "delta ", "epsilon ", "zeta\t", "eta ", "theta,"];
        let mut seed = 12345u32;
        let mut v = Vec::new();
        while v.len() < n {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            v.extend_from_slice(words[(seed >> 16) as usize % words.len()].as_bytes());
            if seed.is_multiple_of(7) {
                v.push((seed >> 8) as u8);
            }
        }
        v.truncate(n);
        v
    }

    #[test]
    fn roundtrip_with_structured_zstd_encoder() {
        for n in [0usize, 1, 5, 100, 4000, 70_000, 300_000] {
            let data = sample(n);
            for level in [1, 3, 9, 19] {
                let z = encode(&data, level);
                assert_eq!(decode(&z).unwrap(), data, "n={n} level={level}");
            }
        }
    }

    /// Vetores gerados pelo zstd 1.5.7 do oráculo: `ZSTD_VECTORS=<dir> cargo test -- --ignored`.
    /// `X.<variante>.zst` descomprime pra `X`, `multi.zst` pra `multi.orig`, e `*.dict.zst` usa `dict.bin`.
    #[test]
    #[ignore]
    fn vectors_from_reference_zstd() {
        let Ok(dir) = std::env::var("ZSTD_VECTORS") else { return };
        let dir = std::path::Path::new(&dir);
        let dict = Dict::parse(&std::fs::read(dir.join("dict.bin")).unwrap()).unwrap();
        let mut n = 0;
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let Some(stem) = name.strip_suffix(".zst") else { continue };
            let orig = if stem == "multi" {
                "multi.orig".to_string()
            } else {
                stem.rsplit_once('.').unwrap().0.to_string()
            };
            let want = std::fs::read(dir.join(&orig)).unwrap();
            let z = std::fs::read(&p).unwrap();
            let d = if stem.ends_with(".dict") { Some(dict.clone()) } else { None };
            assert_eq!(decode_with(&z, d).map(|v| v == want), Ok(true), "{name}");
            n += 1;
        }
        assert!(n > 50, "{n}");
    }

    #[test]
    fn corrupted_checksum_is_reported() {
        let data = sample(1000);
        let mut z = encode(&data, 3);
        let last = z.len() - 1;
        z[last] ^= 1;
        assert_eq!(decode(&z), Err(Error::ChecksumWrong));
    }
}
