//! Compressão do arquivo: o GNU tar roda o compressor como processo filho (`gzip -d`, `bzip2 -d`...);
//! aqui os formatos conhecidos rodam em processo pelos codecs do crate, reproduzindo o que o usuário
//! vê do filho: a mensagem de erro do compressor e o "Child returned status N" do tar. `-I PROG`,
//! `-Z` e `--lzop` rodam o programa do sandbox por `/bin/sh -c`, como o GNU.

use sysabi::{Errno, Fd, FdWriter, OFlags};

use crate::codec::{self, DecodeError, Trailing};

use super::args::Compression;
use super::reader::{Reader, Source};
use super::writer::{Sink, Writer};
use super::{Fatal, R, Tar, quote};

/// Estado do "processo filho" de descompressão: o status com que ele terminaria.
pub struct Child {
    pub status: i32,
}

/// Formato do codec pra uma opção de compressão (os que o crate faz em processo).
fn codec_format(c: &Compression) -> Option<codec::Format> {
    Some(match c {
        Compression::Gzip => codec::Format::Gzip,
        Compression::Bzip2 => codec::Format::Bzip2,
        Compression::Xz => codec::Format::Xz,
        // O tar do Debian grava xz no `--lzma` (o programa configurado é o xz).
        Compression::Lzma => codec::Format::Xz,
        Compression::Lzip => codec::Format::Lzip,
        Compression::Zstd => codec::Format::Zstd,
        _ => return None,
    })
}

/// Opção que o tar sugere no "Archive is compressed. Use %s option".
fn option_for(f: codec::Format) -> &'static str {
    match f {
        codec::Format::Gzip => "-z",
        codec::Format::Bzip2 => "-j",
        codec::Format::Xz => "-J",
        codec::Format::Lzma => "--lzma",
        codec::Format::Lzip => "--lzip",
        codec::Format::Zstd => "--zstd",
    }
}

/// Programa externo de uma opção de compressão.
fn program_of(c: &Compression) -> Option<Vec<u8>> {
    match c {
        Compression::Compress => Some(b"compress".to_vec()),
        Compression::Lzop => Some(b"lzop".to_vec()),
        Compression::Program(p) => Some(p.clone()),
        _ => None,
    }
}

/// O que o descompressor escreve no stderr quando lê da entrada padrão, e o status de saída dele.
pub fn child_message(fmt: codec::Format, err: Option<&DecodeError>, trailing: Trailing) -> (Vec<u8>, i32) {
    const BZ_TAIL: &str = "\tInput file = (stdin), output file = (stdout)\n\nIt is possible that the compressed file(s) have become corrupted.\nYou can use the -tvv option to test integrity of such files.\n\nYou can use the `bzip2recover' program to attempt to recover\ndata from undamaged sections of corrupted files.\n\n";
    match (fmt, err) {
        (codec::Format::Gzip, Some(e)) => {
            let what = match e {
                DecodeError::NotFormat => "not in gzip format",
                DecodeError::Truncated => "unexpected end of file",
                DecodeError::Checksum => "invalid compressed data--crc error",
                DecodeError::Length => "invalid compressed data--length error",
                _ => "invalid compressed data--format violated",
            };
            (format!("\ngzip: stdin: {what}\n").into_bytes(), 1)
        }
        (codec::Format::Gzip, None) => match trailing {
            Trailing::Zeros => (b"\ngzip: stdin: decompression OK, trailing zero bytes ignored\n".to_vec(), 2),
            Trailing::Garbage(_) => (b"\ngzip: stdin: decompression OK, trailing garbage ignored\n".to_vec(), 2),
            Trailing::None => (Vec::new(), 0),
        },
        (codec::Format::Bzip2, Some(DecodeError::NotFormat)) => {
            (b"bzip2: (stdin) is not a bzip2 file.\n".to_vec(), 2)
        }
        (codec::Format::Bzip2, Some(DecodeError::Truncated)) => (
            format!(
                "\nbzip2: Compressed file ends unexpectedly;\n\tperhaps it is corrupted?  *Possible* reason follows.\nbzip2: Inappropriate ioctl for device\n{BZ_TAIL}"
            )
            .into_bytes(),
            2,
        ),
        (codec::Format::Bzip2, Some(_)) => {
            (format!("\nbzip2: Data integrity error when decompressing.\n{BZ_TAIL}").into_bytes(), 2)
        }
        (codec::Format::Bzip2, None) => match trailing {
            Trailing::Garbage(_) | Trailing::Zeros => (b"bzip2: (stdin): trailing garbage after EOF ignored\n".to_vec(), 0),
            Trailing::None => (Vec::new(), 0),
        },
        (codec::Format::Xz | codec::Format::Lzma, Some(e)) => {
            let what = match e {
                DecodeError::NotFormat => "File format not recognized",
                DecodeError::Truncated => "Unexpected end of input",
                _ => "Compressed data is corrupt",
            };
            (format!("xz: (stdin): {what}\n").into_bytes(), 1)
        }
        (codec::Format::Xz | codec::Format::Lzma, None) => match trailing {
            Trailing::Garbage(_) => (b"xz: (stdin): Unexpected end of input\n".to_vec(), 1),
            _ => (Vec::new(), 0),
        },
        (codec::Format::Zstd, Some(e)) => {
            let what = match e {
                DecodeError::NotFormat => return (b"zstd: /*stdin*\\: unsupported format \n".to_vec(), 1),
                DecodeError::Truncated => "Read error (39) : premature end ",
                DecodeError::Checksum => "Decoding error (36) : Restored data doesn't match checksum ",
                _ => "Decoding error (36) : Data corruption detected ",
            };
            (format!("/*stdin*\\ : {what}\n").into_bytes(), 1)
        }
        (codec::Format::Zstd, None) => match trailing {
            Trailing::Garbage(_) => (b"zstd: /*stdin*\\: unsupported format \n".to_vec(), 1),
            _ => (Vec::new(), 0),
        },
        (codec::Format::Lzip, Some(e)) => {
            let what = match e {
                DecodeError::NotFormat => "Bad magic number (file not in lzip format).".to_string(),
                DecodeError::Truncated => "File ends unexpectedly at pos 0".to_string(),
                _ => "Decoder error at pos 0".to_string(),
            };
            (format!("  (stdin): {what}\n").into_bytes(), 2)
        }
        (codec::Format::Lzip, None) => (Vec::new(), 0),
    }
}

