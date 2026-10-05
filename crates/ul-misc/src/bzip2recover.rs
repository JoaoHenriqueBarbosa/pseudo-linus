//! `bzip2recover` do bzip2 1.0.8: procura as fronteiras de bloco num `.bz2` danificado (padrão de
//! 48 bits em qualquer alinhamento de bit) e grava cada bloco íntegro num arquivo próprio
//! `recNNNNNnome.bz2`, no diretório corrente.
//!
//! Comportamento do original, reproduzido:
//!
//! - O banner sai no stderr antes de qualquer verificação. Sem exatamente um argumento, mostra o
//!   uso e sai com 1.
//! - Um bloco só é aceito se tiver pelo menos 131 bits entre as fronteiras; o último trecho, sem
//!   marca de fim, sai como `(incomplete)` quando tem 40 bits ou mais e não é gravado.
//! - Cada arquivo de saída leva `BZh` e o nível do original, a marca de bloco, o bloco copiado
//!   bit a bit, a marca de fim de fluxo e o CRC do próprio bloco como CRC combinado.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, OFlags};

use crate::util::io;

const MAX_BLOCKS: usize = 50000;
const HEADER_HI: u32 = 0x0000_3141;
const HEADER_LO: u32 = 0x5926_5359;
const ENDMARK_HI: u32 = 0x0000_1772;
const ENDMARK_LO: u32 = 0x4538_5090;

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// Escritor de bits, do mais significativo para o menos.
struct BitWriter {
    out: Vec<u8>,
    cur: u8,
    n: u8,
}

impl BitWriter {
    fn new() -> Self {
        BitWriter {
            out: Vec::new(),
            cur: 0,
            n: 0,
        }
    }

    fn bit(&mut self, b: u8) {
        self.cur = (self.cur << 1) | (b & 1);
        self.n += 1;
        if self.n == 8 {
            self.out.push(self.cur);
            self.cur = 0;
            self.n = 0;
        }
    }

    fn byte(&mut self, v: u8) {
        for i in (0..8).rev() {
            self.bit(v >> i);
        }
    }

