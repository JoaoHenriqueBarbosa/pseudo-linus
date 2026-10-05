//! `pzstd` 1.5.7 do Debian 13 (`contrib/pzstd`): zstd paralelo.
//!
//! O formato do `pzstd` não é o do `zstd`: cada pedaço da entrada (`4 << windowLog` bytes) vira um
//! skippable frame de 12 bytes (magic `0x184D2A50`, tamanho 4, tamanho do frame seguinte) seguido de um
//! frame zstd em modo streaming, sem tamanho de conteúdo no cabeçalho e com checksum. A descompressão
//! lê esse formato com o decodificador próprio, que já ignora os skippable frames. O paralelismo não
//! muda o resultado, então o `-p` é validado e descartado.
//!
//! O codificador do `structured-zstd` gera frames válidos, mas não idênticos aos do zstd 1.5.7. Aqui o
//! cabeçalho do frame é regravado no formato do C (janela do nível, sem tamanho de conteúdo) e um frame
//! de bloco único que o C deixaria cru também volta a ser cru.

use std::io::Write;

use structured_zstd::encoding::{CompressionLevel, StreamingEncoder};
use sysabi::Fd;
use sysabi::OFlags;

use super::{common, zstd_dec};

const DEFAULT_LEVEL: i32 = 3;
const MAX_LEVEL: i32 = 19;
const ULTRA_MAX_LEVEL: i32 = 22;
const DEFAULT_VERBOSITY: i32 = 2;
const SKIPPABLE_MAGIC: u32 = 0x184D_2A50;
const BLOCK_MAX: usize = 1 << 17;
/// Nível de log das mensagens de erro e das de progresso (`kLogError` e `kLogInfo`).
const LOG_ERROR: i32 = 1;
const LOG_INFO: i32 = 2;

fn eprint(text: &str) {
    let _ = sysabi::sys::write_all(Fd::STDERR, text.as_bytes());
}

fn help() -> String {
    let mut s = String::new();
    s.push_str("Usage:\n");
    s.push_str("  pzstd [args] [FILE(s)]\n");
    s.push_str("Parallel ZSTD options:\n");
    s.push_str("  -p, --processes   #    : number of threads to use for (de)compression (default:<numcpus>)\n");
    s.push_str("ZSTD options:\n");
    s.push_str(&format!("  -#                     : # compression level (1-{MAX_LEVEL}, default:<numcpus>)\n"));
    s.push_str("  -d, --decompress       : decompression\n");
    s.push_str("  -o                file : result stored into `file` (only if 1 input file)\n");
    s.push_str("  -f, --force            : overwrite output without prompting, (de)compress links\n");
    s.push_str("      --rm               : remove source file(s) after successful (de)compression\n");
    s.push_str("  -k, --keep             : preserve source file(s) (default)\n");
    s.push_str("  -h, --help             : display help and exit\n");
    s.push_str("  -V, --version          : display version number and exit\n");
    s.push_str(&format!(
        "  -v, --verbose          : verbose mode; specify multiple times to increase log level (default:{DEFAULT_VERBOSITY})\n"
    ));
    s.push_str("  -q, --quiet            : suppress warnings; specify twice to suppress errors too\n");
    s.push_str("  -c, --stdout           : write to standard output (even if it is the console)\n");
    s.push_str("  -r                     : operate recursively on directories\n");
    s.push_str(&format!(
        "      --ultra            : enable levels beyond {MAX_LEVEL}, up to {ULTRA_MAX_LEVEL} (requires more memory)\n"
    ));
    s.push_str("  -C, --check            : integrity check (default)\n");
    s.push_str("      --no-check         : no integrity check\n");
    s.push_str("  -t, --test             : test compressed file integrity\n");
    s.push_str("  --                     : all arguments after \"--\" are treated as files\n");
    s
}

/// Argumento inválido: só a linha, sem a ajuda (como o `Options::parse` do 1.5.7).
fn invalid(arg: &[u8]) -> i32 {
    eprint(&format!("Invalid argument: {}\n", String::from_utf8_lossy(arg)));
    1
}

/// `-p` e `--processes`: precisa ser um inteiro positivo.
fn check_threads(value: &[u8]) -> bool {
    std::str::from_utf8(value).ok().and_then(|s| s.parse::<u32>().ok()).is_some_and(|n| n > 0)
}

/// As opções já interpretadas.
struct Settings {
    level: i32,
    decompress: bool,
    force: bool,
    remove: bool,
    to_stdout: bool,
    output: Option<Vec<u8>>,
    verbosity: i32,
    test: bool,
    checksum: bool,
    ultra: bool,
    files: Vec<Vec<u8>>,
}