/// Lê o resto de um fd pra memória (depois do prefixo já lido).
fn read_rest(fd: Fd, mut data: Vec<u8>) -> Result<Vec<u8>, Errno> {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match sysabi::sys::read(fd, &mut buf) {
            Ok(0) => return Ok(data),
            Ok(n) => {
                if data.try_reserve(n).is_err() {
                    return Err(Errno::ENOMEM);
                }
                data.extend_from_slice(&buf[..n]);
            }
            Err(Errno::EINTR) => {}
            Err(e) => return Err(e),
        }
    }
}

/// Tudo que o decodificador em fluxo consegue tirar antes do erro (o que o filho do GNU já tinha
/// escrito no cano quando parou).
fn partial_decode(fmt: codec::Format, input: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut out = Vec::new();
    let Ok(mut r) = codec::decoder(fmt, std::io::Cursor::new(input)) else { return out };
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => return out,
            Ok(n) => out.extend_from_slice(&buf[..n]),
        }
    }
}

/// Descomprime em processo e monta o leitor; a mensagem do "filho" sai já.
fn decode_into_reader(t: &mut Tar, fmt: codec::Format, data: Vec<u8>) -> (Reader, Child) {
    let fmt = if fmt == codec::Format::Xz && codec::Format::sniff(&data) == Some(codec::Format::Lzma) {
        codec::Format::Lzma
    } else {
        fmt
    };
    match codec::decompress(fmt, &data) {
        Ok(d) => {
            let (msg, status) = child_message(fmt, None, d.trailing);
            if !msg.is_empty() {
                t.out.flush();
                crate::sysutil::eprint(msg);
            }
            t.child_failed = status != 0;
            (Reader::new(Source::Mem { data: d.data, pos: 0 }), Child { status })
        }
        Err(e) => {
            let (msg, status) = child_message(fmt, Some(&e), Trailing::None);
            t.out.flush();
            crate::sysutil::eprint(msg);
            t.child_failed = status != 0;
            let partial = partial_decode(fmt, &data);
            (Reader::new(Source::Mem { data: partial, pos: 0 }), Child { status })
        }
    }
}

