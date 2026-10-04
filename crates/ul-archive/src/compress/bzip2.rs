//! `bzip2` 1.0.8, com `bunzip2` e `bzcat` (o mesmo programa, que muda o modo pelo nome com que foi
//! chamado). Escrito a partir do manual do bzip2 e do comportamento observado no oráculo: flags e
//! arquivos em qualquer ordem, `BZIP2`/`BZIP` no ambiente, mensagens e códigos de saída (1 pra
//! problema de ambiente, 2 pra arquivo corrompido), estatísticas do `-v` e os erros de dados que
//! abortam a execução apagando a saída.

use std::io::Write;

use sysabi::{Errno, Fd, Stat};

use super::common::{self, Input, Sink};

const USAGE_TAIL: &str = " [flags and input files in any order]

   -h --help           print this message
   -d --decompress     force decompression
   -z --compress       force compression
   -k --keep           keep (don't delete) input files
   -f --force          overwrite existing output files
   -t --test           test compressed file integrity
   -c --stdout         output to standard out
   -q --quiet          suppress noncritical error messages
   -v --verbose        be verbose (a 2nd -v gives more)
   -L --license        display software version & license
   -V --version        display software version & license
   -s --small          use less memory (at most 2500k)
   -1 .. -9            set block size to 100k .. 900k
   --fast              alias for -1
   --best              alias for -9

   If invoked as `bzip2', default action is to compress.
              as `bunzip2',  default action is to decompress.
              as `bzcat', default action is to decompress to stdout.

   If no file names are given, bzip2 compresses or decompresses
   from standard input to standard output.  You can combine
   short flags, so `-v -4' means the same as -v4 or -4v, &c.

";

const BANNER: &str = "bzip2, a block-sorting file compressor.  Version 1.0.8, 13-Jul-2019.\n";

// As linhas "vazias" do texto de licença do bzip2 têm três espaços (escritos como `\x20`).
const LICENSE: &str = "\x20\x20\x20\n   Copyright (C) 1996-2019 by Julian Seward.\n\x20\x20\x20\n   This program is free software; you can redistribute it and/or modify\n   it under the terms set out in the LICENSE file, which is included\n   in the bzip2 source distribution.\n\x20\x20\x20\n   This program is distributed in the hope that it will be useful,\n   but WITHOUT ANY WARRANTY; without even the implied warranty of\n   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the\n   LICENSE file for more details.\n\x20\x20\x20\n";

const RECOVER: &str = "\nYou can use the `bzip2recover' program to attempt to recover\ndata from undamaged sections of corrupted files.\n\n";

const CORRUPT_ADVICE: &str = "\nIt is possible that the compressed file(s) have become corrupted.\nYou can use the -tvv option to test integrity of such files.\n";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Compress,
    Decompress,
    Test,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Src {
    /// Entrada padrão pra saída padrão.
    StdinToStdout,
    /// Arquivo pra saída padrão (`-c`).
    FileToStdout,
    /// Arquivo pra arquivo.
    FileToFile,
}

/// Erro que encerra o programa (o `cleanUpAndFail` do bzip2): mensagem já impressa, código dado.
struct Exit(i32);

struct Bzip2 {
    prog: String,
    op: Op,
    src: Src,
    block: u32,
    keep: bool,
    force: bool,
    quiet: bool,
    verbose: u32,
    exit: i32,
    longest: usize,
    test_failed: bool,
    num_files: usize,
    processed: usize,
}

/// Como terminou a leitura de um fluxo bzip2.
enum Outcome {
    Ok,
    /// Não é bzip2 (magia errada no primeiro fluxo).
    NotBzip2,
    Truncated,
    Crc,
    /// Lixo depois do último fluxo (avisado, não é erro).
    Trailing,
    Write(Errno),
    Read(Errno),
}

impl Bzip2 {
    fn set_exit(&mut self, v: i32) {
        self.exit = self.exit.max(v);
    }

    fn noisy(&self) -> bool {
        !self.quiet
    }

    fn pad(&self, name: &str) -> String {
        " ".repeat(self.longest.saturating_sub(name.chars().count()))
    }

    fn usage(&self) {
        common::eprint(format!("{BANNER}\n   usage: {}{USAGE_TAIL}", self.prog));
    }

    fn run(&mut self, args: &[Vec<u8>]) -> i32 {
        self.prog = common::show(common::base_name(args.first().map(Vec::as_slice).unwrap_or(b"bzip2")));
        let lower = self.prog.clone();
        if lower.contains("unzip") || lower.contains("UNZIP") {
            self.op = Op::Decompress;
        }
        let as_cat = lower.contains("z2cat") || lower.contains("Z2CAT") || lower.contains("zcat") || lower.contains("ZCAT");
        if as_cat {
            self.op = Op::Decompress;
        }
        // Flags do ambiente vêm antes das da linha de comando.
        let mut all: Vec<Vec<u8>> = Vec::new();
        for var in ["BZIP2", "BZIP"] {
            if let Some(v) = common::getenv(var) {
                all.extend(v.split(|b| b.is_ascii_whitespace()).filter(|s| !s.is_empty()).map(<[u8]>::to_vec));
            }
        }
        all.extend(args.iter().skip(1).cloned());
        let mut files: Vec<Vec<u8>> = Vec::new();
        let mut flags_done = false;
        let mut to_stdout = as_cat;
        for a in &all {
            if a.as_slice() == b"--" {
                flags_done = true;
                continue;
            }
            if flags_done || a.first() != Some(&b'-') || a.len() == 1 {
                files.push(a.clone());
                continue;
            }
            if a.starts_with(b"--") {
                match a.as_slice() {
                    b"--stdout" => to_stdout = true,
                    b"--decompress" => self.op = Op::Decompress,
                    b"--compress" => self.op = Op::Compress,
                    b"--force" => self.force = true,
                    b"--test" => self.op = Op::Test,
                    b"--keep" => self.keep = true,
                    b"--small" => {}
                    b"--quiet" => self.quiet = true,
                    b"--version" | b"--license" => {
                        common::eprint(format!("{BANNER}{LICENSE}"));
                        return 0;
                    }
                    b"--exponential" => {}
                    b"--repetitive-best" | b"--repetitive-fast" => {
                        if self.noisy() {
                            common::eprint(format!(
                                "{}: {} is redundant in versions 0.9.5 and above\n",
                                self.prog,
                                common::show(a)
                            ));
                        }
                    }
                    b"--fast" => self.block = 1,
                    b"--best" => self.block = 9,
                    b"--verbose" => self.verbose += 1,
                    b"--help" => {
                        self.usage();
                        return 0;
                    }
                    _ => {
                        common::eprint(format!("{}: Bad flag `{}'\n", self.prog, common::show(a)));
                        self.usage();
                        return 1;
                    }
                }
                continue;
            }
            for &c in &a[1..] {
                match c {
                    b'c' => to_stdout = true,
                    b'd' => self.op = Op::Decompress,
                    b'z' => self.op = Op::Compress,
                    b'f' => self.force = true,
                    b't' => self.op = Op::Test,
                    b'k' => self.keep = true,
                    b's' => {}
                    b'q' => self.quiet = true,
                    b'1'..=b'9' => self.block = (c - b'0') as u32,
                    b'V' | b'L' => {
                        common::eprint(format!("{BANNER}{LICENSE}"));
                        return 0;
                    }
                    b'v' => self.verbose += 1,
                    b'h' => {
                        self.usage();
                        return 0;
                    }
                    _ => {
                        common::eprint(format!("{}: Bad flag `{}'\n", self.prog, common::show(a)));
                        self.usage();
                        return 1;
                    }
                }
            }
        }
        for f in &files {
            self.longest = self.longest.max(String::from_utf8_lossy(f).chars().count());
        }
        self.src = if files.is_empty() {
            Src::StdinToStdout
        } else if to_stdout {
            Src::FileToStdout
        } else {
            Src::FileToFile
        };
        let r = if files.is_empty() {
            self.process(None)
        } else {
            self.num_files = files.len();
            files.iter().try_for_each(|f| {
                self.processed += 1;
                self.process(Some(f))
            })
        };
        if let Err(Exit(code)) = r {
            if self.noisy() && self.processed < self.num_files {
                common::eprint(format!(
                    "{p}: WARNING: some files have not been processed:\n{p}:    {} specified on command line, {} not processed yet.\n\n",
                    self.num_files,
                    self.num_files - self.processed,
                    p = self.prog
                ));
            }
            return code;
        }
        if self.op == Op::Test && self.test_failed && self.noisy() {
            common::eprint(RECOVER);
        }
        self.exit
    }

    fn process(&mut self, name: Option<&[u8]>) -> Result<(), Exit> {
        match self.op {
            Op::Compress => self.compress(name),
            Op::Decompress => self.decompress(name),
            Op::Test => {
                self.test(name);
                Ok(())
            }
        }
    }

    /// Abre e confere a entrada de um modo com arquivo. `None` quando o erro já foi reportado.
    fn open_checked(&mut self, path: &[u8], shown: &str, check_suffix: bool) -> Option<(Fd, Stat)> {
        // O teste escreve "Can't open input X"; compressão e descompressão, "Can't open input file X".
        let what = if self.op == Op::Test { "input" } else { "input file" };
        let lst = match common::lstat(path) {
            Ok(s) => s,
            Err(e) => {
                common::eprint(format!("{}: Can't open {what} {shown}: {}.\n", self.prog, e.message()));
                self.set_exit(1);
                return None;
            }
        };
        if check_suffix {
            for s in [&b".bz2"[..], b".bz", b".tbz2", b".tbz"] {
                if path.ends_with(s) {
                    if self.noisy() {
                        common::eprint(format!(
                            "{}: Input file {shown} already has {} suffix.\n",
                            self.prog,
                            common::show(s)
                        ));
                    }
                    self.set_exit(1);
                    return None;
                }
            }
        }
        let st = common::stat(path).unwrap_or(lst.clone());
        if st.file_type() == sysabi::FileType::Directory {
            common::eprint(format!("{}: Input file {shown} is a directory.\n", self.prog));
            self.set_exit(1);
            return None;
        }
        if self.src == Src::FileToFile && !self.force && self.op != Op::Test {
            if lst.file_type() != sysabi::FileType::Regular {
                if self.noisy() {
                    common::eprint(format!("{}: Input file {shown} is not a normal file.\n", self.prog));
                }
                self.set_exit(1);
                return None;
            }
            if lst.nlink > 1 {
                let n = lst.nlink - 1;
                if self.noisy() {
                    common::eprint(format!(
                        "{}: Input file {shown} has {n} other link{}.\n",
                        self.prog,
                        if n > 1 { "s" } else { "" }
                    ));
                }
                self.set_exit(1);
                return None;
            }
        }
        match common::open_input(path, false) {
            Ok(fd) => Some((fd, st)),
            Err(e) => {
                common::eprint(format!("{}: Can't open {what} {shown}: {}.\n", self.prog, e.message()));
                self.set_exit(1);
                None
            }
        }
    }

    /// Cria a saída de um modo arquivo pra arquivo, respeitando `-f`.
    fn create_out(&mut self, out: &[u8]) -> Option<Fd> {
        let shown = common::show(out);
        if common::lstat(out).is_ok() {
            if !self.force {
                common::eprint(format!("{}: Output file {shown} already exists.\n", self.prog));
                self.set_exit(1);
                return None;
            }
            let _ = common::unlink(out);
        }
        match common::create_exclusive(out) {
            Ok(fd) => Some(fd),
            Err(e) => {
                common::eprint(format!("{}: Can't create output file {shown}: {}.\n", self.prog, e.message()));
                self.set_exit(1);
                None
            }
        }
    }

    fn terminal_check(&mut self, reading: bool) -> bool {
        let tty = if reading { common::isatty(Fd::STDIN) } else { common::isatty(Fd::STDOUT) };
        if tty {
            let what = if reading { "read compressed data from" } else { "write compressed data to" };
            common::eprint(format!(
                "{}: I won't {what} a terminal.\n{}: For help, type: `{} --help'.\n",
                self.prog, self.prog, self.prog
            ));
            self.set_exit(1);
        }
        tty
    }

    fn compress(&mut self, name: Option<&[u8]>) -> Result<(), Exit> {
        let shown = name.map(common::show).unwrap_or_else(|| "(stdin)".into());
        let (fd, st) = match name {
            None => (Fd::STDIN, None),
            Some(p) => match self.open_checked(p, &shown, true) {
                Some((fd, st)) => (fd, Some(st)),
                None => return Ok(()),
            },
        };
        if self.src != Src::FileToFile && self.terminal_check(false) {
            if name.is_some() {
                common::close(fd);
            }
            return Ok(());
        }
        let out_path = name.map(|p| common::cat(&[p, b".bz2"]));
        let ofd = match (&out_path, self.src) {
            (Some(o), Src::FileToFile) => match self.create_out(o) {
                Some(f) => f,
                None => {
                    common::close(fd);
                    return Ok(());
                }
            },
            _ => Fd::STDOUT,
        };
        if self.verbose > 0 {
            common::eprint(format!("  {shown}: {}", self.pad(&shown)));
        }
        let mut input = Input::new(fd);
        let mut sink = Sink::fd(ofd);
        let r = (|| -> std::io::Result<()> {
            let mut enc = bzip2::write::BzEncoder::new(&mut sink, bzip2::Compression::new(self.block));
            loop {
                let chunk = input.fill();
                if chunk.is_empty() {
                    break;
                }
                let n = chunk.len();
                enc.write_all(chunk)?;
                input.consume(n);
            }
            if let Some(e) = input.error {
                return Err(e.to_io());
            }
            enc.finish()?;
            Ok(())
        })();
        if name.is_some() {
            common::close(fd);
        }
        if let Err(e) = r {
            let e = Errno::from_io(&e);
            common::eprint(format!("{}: I/O or other error, bailing out.  Possible reason follows.\n{}: {}\n", self.prog, self.prog, e.message()));
            if let (Some(o), Src::FileToFile) = (&out_path, self.src) {
                common::close(ofd);
                let _ = common::unlink(o);
            }
            return Err(Exit(1));
        }
        if self.verbose > 0 {
            let (i, o) = (input.consumed, sink.written);
            if i == 0 {
                common::eprint(" no data compressed.\n");
            } else {
                let (fi, fo) = (i as f64, o as f64);
                common::eprint(format!(
                    "{:6.3}:1, {:6.3} bits/byte, {:5.2}% saved, {i} in, {o} out.\n",
                    fi / fo,
                    8.0 * fo / fi,
                    100.0 * (1.0 - fo / fi)
                ));
            }
        }
        if let (Some(o), Src::FileToFile, Some(st), Some(p)) = (&out_path, self.src, &st, name) {
            common::copy_attrs(ofd, o, st, st.mtime);
            common::close(ofd);
            if !self.keep {
                let _ = common::unlink(p);
            }
        }
        Ok(())
    }

    /// Decodifica todos os fluxos de `input` em `sink`.
    fn unzip(&mut self, input: &mut Input, sink: &mut Sink) -> Outcome {
        let mut streams = 0usize;
        let mut chunk = vec![0u8; common::CHUNK];
        loop {
            if streams > 0 {
                let peek = input.ensure(4);
                if peek.is_empty() {
                    return Outcome::Ok;
                }
                if !(peek.len() >= 4 && peek.starts_with(b"BZh") && (b'1'..=b'9').contains(&peek[3])) {
                    input.drain_check_zeros();
                    return Outcome::Trailing;
                }
            } else {
                let peek = input.ensure(4);
                if !(peek.len() >= 4 && peek.starts_with(b"BZh") && (b'1'..=b'9').contains(&peek[3])) {
                    if let Some(e) = input.error {
                        return Outcome::Read(e);
                    }
                    return Outcome::NotBzip2;
                }
            }
            let mut dec = bzip2::Decompress::new(false);
            loop {
                sysabi::sys::checkpoint();
                let avail = input.fill();
                if avail.is_empty() {
                    if let Some(e) = input.error {
                        return Outcome::Read(e);
                    }
                    return Outcome::Truncated;
                }
                let (in0, out0) = (dec.total_in(), dec.total_out());
                let st = dec.decompress(avail, &mut chunk);
                let consumed = (dec.total_in() - in0) as usize;
                let produced = (dec.total_out() - out0) as usize;
                input.consume(consumed);
                if produced > 0
                    && let Err(e) = sink.write_all(&chunk[..produced])
                {
                    return Outcome::Write(Errno::from_io(&e));
                }
                match st {
                    Ok(bzip2::Status::StreamEnd) => break,
                    Ok(_) => {}
                    Err(bzip2::Error::DataMagic) => return Outcome::NotBzip2,
                    Err(_) => return Outcome::Crc,
                }
            }
            streams += 1;
        }
    }

    fn decompress(&mut self, name: Option<&[u8]>) -> Result<(), Exit> {
        let shown = name.map(common::show).unwrap_or_else(|| "(stdin)".into());
        let (fd, st) = match name {
            None => (Fd::STDIN, None),
            Some(p) => match self.open_checked(p, &shown, false) {
                Some((fd, st)) => (fd, Some(st)),
                None => return Ok(()),
            },
        };
        if name.is_none() && self.terminal_check(true) {
            return Ok(());
        }
        // Nome de saída: tira o sufixo conhecido, ou acrescenta ".out".
        let out_path = match (name, self.src) {
            (Some(p), Src::FileToFile) => {
                let mut out = None;
                for (s, r) in [(&b".bz2"[..], &b""[..]), (b".bz", b""), (b".tbz2", b".tar"), (b".tbz", b".tar")] {
                    if p.len() > s.len() && p.ends_with(s) {
                        out = Some(common::cat(&[&p[..p.len() - s.len()], r]));
                        break;
                    }
                }
                Some(match out {
                    Some(o) => o,
                    None => {
                        let o = common::cat(&[p, b".out"]);
                        if self.noisy() {
                            common::eprint(format!(
                                "{}: Can't guess original name for {shown} -- using {}\n",
                                self.prog,
                                common::show(&o)
                            ));
                        }
                        o
                    }
                })
            }
            _ => None,
        };
        let ofd = match &out_path {
            Some(o) => match self.create_out(o) {
                Some(f) => f,
                None => {
                    common::close(fd);
                    return Ok(());
                }
            },
            None => Fd::STDOUT,
        };
        if self.verbose > 0 {
            common::eprint(format!("  {shown}: {}", self.pad(&shown)));
        }
        let mut input = Input::new(fd);
        let mut sink = Sink::fd(ofd);
        let outcome = self.unzip(&mut input, &mut sink);
        if name.is_some() {
            common::close(fd);
        }
        let out_shown = out_path.as_deref().map(common::show).unwrap_or_else(|| "(stdout)".into());
        let remove_out = |me: &Bzip2| {
            if let Some(o) = &out_path {
                common::close(ofd);
                let _ = common::unlink(o);
                let _ = me;
            }
        };
        match outcome {
            Outcome::Ok | Outcome::Trailing => {
                if matches!(outcome, Outcome::Trailing) && self.noisy() {
                    common::eprint(format!("\n{}: {shown}: trailing garbage after EOF ignored\n", self.prog));
                }
                if self.verbose > 0 {
                    common::eprint("done\n");
                }
                if let (Some(o), Some(st), Some(p)) = (&out_path, &st, name) {
                    common::copy_attrs(ofd, o, st, st.mtime);
                    common::close(ofd);
                    if !self.keep {
                        let _ = common::unlink(p);
                    }
                }
                Ok(())
            }
            Outcome::NotBzip2 => {
                remove_out(self);
                if self.noisy() {
                    common::eprint(format!("{}: {shown} is not a bzip2 file.\n", self.prog));
                }
                self.set_exit(2);
                Ok(())
            }
            Outcome::Truncated => {
                common::eprint(format!(
                    "\n{p}: Compressed file ends unexpectedly;\n\tperhaps it is corrupted?  *Possible* reason follows.\n{p}: {reason}\n\tInput file = {shown}, output file = {out_shown}\n{CORRUPT_ADVICE}{RECOVER}",
                    p = self.prog,
                    // O "motivo" é o errno que sobrou: a conferência de que a saída não existe deixa
                    // ENOENT no modo arquivo pra arquivo; pra stdout ele é zero.
                    reason = if out_path.is_some() { Errno::ENOENT.message() } else { "Success".to_string() }
                ));
                self.fail_cleanup(&out_path, ofd);
                Err(Exit(2))
            }
            Outcome::Crc => {
                common::eprint(format!(
                    "\n{p}: Data integrity error when decompressing.\n\tInput file = {shown}, output file = {out_shown}\n{CORRUPT_ADVICE}{RECOVER}",
                    p = self.prog
                ));
                self.fail_cleanup(&out_path, ofd);
                Err(Exit(2))
            }
            Outcome::Write(e) | Outcome::Read(e) => {
                common::eprint(format!(
                    "\n{p}: I/O or other error, bailing out.  Possible reason follows.\n{p}: {}\n\tInput file = {shown}, output file = {out_shown}\n",
                    e.message(),
                    p = self.prog
                ));
                self.fail_cleanup(&out_path, ofd);
                Err(Exit(1))
            }
        }
    }

    /// Apaga a saída parcial como o bzip2 faz antes de sair com erro.
    fn fail_cleanup(&mut self, out: &Option<Vec<u8>>, ofd: Fd) {
        if let Some(o) = out {
            common::close(ofd);
            if self.noisy() {
                common::eprint(format!("{}: Deleting output file {}, if it exists.\n", self.prog, common::show(o)));
            }
            let _ = common::unlink(o);
        }
    }

    fn test(&mut self, name: Option<&[u8]>) {
        let shown = name.map(common::show).unwrap_or_else(|| "(stdin)".into());
        let fd = match name {
            None => {
                if self.terminal_check(true) {
                    return;
                }
                Fd::STDIN
            }
            Some(p) => match self.open_checked(p, &shown, false) {
                Some((fd, _)) => fd,
                None => return,
            },
        };
        if self.verbose > 0 {
            common::eprint(format!("  {shown}: {}", self.pad(&shown)));
        }
        let mut input = Input::new(fd);
        let mut sink = Sink::null();
        let outcome = self.unzip(&mut input, &mut sink);
        if name.is_some() {
            common::close(fd);
        }
        let prefix = if self.verbose == 0 { format!("{}: {shown}: ", self.prog) } else { String::new() };
        let what = match outcome {
            Outcome::Ok => {
                if self.verbose > 0 {
                    common::eprint("ok\n");
                }
                return;
            }
            Outcome::Trailing => {
                if self.noisy() {
                    common::eprint(format!("{prefix}trailing garbage after EOF ignored\n"));
                }
                if self.verbose > 0 {
                    common::eprint("ok\n");
                }
                return;
            }
            Outcome::NotBzip2 => "bad magic number (file not created by bzip2)",
            Outcome::Truncated => "file ends unexpectedly",
            Outcome::Crc => "data integrity (CRC) error in data",
            Outcome::Write(e) | Outcome::Read(e) => {
                common::eprint(format!("{prefix}{}\n", e.message()));
                self.set_exit(1);
                return;
            }
        };
        common::eprint(format!("{prefix}{what}\n"));
        self.test_failed = true;
        self.set_exit(2);
    }
}

/// Entrada do `bzip2` (e dos aliases, pelo argv[0]).
pub fn main(args: &[Vec<u8>]) -> i32 {
    let mut b = Bzip2 {
        prog: String::new(),
        op: Op::Compress,
        src: Src::StdinToStdout,
        block: 9,
        keep: false,
        force: false,
        quiet: false,
        verbose: 0,
        exit: 0,
        longest: 7,
        test_failed: false,
        num_files: 0,
        processed: 0,
    };
    b.run(args)
}