impl Settings {
    fn log(&self, level: i32, text: &str) {
        if self.verbosity >= level {
            eprint(text);
        }
    }

    /// A linha de progresso: limpeza da linha e depois a mensagem.
    fn info(&self, text: &str) {
        if self.verbosity >= LOG_INFO {
            eprint(&format!("\r{:79}\r{text}", ""));
        }
    }
}

/// Resultado da leitura da linha de comando.
enum Parsed {
    Run(Settings),
    Exit(i32),
}

fn parse(argv: &[Vec<u8>]) -> Parsed {
    let mut s = Settings {
        level: DEFAULT_LEVEL,
        decompress: false,
        force: false,
        remove: false,
        to_stdout: false,
        output: None,
        verbosity: DEFAULT_VERBOSITY,
        test: false,
        checksum: true,
        ultra: false,
        files: Vec::new(),
    };
    let mut only_files = false;
    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        i += 1;
        if only_files || arg == b"-" || !arg.starts_with(b"-") {
            s.files.push(arg.clone());
            continue;
        }
        if arg == b"--" {
            only_files = true;
            continue;
        }
        if let Some(long) = arg.strip_prefix(b"--") {
            let (name, value) = match long.iter().position(|b| *b == b'=') {
                Some(p) => (&long[..p], Some(long[p + 1..].to_vec())),
                None => (long, None),
            };
            match name {
                b"processes" => {
                    let v = match value {
                        Some(v) => v,
                        None => {
                            let Some(v) = argv.get(i).cloned() else { return Parsed::Exit(invalid(arg)) };
                            i += 1;
                            v
                        }
                    };
                    if !check_threads(&v) {
                        return Parsed::Exit(invalid(arg));
                    }
                }
                b"decompress" => s.decompress = true,
                b"force" => s.force = true,
                b"rm" => s.remove = true,
                b"keep" => s.remove = false,
                b"stdout" => s.to_stdout = true,
                b"test" => {
                    s.decompress = true;
                    s.test = true;
                }
                b"ultra" => s.ultra = true,
                b"check" => s.checksum = true,
                b"no-check" => s.checksum = false,
                b"verbose" => s.verbosity += 1,
                b"quiet" => s.verbosity -= 1,
                b"help" => {
                    eprint(&help());
                    return Parsed::Exit(0);
                }
                b"version" => {
                    eprint("PZSTD version: 1.5.7.\n");
                    return Parsed::Exit(0);
                }
                _ => return Parsed::Exit(invalid(arg)),
            }
            continue;
        }
        // Opções curtas agrupadas.
        let shorts = &arg[1..];
        let mut j = 0;
        while j < shorts.len() {
            let c = shorts[j];
            j += 1;
            match c {
                b'0'..=b'9' => {
                    let mut level = i32::from(c - b'0');
                    while j < shorts.len() && shorts[j].is_ascii_digit() {
                        level = level.saturating_mul(10).saturating_add(i32::from(shorts[j] - b'0'));
                        j += 1;
                    }
                    s.level = level;
                }
                b'd' => s.decompress = true,
                b'f' => s.force = true,
                b'k' => s.remove = false,
                b'c' => s.to_stdout = true,
                b't' => {
                    s.decompress = true;
                    s.test = true;
                }
                b'r' => {}
                b'C' => s.checksum = true,
                b'v' => s.verbosity += 1,
                b'q' => s.verbosity -= 1,
                b'h' => {
                    eprint(&help());
                    return Parsed::Exit(0);
                }
                b'V' => {
                    eprint("PZSTD version: 1.5.7.\n");
                    return Parsed::Exit(0);
                }
                b'p' | b'o' => {
                    // O valor é o resto do grupo ou o próximo argumento.
                    let value = if j < shorts.len() {
                        let v = shorts[j..].to_vec();
                        j = shorts.len();
                        v
                    } else {
                        let Some(v) = argv.get(i).cloned() else { return Parsed::Exit(invalid(arg)) };
                        i += 1;
                        v
                    };
                    if c == b'p' {
                        if !check_threads(&value) {
                            return Parsed::Exit(invalid(arg));
                        }
                    } else {
                        s.output = Some(value);
                    }
                }
                _ => return Parsed::Exit(invalid(arg)),
            }
        }
    }
    let max = if s.ultra { ULTRA_MAX_LEVEL } else { MAX_LEVEL };
    if s.level == 0 {
        s.level = DEFAULT_LEVEL;
    }
    s.level = s.level.clamp(1, max);
    if s.output.is_some() && s.files.len() > 1 {
        return Parsed::Exit(invalid(b"-o requires exactly one input file"));
    }
    Parsed::Run(s)
}