/// Abre o arquivo pra leitura (com descompressão).
pub fn open_read(t: &mut Tar, name: &[u8]) -> R<(Reader, Child)> {
    let stdin = name == b"-";
    let fd = if stdin {
        Fd::STDIN
    } else {
        match super::open(name, OFlags::RDONLY, 0) {
            Ok(fd) => fd,
            Err(e) => {
                let mut m = quote::colon(name);
                m.extend_from_slice(format!(": Cannot open: {}", e.message()).as_bytes());
                return Err(t.fatal(m));
            }
        }
    };
    let read_err = |t: &mut Tar, e: Errno| -> Fatal {
        let mut m = quote::colon(name);
        m.extend_from_slice(format!(": Read error: {}", e.message()).as_bytes());
        t.fatal(m)
    };
    if let Some(c) = t.o.compression.clone() {
        if let Some(prog) = program_of(&c) {
            return external_decompress(t, &prog, fd);
        }
        let data = read_rest(fd, Vec::new()).map_err(|e| read_err(t, e))?;
        let fmt = codec_format(&c).expect("formato do codec");
        return Ok(decode_into_reader(t, fmt, data));
    }
    // Olha o começo pra saber se está comprimido.
    let mut head = vec![0u8; 512];
    let mut got = 0;
    while got < head.len() {
        match sysabi::sys::read(fd, &mut head[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(Errno::EINTR) => {}
            Err(e) => return Err(read_err(t, e)),
        }
    }
    head.truncate(got);
    if let Some(fmt) = codec::Format::sniff(&head) {
        if stdin {
            let msg = format!("Archive is compressed. Use {} option", option_for(fmt));
            return Err(t.fatal(msg));
        }
        let data = read_rest(fd, head).map_err(|e| read_err(t, e))?;
        return Ok(decode_into_reader(t, fmt, data));
    }
    let mut r = Reader::new(Source::Fd(fd));
    r.prepend(head);
    Ok((r, Child { status: 0 }))
}

/// Fecha a leitura: o status do "filho" vira erro fatal, como o GNU faz ao esperar o processo.
pub fn finish_read(t: &mut Tar, child: Child, res: R<()>) -> R<()> {
    res?;
    if child.status != 0 {
        let msg = format!("Child returned status {}", child.status);
        return Err(t.fatal(msg));
    }
    Ok(())
}

/// Compressão a usar na escrita: a opção explícita, ou pelo sufixo com `-a`.
pub fn write_compression(t: &Tar, name: &[u8]) -> Option<Compression> {
    if let Some(c) = &t.o.compression {
        return Some(c.clone());
    }
    if t.o.auto_compress {
        let suffixes: &[(&[u8], Compression)] = &[
            (b".tar.gz", Compression::Gzip),
            (b".tgz", Compression::Gzip),
            (b".taz", Compression::Gzip),
            (b".gz", Compression::Gzip),
            (b".tar.Z", Compression::Compress),
            (b".taZ", Compression::Compress),
            (b".Z", Compression::Compress),
            (b".tar.bz2", Compression::Bzip2),
            (b".tz2", Compression::Bzip2),
            (b".tbz2", Compression::Bzip2),
            (b".tbz", Compression::Bzip2),
            (b".bz2", Compression::Bzip2),
            (b".tar.lz", Compression::Lzip),
            (b".lz", Compression::Lzip),
            (b".tar.lzma", Compression::Lzma),
            (b".tlz", Compression::Lzma),
            (b".lzma", Compression::Lzma),
            (b".tar.lzo", Compression::Lzop),
            (b".lzo", Compression::Lzop),
            (b".tar.xz", Compression::Xz),
            (b".txz", Compression::Xz),
            (b".xz", Compression::Xz),
            (b".tar.zst", Compression::Zstd),
            (b".tzst", Compression::Zstd),
            (b".zst", Compression::Zstd),
        ];
        for (s, c) in suffixes {
            if name.ends_with(s) {
                return Some(c.clone());
            }
        }
    }
    None
}

/// Prepara o destino dos blocos pra escrita do arquivo `fd` com a compressão dada.
pub fn sink_for(t: &mut Tar, fd: Fd, comp: &Option<Compression>) -> R<(Sink, Option<Vec<u8>>)> {
    match comp {
        None => Ok((Sink::Fd(fd), None)),
        Some(c) => {
            if let Some(prog) = program_of(c) {
                return Ok((Sink::Mem(Vec::new()), Some(prog)));
            }
            let fmt = codec_format(c).expect("formato");
            let level = fmt.default_level();
            match codec::Encoder::new(fmt, level, &codec::GzipHeader::default(), FdWriter(fd)) {
                Ok(e) => Ok((Sink::Encoder(e), None)),
                Err(e) => Err(t.fatal(format!("Cannot start compressor: {e}"))),
            }
        }
    }
}

/// Termina a escrita: fecha o fluxo e, com programa externo, roda o compressor.
pub fn finish_write(t: &mut Tar, w: Writer, fd: Fd, program: Option<Vec<u8>>, name: &[u8]) -> R<()> {
    match w.finish(true) {
        Ok(Some(data)) => match program {
            Some(prog) => external_compress(t, &prog, &data, fd),
            None => Ok(()),
        },
        Ok(None) => Ok(()),
        Err(e) => {
            let errno = sysabi::Errno::from_io(&e);
            let mut m = quote::colon(name);
            m.extend_from_slice(format!(": Cannot write: {}", errno.message()).as_bytes());
            Err(t.fatal(m))
        }
    }
}

/// Roda `/bin/sh -c CMD` herdando a entrada e a saída (o `--checkpoint-action=exec`).
pub fn run_shell_command(t: &mut Tar, command: &[u8]) -> R<i32> {
    run_shell(t, command, Fd::STDIN, Fd::STDOUT)
}

/// Roda `/bin/sh -c PROG` com a entrada e a saída dadas; devolve o status.
fn run_shell(t: &mut Tar, command: &[u8], stdin: Fd, stdout: Fd) -> R<i32> {
    use sysabi::{FdAction, ProcAttrs, SpawnSpec, WaitOptions, WaitTarget};
    let sys = sysabi::sys::current();
    let mut fd_actions = Vec::new();
    if stdin != Fd::STDIN {
        fd_actions.push(FdAction::Dup2 { from: stdin, to: Fd::STDIN });
    }
    if stdout != Fd::STDOUT {
        fd_actions.push(FdAction::Dup2 { from: stdout, to: Fd::STDOUT });
    }
    let spec = SpawnSpec {
        path: b"/bin/sh".to_vec(),
        argv: vec![b"/bin/sh".to_vec(), b"-c".to_vec(), command.to_vec()],
        attrs: ProcAttrs { fd_actions, ..ProcAttrs::default() },
    };
    t.out.flush();
    let pid = match sys.spawn(spec) {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("Cannot fork: {}", e.message());
            return Err(t.fatal(msg));
        }
    };
    match sys.wait4(WaitTarget::Pid(pid), WaitOptions::empty()) {
        Ok(Some((_, st))) => Ok(st.shell_status()),
        _ => Ok(127),
    }
}