    fn u32(&mut self, v: u32) {
        for i in (0..4).rev() {
            self.byte((v >> (i * 8)) as u8);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        while self.n != 0 {
            self.bit(0);
        }
        self.out
    }
}

fn bit_at(data: &[u8], pos: u64) -> u8 {
    (data[(pos / 8) as usize] >> (7 - (pos % 8))) & 1
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let prog = io::argv0(args);
    io::eprint("bzip2recover 1.0.8: extracts blocks from damaged .bz2 files.\n");
    if argv.len() != 2 {
        io::eprint(format!(
            "{prog}: usage is `{prog} damaged_file_name'.\n\trestrictions on size of recovered file: None\n"
        ));
        return 1;
    }
    let in_name = &argv[1];
    if in_name.len() >= 2000 - 20 {
        io::eprint(format!(
            "{prog}: supplied filename is suspiciously (>= {} chars) long.  Bye!\n",
            in_name.len()
        ));
        return 1;
    }
    let Ok(mut in_file) = io::File::open(in_name) else {
        io::eprint(format!("{prog}: can't read `{}'\n", io::lossy(in_name)));
        return 1;
    };
    io::eprint(format!("{prog}: searching for block boundaries ...\n"));
    let data = match in_file.read_to_end_sys() {
        Ok(d) => d,
        Err(e) => {
            io::eprint(format!(
                "{prog}: I/O error reading `{}', possible reason follows.\n{prog}: {}\n{prog}: warning: output file(s) may be incomplete.\n",
                io::lossy(in_name),
                e.message()
            ));
            return 1;
        }
    };

    let nbits = data.len() as u64 * 8;
    let mut bits_read: u64 = 0;
    let (mut hi, mut lo) = (0u32, 0u32);
    let mut starts: Vec<u64> = vec![0];
    let mut ends: Vec<u64> = vec![0];
    let mut cur: usize = 0;
    let mut found: Vec<(u64, u64)> = Vec::new();
    loop {
        let eof = bits_read >= nbits;
        let b = if eof { 2 } else { bit_at(&data, bits_read) };
        bits_read += 1;
        if eof {
            if bits_read >= starts[cur] && bits_read - starts[cur] >= 40 {
                ends[cur] = bits_read - 1;
                if cur > 0 {
                    io::eprint(format!(
                        "   block {} runs from {} to {} (incomplete)\n",
                        cur, starts[cur], ends[cur]
                    ));
                }
            }
            break;
        }
        hi = (hi << 1) | (lo >> 31);
        lo = (lo << 1) | u32::from(b & 1);
        let header = (hi & 0xffff) == HEADER_HI && lo == HEADER_LO;
        let endmark = (hi & 0xffff) == ENDMARK_HI && lo == ENDMARK_LO;
        if header || endmark {
            ends[cur] = if bits_read > 49 { bits_read - 49 } else { 0 };
            if cur > 0 && ends[cur].wrapping_sub(starts[cur]) >= 130 {
                io::eprint(format!(
                    "   block {} runs from {} to {}\n",
                    found.len() + 1,
                    starts[cur],
                    ends[cur]
                ));
                found.push((starts[cur], ends[cur]));
            }
            if cur >= MAX_BLOCKS {
                io::eprint(format!(
                    "{prog}: `{}' appears to contain more than {MAX_BLOCKS} blocks\n{prog}: and cannot be handled.  To fix, increase\n{prog}: BZ_MAX_HANDLED_BLOCKS in bzip2recover.c, and recompile.\n",
                    io::lossy(in_name)
                ));
                return 1;
            }
            cur += 1;
            starts.push(bits_read);
            ends.push(0);
        }
    }

    if found.is_empty() {
        io::eprint(format!("{prog}: sorry, I couldn't find any block boundaries.\n"));
        return 1;
    }
    io::eprint(format!("{prog}: splitting into blocks\n"));

    // O original sempre grava o nível 9 no cabeçalho, qualquer que seja o do arquivo de entrada.
    let level = b'9';
    let (dir, base): (&[u8], &[u8]) = match in_name.iter().rposition(|&c| c == b'/') {
        Some(i) => (&in_name[..=i], &in_name[i + 1..]),
        None => (&[], in_name),
    };
    let stem: &[u8] = base.strip_suffix(b".bz2").unwrap_or(base);
    for (i, &(s, e)) in found.iter().enumerate() {
        let mut name = dir.to_vec();
        name.extend_from_slice(format!("rec{:05}", i + 1).as_bytes());
        name.extend_from_slice(stem);
        name.extend_from_slice(b".bz2");
        let shown = io::lossy(&name);
        io::eprint(format!("   writing block {} to `{shown}' ...\n", i + 1));

        let mut w = BitWriter::new();
        for c in [b'B', b'Z', b'h', level] {
            w.byte(c);
        }
        for c in [0x31, 0x41, 0x59, 0x26, 0x53, 0x59] {
            w.byte(c);
        }
        let mut crc: u32 = 0;
        for p in s..=e {
            if p >= nbits {
                break;
            }
            let b = bit_at(&data, p);
            if p - s < 32 {
                crc = (crc << 1) | u32::from(b);
            }
            w.bit(b);
        }
        for c in [0x17, 0x72, 0x45, 0x38, 0x50, 0x90] {
            w.byte(c);
        }
        w.u32(crc);
        let bytes = w.finish();

        let file = io::File::open_with(
            &name,
            OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC,
            0o600,
        );
        let ok = match file {
            Ok(mut f) => f.write_all(&bytes).is_ok(),
            Err(_) => false,
        };
        if !ok {
            io::eprint(format!("{prog}: can't write `{shown}'\n"));
            return 1;
        }
    }
    io::eprint(format!("{prog}: finished\n"));
    0
}