/// `windowLog` do nível pra tamanho desconhecido (primeira tabela do `ZSTD_defaultCParameters`).
fn window_log(level: i32) -> u32 {
    match level {
        ..=1 => 19,
        2 => 20,
        3..=8 => 21,
        9..=16 => 22,
        17..=19 => 23,
        20 => 25,
        21 => 26,
        _ => 27,
    }
}

/// Grava no destino (`None` é o modo de teste); `false` em erro de escrita.
fn put(fd: Option<Fd>, bytes: &[u8]) -> bool {
    match fd {
        Some(fd) if !bytes.is_empty() => sysabi::sys::write_all(fd, bytes).is_ok(),
        _ => true,
    }
}

/// Troca o cabeçalho do frame do crate pelo do C (janela do nível, sem tamanho de conteúdo) e devolve a
/// um bloco cru o frame de bloco único que o C não deixaria comprimido.
fn rewrite_frame(frame: &[u8], chunk: &[u8], wlog: u32, checksum: bool) -> Vec<u8> {
    let mut fh = zstd_dec::FrameHeader::default();
    if zstd_dec::get_frame_header(&mut fh, frame) != Ok(0) {
        return frame.to_vec();
    }
    let old_header = fh.header_size as usize;
    if old_header > frame.len() {
        return frame.to_vec();
    }
    let window = 1u64 << wlog;
    let replace = fh.content_size == zstd_dec::CONTENTSIZE_UNKNOWN
        && fh.dict_id == 0
        && fh.window_size <= window
        && (10..=30).contains(&wlog);
    let mut body = frame[old_header..].to_vec();
    // Frame de bloco único: o C só mantém o bloco comprimido quando ele ganha o `ZSTD_minGain`.
    let tail = if checksum { 4 } else { 0 };
    if body.len() >= 3 + tail {
        let h = u32::from(body[0]) | u32::from(body[1]) << 8 | u32::from(body[2]) << 16;
        let last = h & 1 == 1;
        let block_type = (h >> 1) & 3;
        let size = (h >> 3) as usize;
        if last && block_type == 2 && 3 + size + tail == body.len() && chunk.len() <= BLOCK_MAX {
            let min_gain = (chunk.len() >> 6) + 2;
            if size + min_gain >= chunk.len() {
                let raw = (((chunk.len() as u32) << 3) | 1).to_le_bytes();
                let checksum_bytes = body[body.len() - tail..].to_vec();
                body.clear();
                body.extend_from_slice(&raw[..3]);
                body.extend_from_slice(chunk);
                body.extend_from_slice(&checksum_bytes);
            }
        }
    }
    let mut out = Vec::with_capacity(body.len() + 6);
    if replace {
        out.extend_from_slice(&zstd_dec::MAGIC.to_le_bytes());
        out.push(u8::from(checksum) << 2);
        out.push(((wlog - 10) << 3) as u8);
    } else {
        out.extend_from_slice(&frame[..old_header]);
    }
    out.extend_from_slice(&body);
    out
}

/// Um frame zstd em modo streaming (sem tamanho de conteúdo) pra um pedaço.
fn encode_chunk(chunk: &[u8], level: i32, checksum: bool, wlog: u32) -> Option<Vec<u8>> {
    let mut enc = StreamingEncoder::new(Vec::new(), CompressionLevel::from_level(level));
    enc.set_content_checksum(checksum).ok()?;
    enc.set_content_size_flag(false).ok()?;
    enc.write_all(chunk).ok()?;
    let frame = enc.finish().ok()?;
    Some(rewrite_frame(&frame, chunk, wlog, checksum))
}

/// Comprime a entrada pedaço a pedaço; devolve os bytes gravados.
fn compress(data: &[u8], s: &Settings, out: Option<Fd>) -> Result<u64, &'static str> {
    let wlog = window_log(s.level);
    let chunk_size = 1usize << (wlog + 2);
    let mut chunks: Vec<&[u8]> = data.chunks(chunk_size).collect();
    if chunks.is_empty() {
        chunks.push(&[]);
    }
    let mut written = 0u64;
    for chunk in chunks {
        let Some(frame) = encode_chunk(chunk, s.level, s.checksum, wlog) else {
            return Err("Compression error");
        };
        let mut buf = Vec::with_capacity(12 + frame.len());
        buf.extend_from_slice(&SKIPPABLE_MAGIC.to_le_bytes());
        buf.extend_from_slice(&4u32.to_le_bytes());
        buf.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        buf.extend_from_slice(&frame);
        if !put(out, &buf) {
            return Err("Failed to write output");
        }
        written += buf.len() as u64;
    }
    Ok(written)
}