/// Arquivo temporário anônimo em /tmp.
fn temp_fd(t: &mut Tar) -> R<Fd> {
    let sys = sysabi::sys::current();
    for i in 0..1000u32 {
        let name = format!("/tmp/tar-{}-{i}", sys.getpid());
        match sysabi::sys::open(name.as_bytes(), OFlags::RDWR | OFlags::CREAT | OFlags::EXCL | OFlags::CLOEXEC, 0o600) {
            Ok(fd) => {
                let _ = sys.unlinkat(Fd::CWD, name.as_bytes(), sysabi::AtFlags::empty());
                return Ok(fd);
            }
            Err(Errno::EEXIST) => continue,
            Err(e) => {
                let msg = format!("Cannot create temporary file: {}", e.message());
                return Err(t.fatal(msg));
            }
        }
    }
    Err(t.fatal("Cannot create temporary file"))
}

fn external_decompress(t: &mut Tar, prog: &[u8], fd: Fd) -> R<(Reader, Child)> {
    let tmp = temp_fd(t)?;
    let mut cmd = prog.to_vec();
    cmd.extend_from_slice(b" -d");
    let status = run_shell(t, &cmd, fd, tmp)?;
    let sys = sysabi::sys::current();
    let _ = sys.lseek(tmp, 0, sysabi::Whence::Set);
    let data = read_rest(tmp, Vec::new()).unwrap_or_default();
    let _ = sys.close(tmp);
    t.child_failed = status != 0;
    Ok((Reader::new(Source::Mem { data, pos: 0 }), Child { status }))
}

fn external_compress(t: &mut Tar, prog: &[u8], data: &[u8], fd: Fd) -> R<()> {
    let tmp = temp_fd(t)?;
    let sys = sysabi::sys::current();
    if sysabi::sys::write_all(tmp, data).is_err() {
        return Err(t.fatal("Cannot write temporary file"));
    }
    let _ = sys.lseek(tmp, 0, sysabi::Whence::Set);
    let status = run_shell(t, prog, tmp, fd)?;
    let _ = sys.close(tmp);
    if status != 0 {
        let msg = format!("Child returned status {status}");
        return Err(t.fatal(msg));
    }
    Ok(())
}