/// Decodifica todos os frames da entrada (os skippable são ignorados pelo decodificador).
fn decompress(data: &[u8], out: Option<Fd>) -> Result<u64, String> {
    let mut dstream = zstd_dec::DStream::new(1u64 << 27);
    let mut off = 0usize;
    let mut total = 0u64;
    while off < data.len() {
        dstream.reset_session();
        loop {
            let mut buf = Vec::new();
            let mut pos = 0usize;
            let hint = dstream
                .decompress_stream(&mut buf, zstd_dec::DSTREAM_OUT_SIZE, &data[off..], &mut pos)
                .map_err(|e| e.name().to_string())?;
            off += pos;
            if !put(out, &buf) {
                return Err("Failed to write output".to_string());
            }
            total += buf.len() as u64;
            if hint == 0 {
                break;
            }
            if (pos == 0 && buf.is_empty()) || off >= data.len() {
                return Err("Unexpected end of input".to_string());
            }
        }
    }
    Ok(total)
}

/// Para onde vai a saída de uma entrada.
enum Target {
    Stdout,
    File(Vec<u8>),
    Discard,
}

fn target_for(s: &Settings, input: &[u8]) -> Result<Target, String> {
    if s.test {
        return Ok(Target::Discard);
    }
    if let Some(name) = &s.output {
        if name == b"-" {
            return Ok(Target::Stdout);
        }
        return Ok(Target::File(name.clone()));
    }
    if s.to_stdout || input == b"-" {
        return Ok(Target::Stdout);
    }
    if s.decompress {
        return match input.strip_suffix(b".zst") {
            Some(base) if !base.is_empty() => Ok(Target::File(base.to_vec())),
            _ => Err(format!("Invalid argument: {} does not end in .zst", String::from_utf8_lossy(input))),
        };
    }
    let mut name = input.to_vec();
    name.extend_from_slice(b".zst");
    Ok(Target::File(name))
}

/// Processa uma entrada; `true` quando deu certo.
fn run_one(s: &Settings, input: &[u8]) -> bool {
    let shown = String::from_utf8_lossy(input).into_owned();
    let fail = |what: &str| {
        s.log(LOG_ERROR, &format!("pzstd: {shown}: {what}.\n"));
        false
    };
    let from_stdin = input == b"-";
    let in_fd = if from_stdin {
        Fd::STDIN
    } else {
        match common::open_input(input, false) {
            Ok(fd) => fd,
            Err(_) => return fail("Failed to open input file"),
        }
    };
    let data = common::read_all(in_fd);
    if !from_stdin {
        common::close(in_fd);
    }
    let Ok(data) = data else { return fail("Failed to open input file") };

    let target = match target_for(s, input) {
        Ok(t) => t,
        Err(msg) => {
            s.log(LOG_ERROR, &format!("{msg}\n"));
            return false;
        }
    };
    let (out_fd, out_name, out_path) = match &target {
        Target::Discard => (None, String::new(), None),
        Target::Stdout => (Some(Fd::STDOUT), "stdout".to_string(), None),
        Target::File(path) => {
            if !s.force && common::stat(path).is_ok() {
                return fail("Failed to open output file");
            }
            let flags = OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC;
            match sysabi::sys::open(path, flags, 0o666) {
                Ok(fd) => (Some(fd), String::from_utf8_lossy(path).into_owned(), Some(path.clone())),
                Err(_) => return fail("Failed to open output file"),
            }
        }
    };

    let result = if s.decompress {
        decompress(&data, out_fd)
    } else {
        compress(&data, s, out_fd).map_err(str::to_string)
    };
    if let (Some(fd), Some(_)) = (out_fd, &out_path) {
        common::close(fd);
    }
    let written = match result {
        Ok(n) => n,
        Err(e) => {
            if let Some(path) = &out_path {
                let _ = common::unlink(path);
            }
            return fail(&e);
        }
    };

    if s.decompress {
        if !from_stdin {
            s.info(&format!("{shown:<20}: {written} bytes \n"));
        }
    } else {
        let ratio = written as f64 / data.len() as f64;
        s.info(&format!(
            "{shown:<20} :{:6.2}%   ({:6} => {:6} bytes, {out_name})\n",
            ratio * 100.0,
            data.len(),
            written
        ));
    }
    if s.remove && !from_stdin && !s.to_stdout && !s.test && out_path.is_some() {
        let _ = common::unlink(input);
    }
    true
}

pub fn main(argv: &[Vec<u8>]) -> i32 {
    let mut s = match parse(argv) {
        Parsed::Run(s) => s,
        Parsed::Exit(code) => return code,
    };
    if s.files.is_empty() {
        s.files.push(b"-".to_vec());
    }
    let mut code = 0;
    for input in s.files.clone() {
        if !run_one(&s, &input) {
            code = 1;
        }
    }
    code
}
