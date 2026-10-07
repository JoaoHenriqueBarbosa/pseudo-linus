//! `ar` e `ranlib` do GNU binutils 2.44 (Debian 13), a partir do comportamento observado no
//! oráculo e da man page (o código do binutils é GPL e não foi consultado).
//!
//! Formato GNU/SysV: `!<arch>\n`, cabeçalhos de 60 bytes, tabela de símbolos `/` (contagem e
//! deslocamentos em big-endian, nomes terminados em NUL), tabela de nomes longos `//` e nomes
//! longos referidos por `/deslocamento`. Operações `d m p q r s t x`; modificadores
//! `a b c D f i N o O P s S T u U v V`.
//!
//! O Debian compila o binutils com `--enable-deterministic-archives`: por padrão (`D`) os membros
//! novos levam data, dono e grupo zero e modo 644; `U` usa os dados reais do arquivo.
//!
//! Divergências conhecidas: `-M` (script MRI), arquivos finos (`T`/`--thin`), `--record-libdeps`,
//! `--plugin`, `--target` e `O` (mostrar deslocamentos em `t`) são aceitos mas não têm efeito;
//! `P` não muda a comparação de nomes além de guardar o caminho inteiro; o fuso das datas de
//! `tv` é UTC.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, OFlags, SetTime, TimeSpec, sys};

use crate::strings::{TARGETS, expand_response_files};
use crate::util::io::{self, File};
use crate::util::{Getopt, HasArg, LongOpt};

use super::elf::Elf;

const SHORTOPTS: &str = "dmpqrtxcoOVvuaibDUfNPTsSMhH";

const ID_PLUGIN: i32 = 256;
const ID_TARGET: i32 = 257;
const ID_OUTPUT: i32 = 258;
const ID_LIBDEPS: i32 = 259;
const ID_THIN: i32 = 260;

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("plugin", HasArg::Required, ID_PLUGIN),
    LongOpt::new("target", HasArg::Required, ID_TARGET),
    LongOpt::new("version", HasArg::No, 'V' as i32),
    LongOpt::new("output", HasArg::Required, ID_OUTPUT),
    LongOpt::new("record-libdeps", HasArg::Required, ID_LIBDEPS),
    LongOpt::new("thin", HasArg::No, ID_THIN),
];

const VERSION_TAIL: &str = "Copyright (C) 2025 Free Software Foundation, Inc.\n\
This program is free software; you may redistribute it under the terms of\n\
the GNU General Public License version 3 or (at your option) any later version.\n\
This program has absolutely no warranty.\n";

const AR_USAGE_BODY: &str = "       ar -M [<mri-script]\n\
\x20commands:\n\
\x20 d            - delete file(s) from the archive\n\
\x20 m[ab]        - move file(s) in the archive\n\
\x20 p            - print file(s) found in the archive\n\
\x20 q[f]         - quick append file(s) to the archive\n\
\x20 r[ab][f][u]  - replace existing or insert new file(s) into the archive\n\
\x20 s            - act as ranlib\n\
\x20 t[O][v]      - display contents of the archive\n\
\x20 x[o]         - extract file(s) from the archive\n\
\x20command specific modifiers:\n\
\x20 [a]          - put file(s) after [member-name]\n\
\x20 [b]          - put file(s) before [member-name] (same as [i])\n\
\x20 [D]          - use zero for timestamps and uids/gids (default)\n\
\x20 [U]          - use actual timestamps and uids/gids\n\
\x20 [N]          - use instance [count] of name\n\
\x20 [f]          - truncate inserted file names\n\
\x20 [P]          - use full path names when matching\n\
\x20 [o]          - preserve original dates\n\
\x20 [O]          - display offsets of files in the archive\n\
\x20 [u]          - only replace files that are newer than current archive contents\n\
\x20generic modifiers:\n\
\x20 [c]          - do not warn if the library had to be created\n\
\x20 [s]          - create an archive index (cf. ranlib)\n\
\x20 [l <text> ]  - specify the dependencies of this library\n\
\x20 [S]          - do not build a symbol table\n\
\x20 [T]          - deprecated, use --thin instead\n\
\x20 [v]          - be verbose\n\
\x20 [V]          - display the version number\n\
\x20 @<file>      - read options from <file>\n\
\x20 --target=BFDNAME - specify the target object format as BFDNAME\n\
\x20 --output=DIRNAME - specify the output directory for extraction operations\n\
\x20 --record-libdeps=<text> - specify the dependencies of this library\n\
\x20 --thin       - make a thin archive\n\
\x20optional:\n\
\x20 --plugin <p> - load the specified plugin\n\
\x20emulation options: \n\
\x20 No emulation specific options\n";

const RANLIB_USAGE_BODY: &str = " Generate an index to speed access to archives\n\
\x20The options are:\n\
\x20 @<file>                      Read options from <file>\n\
\x20 --plugin <name>              Load the specified plugin\n\
\x20 -D                           Use zero for symbol map timestamp (default)\n\
\x20 -U                           Use an actual symbol map timestamp\n\
\x20 -t                           Update the archive's symbol map timestamp\n\
\x20 -h --help                    Print this help message\n\
\x20 -v --version                 Print version information\n";

pub(crate) fn version_text(tool: &str) -> String {
    format!("GNU {tool} (GNU Binutils for Debian) 2.44\n{VERSION_TAIL}")
}

pub(crate) fn print_version(tool: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(version_text(tool).as_bytes());
}

fn ar_usage(prog: &str, to_stdout: bool) -> i32 {
    let mut text = format!(
        "Usage: {prog} [emulation options] [-]{{dmpqrstx}}[abcDfilMNoOPsSTuvV] [--plugin <name>] [member-name] [count] archive-file file...\n"
    );
    text.push_str(AR_USAGE_BODY);
    text.push_str(&format!(
        "{prog}: supported targets: {}\n",
        TARGETS.join(" ")
    ));
    emit_usage(text, to_stdout)
}

fn ranlib_usage(prog: &str, to_stdout: bool) -> i32 {
    let mut text = format!("Usage: {prog} [options] archive\n");
    text.push_str(RANLIB_USAGE_BODY);
    text.push_str(&format!(
        "{prog}: supported targets: {}\n",
        TARGETS.join(" ")
    ));
    emit_usage(text, to_stdout)
}

fn emit_usage(mut text: String, to_stdout: bool) -> i32 {
    if to_stdout {
        text.push_str("Report bugs to <https://sourceware.org/bugzilla/>\n");
        let mut out = io::stdout();
        let _ = out.write_all(text.as_bytes());
        0
    } else {
        io::eprint(text);
        1
    }
}

/// Um membro do arquivo, já com o nome resolvido (sem a barra final).
#[derive(Clone, Debug)]
struct Member {
    name: Vec<u8>,
    date: u64,
    uid: u64,
    gid: u64,
    mode: u64,
    data: Vec<u8>,
}

fn field_num(raw: &[u8], radix: u32) -> Option<u64> {
    let s = std::str::from_utf8(raw).ok()?.trim();
    if s.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(s, radix).ok()
}

/// Lê um arquivo `ar`. `Err(())` para formato não reconhecido (inclui arquivo fino).
fn parse_archive(bytes: &[u8]) -> Result<(Vec<Member>, bool), ()> {
    if bytes.len() < 8 || &bytes[..8] != b"!<arch>\n" {
        return Err(());
    }
    let mut pos = 8usize;
    let mut ext: Vec<u8> = Vec::new();
    let mut out = Vec::new();
    let mut had_map = false;
    while pos + 60 <= bytes.len() {
        let h = &bytes[pos..pos + 60];
        if &h[58..60] != b"`\n" {
            return Err(());
        }
        let size = field_num(&h[48..58], 10).ok_or(())? as usize;
        let body_start = pos + 60;
        let body_end = body_start.checked_add(size).ok_or(())?;
        if body_end > bytes.len() {
            return Err(());
        }
        let body = &bytes[body_start..body_end];
        let raw_name = &h[..16];
        let trimmed: Vec<u8> = {
            let end = raw_name
                .iter()
                .rposition(|&b| b != b' ')
                .map_or(0, |p| p + 1);
            raw_name[..end].to_vec()
        };
        if trimmed == b"/" {
            had_map = true;
        } else if trimmed == b"//" {
            ext = body.to_vec();
        } else {
            let name = if trimmed.len() > 1
                && trimmed[0] == b'/'
                && trimmed[1..].iter().all(u8::is_ascii_digit)
            {
                let off: usize = std::str::from_utf8(&trimmed[1..])
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .ok_or(())?;
                let tail = ext.get(off..).ok_or(())?;
                let end = tail.iter().position(|&b| b == b'\n').unwrap_or(tail.len());
                let mut n = tail[..end].to_vec();
                if n.last() == Some(&b'/') {
                    n.pop();
                }
                n
            } else {
                let mut n = trimmed.clone();
                if n.last() == Some(&b'/') {
                    n.pop();
                }
                n
            };
            out.push(Member {
                name,
                date: field_num(&h[16..28], 10).ok_or(())?,
                uid: field_num(&h[28..34], 10).ok_or(())?,
                gid: field_num(&h[34..40], 10).ok_or(())?,
                mode: field_num(&h[40..48], 8).ok_or(())?,
                data: body.to_vec(),
            });
        }
        pos = body_end + (size & 1);
    }
    Ok((out, had_map))
}

fn pad_field(v: &[u8], width: usize) -> Vec<u8> {
    let mut b = v.to_vec();
    b.resize(width, b' ');
    b.truncate(width);
    b
}

fn header(name: &[u8], date: &str, uid: &str, gid: &str, mode: &str, size: usize) -> Vec<u8> {
    let mut h = Vec::with_capacity(60);
    h.extend(pad_field(name, 16));
    h.extend(pad_field(date.as_bytes(), 12));
    h.extend(pad_field(uid.as_bytes(), 6));
    h.extend(pad_field(gid.as_bytes(), 6));
    h.extend(pad_field(mode.as_bytes(), 8));
    h.extend(pad_field(size.to_string().as_bytes(), 10));
    h.extend_from_slice(b"`\n");
    h
}

/// Serializa o arquivo. `armap` liga a tabela de símbolos (só entra se houver símbolos).
fn serialize(members: &[Member], armap: bool) -> Vec<u8> {
    let mut ext: Vec<u8> = Vec::new();
    let mut names: Vec<Vec<u8>> = Vec::new();
    for m in members {
        if m.name.len() > 15 {
            names.push(format!("/{}", ext.len()).into_bytes());
            ext.extend_from_slice(&m.name);
            ext.extend_from_slice(b"/\n");
        } else {
            let mut n = m.name.clone();
            n.push(b'/');
            names.push(n);
        }
    }
    let symbols: Vec<Vec<Vec<u8>>> = if armap {
        members
            .iter()
            .map(|m| {
                Elf::parse(&m.data)
                    .map(|e| e.archive_index_symbols())
                    .unwrap_or_default()
            })
            .collect()
    } else {
        Vec::new()
    };
    let nsyms: usize = symbols.iter().map(Vec::len).sum();
    let strsize: usize = symbols.iter().flatten().map(|s| s.len() + 1).sum();
    let have_map = armap && nsyms > 0;
    let map_size = 4 + 4 * nsyms + strsize;
    let map_padded = map_size + (map_size & 1);
    let ext_padded = ext.len() + (ext.len() & 1);
    let mut base = 8usize;
    if have_map {
        base += 60 + map_padded;
    }
    if !ext.is_empty() {
        base += 60 + ext_padded;
    }
    let mut offsets = Vec::with_capacity(members.len());
    let mut at = base;
    for m in members {
        offsets.push(at);
        at += 60 + m.data.len() + (m.data.len() & 1);
    }
    let mut out = b"!<arch>\n".to_vec();
    if have_map {
        out.extend(header(b"/", "0", "0", "0", "0", map_padded));
        out.extend_from_slice(&(nsyms as u32).to_be_bytes());
        for (i, syms) in symbols.iter().enumerate() {
            for _ in syms {
                out.extend_from_slice(&(offsets[i] as u32).to_be_bytes());
            }
        }
        for s in symbols.iter().flatten() {
            out.extend_from_slice(s);
            out.push(0);
        }
        if map_size & 1 == 1 {
            out.push(0);
        }
    }
    if !ext.is_empty() {
        out.extend(header(b"//", "", "", "", "", ext.len() + (ext.len() & 1)));
        out.extend_from_slice(&ext);
        if ext.len() & 1 == 1 {
            out.push(b'\n');
        }
    }
    for (m, n) in members.iter().zip(&names) {
        out.extend(header(
            n,
            &m.date.to_string(),
            &m.uid.to_string(),
            &m.gid.to_string(),
            &format!("{:o}", m.mode),
            m.data.len(),
        ));
        out.extend_from_slice(&m.data);
        if m.data.len() & 1 == 1 {
            out.push(b'\n');
        }
    }
    out
}

fn write_file(path: &[u8], data: &[u8]) -> Result<(), Errno> {
    let mut f = File::open_with(
        path,
        OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC,
        0o666,
    )?;
    f.write_all(data).map_err(|e| sysabi::Errno::from_io(&e))
}

fn basename(p: &[u8]) -> &[u8] {
    match p.iter().rposition(|&b| b == b'/') {
        Some(i) => &p[i + 1..],
        None => p,
    }
}

fn msg(prog: &str, parts: &[&[u8]]) {
    let mut m = format!("{prog}: ").into_bytes();
    for p in parts {
        m.extend_from_slice(p);
    }
    m.push(b'\n');
    io::eprint(m);
}

/// Lê o arquivo; `Ok(None)` se não existe.
fn load(prog: &str, path: &[u8]) -> Result<Option<(Vec<Member>, bool)>, i32> {
    let data = match io::read_path(path) {
        Ok(d) => d,
        Err(Errno::ENOENT) => return Ok(None),
        Err(e) => {
            msg(prog, &[path, b": ", e.message().as_bytes()]);
            return Err(1);
        }
    };
    match parse_archive(&data) {
        Ok(r) => Ok(Some(r)),
        Err(()) => {
            msg(prog, &[path, b": file format not recognized"]);
            Err(1)
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Pos {
    End,
    After,
    Before,
}

#[derive(Default)]
struct Flags {
    op: Option<u8>,
    pos: Option<Pos>,
    create_quiet: bool,
    verbose: bool,
    truncate: bool,
    preserve_dates: bool,
    only_newer: bool,
    actual: bool,
    count_mode: bool,
    full_path: bool,
    write_map: i32,
    output_dir: Option<Vec<u8>>,
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let prog = io::argv0(args);
    let mut argv = io::args_bytes(args);
    // `ar rcs lib.a x.o`: o primeiro argumento sem `-` ganha um.
    if let Some(first) = argv.get(1) {
        if !first.is_empty() && first[0] != b'-' && first[0] != b'@' {
            let mut v = b"-".to_vec();
            v.extend_from_slice(first);
            argv[1] = v;
        }
    }
    let argv = match expand_response_files(&prog, argv) {
        Ok(a) => a,
        Err(c) => return c,
    };
    if argv.len() < 2 {
        return ar_usage(&prog, false);
    }
    let rest: Vec<Vec<u8>> = argv[1..].to_vec();
    let posix = sys::try_current().is_some_and(|s| s.getenv(b"POSIXLY_CORRECT").is_some());
    let mut g = Getopt::new(&rest, SHORTOPTS, LONGOPTS, posix);
    let mut f = Flags::default();
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&prog)));
                return ar_usage(&prog, false);
            }
        };
        let arg = o.arg.clone().unwrap_or_default();
        match o.id {
            ID_PLUGIN | ID_TARGET | ID_LIBDEPS | ID_THIN => {}
            ID_OUTPUT => f.output_dir = Some(arg),
            id => match u8::try_from(id).unwrap_or(0) {
                c @ (b'd' | b'm' | b'p' | b'q' | b'r' | b't' | b'x') => {
                    if f.op.is_some() {
                        return fatal(&prog, "two different operation options specified");
                    }
                    f.op = Some(c);
                }
                b'h' | b'H' => return ar_usage(&prog, true),
                b'V' => {
                    print_version("ar");
                    return 0;
                }
                b'v' => f.verbose = true,
                b'c' => f.create_quiet = true,
                b'f' => f.truncate = true,
                b'o' => f.preserve_dates = true,
                b'u' => f.only_newer = true,
                b'D' => f.actual = false,
                b'U' => f.actual = true,
                b'a' => f.pos = Some(Pos::After),
                b'b' | b'i' => f.pos = Some(Pos::Before),
                b'N' => f.count_mode = true,
                b'P' => f.full_path = true,
                b's' => f.write_map = 1,
                b'S' => f.write_map = -1,
                b'O' | b'T' | b'M' => {}
                _ => {}
            },
        }
    }
    let operands = g.operands();
    let mut idx = 0usize;
    let mut posname: Option<Vec<u8>> = None;
    let mut count = 0usize;
    if f.pos.is_some() {
        let Some(p) = operands.get(idx) else {
            return ar_usage(&prog, false);
        };
        posname = Some(p.clone());
        idx += 1;
    }
    if f.count_mode {
        let Some(c) = operands.get(idx) else {
            return ar_usage(&prog, false);
        };
        count = std::str::from_utf8(c)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        idx += 1;
    }
    let Some(archive) = operands.get(idx) else {
        return ar_usage(&prog, false);
    };
    let files: Vec<Vec<u8>> = operands[idx + 1..].to_vec();
    if f.only_newer && !f.actual && f.op == Some(b'r') {
        io::eprint(format!(
            "{prog}: `u' modifier ignored since `D' is the default (see `U')\n"
        ));
        f.only_newer = false;
    }
    match f.op {
        None if f.write_map == 1 => ranlib_archive(&prog, archive),
        None => {
            io::eprint(format!("{}: invalid option -- '.'\n", String::from_utf8_lossy(&argv[1])));
            ar_usage(&prog, false)
        }
        Some(op) => {
            let ctx = Ctx {
                prog: &prog,
                f: &f,
                archive,
                files: &files,
                posname: posname.as_deref(),
                count,
            };
            ctx.dispatch(op)
        }
    }
}

fn fatal(prog: &str, text: &str) -> i32 {
    io::eprint(format!("{prog}: {text}\n"));
    1
}

struct Ctx<'a> {
    prog: &'a str,
    f: &'a Flags,
    archive: &'a [u8],
    files: &'a [Vec<u8>],
    posname: Option<&'a [u8]>,
    count: usize,
}

impl Ctx<'_> {
    fn key<'b>(&self, name: &'b [u8]) -> &'b [u8] {
        if self.f.full_path { name } else { basename(name) }
    }

    /// Índice do membro que casa com `name` (a n-ésima instância com `N`).
    fn find(&self, members: &[Member], name: &[u8], from: usize) -> Option<usize> {
        let want = self.key(name);
        let mut seen = 0;
        for (i, m) in members.iter().enumerate().skip(from) {
            if self.key(&m.name) == want {
                seen += 1;
                if self.count == 0 || seen == self.count {
                    return Some(i);
                }
            }
        }
        None
    }

    fn open(&self, creating_ok: bool) -> Result<(Vec<Member>, bool), i32> {
        match load(self.prog, self.archive)? {
            Some(r) => Ok(r),
            None if creating_ok => {
                if !self.f.create_quiet {
                    msg(
                        self.prog,
                        &[b"creating ", self.archive],
                    );
                }
                Ok((Vec::new(), false))
            }
            None => {
                msg(
                    self.prog,
                    &[self.archive, b": ", Errno::ENOENT.message().as_bytes()],
                );
                Err(9)
            }
        }
    }

    fn save(&self, members: &[Member], armap: bool) -> i32 {
        match write_file(self.archive, &serialize(members, armap)) {
            Ok(()) => 0,
            Err(e) => {
                msg(
                    self.prog,
                    &[self.archive, b": ", e.message().as_bytes()],
                );
                1
            }
        }
    }

    fn want_map(&self, had: bool, default_on: bool) -> bool {
        match self.f.write_map {
            1 => true,
            -1 => false,
            _ => had || default_on,
        }
    }

    fn dispatch(&self, op: u8) -> i32 {
        match op {
            b'r' | b'q' => self.add(op),
            b'd' => self.delete(),
            b'm' => self.mv(),
            b'p' | b't' | b'x' => self.read_ops(op),
            _ => 1,
        }
    }

    fn new_member(&self, file: &[u8]) -> Result<Member, i32> {
        let data = match io::read_path(file) {
            Ok(d) => d,
            Err(e) => {
                msg(self.prog, &[file, b": ", e.message().as_bytes()]);
                return Err(1);
            }
        };
        let st = sys::stat(file).ok();
        let mut name = self.key(file).to_vec();
        if self.f.truncate && name.len() > 15 {
            name.truncate(15);
        }
        let (date, uid, gid, mode) = match (&st, self.f.actual) {
            (Some(s), true) => (
                u64::try_from(s.mtime.sec).unwrap_or(0),
                u64::from(s.uid),
                u64::from(s.gid),
                u64::from(s.mode),
            ),
            _ => (0, 0, 0, 0o644),
        };
        Ok(Member {
            name,
            date,
            uid,
            gid,
            mode,
            data,
        })
    }

    fn position(&self, members: &[Member]) -> Result<Option<usize>, i32> {
        let Some(p) = self.posname else {
            return Ok(None);
        };
        match self.find(members, p, 0) {
            Some(i) => Ok(Some(if self.f.pos == Some(Pos::After) { i + 1 } else { i })),
            None => {
                msg(self.prog, &[b"no entry ", p, b" in archive"]);
                Err(1)
            }
        }
    }

    fn add(&self, op: u8) -> i32 {
        let (mut members, had) = match self.open(true) {
            Ok(r) => r,
            Err(c) => return c,
        };
        let mut inserted: Vec<Member> = Vec::new();
        let at = match self.position(&members) {
            Ok(a) => a,
            Err(c) => return c,
        };
        let mut lines: Vec<String> = Vec::new();
        for file in self.files {
            let m = match self.new_member(file) {
                Ok(m) => m,
                Err(c) => return c,
            };
            let existing = if op == b'r' {
                self.find(&members, &m.name, 0)
            } else {
                None
            };
            if let Some(i) = existing {
                if self.f.only_newer && members[i].date >= m.date {
                    continue;
                }
                lines.push(format!("r - {}\n", String::from_utf8_lossy(&m.name)));
                members[i] = m;
            } else {
                lines.push(format!("a - {}\n", String::from_utf8_lossy(&m.name)));
                inserted.push(m);
            }
        }
        match at {
            Some(i) => {
                let tail = members.split_off(i.min(members.len()));
                members.extend(inserted);
                members.extend(tail);
            }
            None => members.extend(inserted),
        }
        let armap = if op == b'q' {
            self.f.write_map == 1
        } else {
            self.want_map(had, true)
        };
        let code = self.save(&members, armap);
        if code == 0 && self.f.verbose {
            let mut out = io::stdout();
            for l in &lines {
                let _ = out.write_all(l.as_bytes());
            }
        }
        code
    }

    fn delete(&self) -> i32 {
        let (mut members, had) = match self.open(false) {
            Ok(r) => r,
            Err(c) => return c,
        };
        let mut lines = Vec::new();
        for file in self.files {
            if let Some(i) = self.find(&members, file, 0) {
                let m = members.remove(i);
                lines.push(format!("d - {}\n", String::from_utf8_lossy(&m.name)));
            }
        }
        let armap = self.want_map(had, false);
        let code = self.save(&members, armap);
        if code == 0 && self.f.verbose {
            let mut out = io::stdout();
            for l in &lines {
                let _ = out.write_all(l.as_bytes());
            }
        }
        code
    }

    fn mv(&self) -> i32 {
        let (mut members, had) = match self.open(false) {
            Ok(r) => r,
            Err(c) => return c,
        };
        let mut moved = Vec::new();
        let mut status = 0;
        for file in self.files {
            match self.find(&members, file, 0) {
                Some(i) => moved.push(members.remove(i)),
                None => {
                    msg(self.prog, &[b"no entry ", file, b" in archive"]);
                    status = 1;
                }
            }
        }
        let at = match self.position(&members) {
            Ok(a) => a,
            Err(c) => return c,
        };
        match at {
            Some(i) => {
                let tail = members.split_off(i.min(members.len()));
                members.extend(moved);
                members.extend(tail);
            }
            None => members.extend(moved),
        }
        let armap = self.want_map(had, false);
        let code = self.save(&members, armap);
        if code != 0 { code } else { status }
    }

    fn read_ops(&self, op: u8) -> i32 {
        let (members, _) = match self.open(false) {
            Ok(r) => r,
            Err(c) => return c,
        };
        let mut status = 0;
        let mut selected: Vec<usize> = Vec::new();
        if self.files.is_empty() {
            selected.extend(0..members.len());
        } else {
            for file in self.files {
                match self.find(&members, file, 0) {
                    Some(i) => selected.push(i),
                    None => {
                        io::eprint([b"no entry ".as_slice(), file, b" in archive\n"].concat());
                    }
                }
            }
        }
        let mut out = io::stdout();
        for i in selected {
            let m = &members[i];
            match op {
                b't' => {
                    if self.f.verbose {
                        let line = format!(
                            "{} {}/{} {:>6} {} ",
                            mode_string(m.mode),
                            m.uid,
                            m.gid,
                            m.data.len(),
                            format_date(m.date)
                        );
                        let _ = out.write_all(line.as_bytes());
                    }
                    let _ = out.write_all(&m.name);
                    let _ = out.write_all(b"\n");
                }
                b'p' => {
                    if self.f.verbose {
                        let _ = out.write_all(b"\n<");
                        let _ = out.write_all(&m.name);
                        let _ = out.write_all(b">\n\n");
                    }
                    let _ = out.write_all(&m.data);
                }
                _ => {
                    if self.f.verbose {
                        let _ = out.write_all(b"x - ");
                        let _ = out.write_all(&m.name);
                        let _ = out.write_all(b"\n");
                    }
                    if let Err(e) = self.extract(m) {
                        msg(self.prog, &[&m.name, b": ", e.message().as_bytes()]);
                        status = 1;
                    }
                }
            }
        }
        status
    }

    fn extract(&self, m: &Member) -> Result<(), Errno> {
        let mut path = Vec::new();
        if let Some(d) = &self.f.output_dir {
            path.extend_from_slice(d);
            path.push(b'/');
        }
        path.extend_from_slice(basename(&m.name));
        let mut f = File::open_with(
            &path,
            OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC,
            0o666,
        )?;
        f.write_all(&m.data).map_err(|e| sysabi::Errno::from_io(&e))?;
        let sysc = sys::current();
        let _ = sysc.fchmod(f.fd(), (m.mode & 0o7777) as u32);
        if self.f.preserve_dates {
            let t = SetTime::At(TimeSpec {
                sec: m.date as i64,
                nsec: 0,
            });
            let _ = sysc.futimens(f.fd(), SetTime::Omit, t);
        }
        Ok(())
    }
}

/// `ar s arquivo` (e `ranlib`): relê e regrava com a tabela de símbolos.
fn ranlib_archive(prog: &str, path: &[u8]) -> i32 {
    match load(prog, path) {
        Ok(Some((members, _))) => match write_file(path, &serialize(&members, true)) {
            Ok(()) => 0,
            Err(e) => {
                msg(prog, &[path, b": ", e.message().as_bytes()]);
                1
            }
        },
        Ok(None) => {
            msg(prog, &[b"'", path, b"': No such file"]);
            1
        }
        Err(c) => c,
    }
}

/// `ranlib`.
pub fn ranlib_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| {
        let prog = io::argv0(args);
        let argv = match expand_response_files(&prog, io::args_bytes(args)) {
            Ok(a) => a,
            Err(c) => return c,
        };
        if argv.len() < 2 {
            return ranlib_usage(&prog, false);
        }
        const RL_SHORT: &str = "DUtvVhH";
        const RL_LONG: &[LongOpt] = &[
            LongOpt::new("help", HasArg::No, 'h' as i32),
            LongOpt::new("plugin", HasArg::Required, ID_PLUGIN),
            LongOpt::new("version", HasArg::No, 'V' as i32),
        ];
        let rest: Vec<Vec<u8>> = argv[1..].to_vec();
        let mut g = Getopt::new(&rest, RL_SHORT, RL_LONG, false);
        let mut bad = false;
        while let Some(r) = g.next_opt() {
            let o = match r {
                Ok(o) => o,
                Err(e) => {
                    io::eprint(format!("{}\n", e.message(&prog)));
                    bad = true;
                    continue;
                }
            };
            match u8::try_from(o.id).unwrap_or(0) {
                b'h' | b'H' => return ranlib_usage(&prog, true),
                b'v' | b'V' => {
                    print_version("ranlib");
                    return 0;
                }
                _ => {}
            }
        }
        let operands = g.operands();
        let Some(archive) = operands.first() else {
            return ranlib_usage(&prog, false);
        };
        let rc = ranlib_archive(&prog, archive);
        if bad { 1 } else { rc }
    })
}

fn mode_string(mode: u64) -> String {
    let bits = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    bits.iter()
        .map(|&(b, c)| if mode & b != 0 { c } else { '-' })
        .collect()
}

/// `%b %e %H:%M %Y` em UTC.
fn format_date(secs: u64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    if month <= 2 {
        year += 1;
    }
    format!(
        "{} {:>2} {:02}:{:02} {}",
        MONTHS[(month - 1) as usize],
        day,
        rem / 3600,
        (rem % 3600) / 60,
        year
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(name: &str, data: &[u8]) -> Member {
        Member {
            name: name.as_bytes().to_vec(),
            date: 0,
            uid: 0,
            gid: 0,
            mode: 0o644,
            data: data.to_vec(),
        }
    }

    #[test]
    fn roundtrip_with_long_names() {
        let ms = vec![
            mem("a.txt", b"abc"),
            mem("a_very_long_member_name.txt", b"hello!"),
        ];
        let bytes = serialize(&ms, false);
        assert!(bytes.starts_with(b"!<arch>\n//"));
        let (back, had) = parse_archive(&bytes).unwrap();
        assert!(!had);
        assert_eq!(back.len(), 2);
        assert_eq!(back[1].name, b"a_very_long_member_name.txt");
        assert_eq!(back[0].data, b"abc");
        assert_eq!(&bytes[8..24], b"//              ");
    }

    #[test]
    fn short_header_layout() {
        let bytes = serialize(&[mem("x", b"12")], false);
        assert_eq!(
            &bytes[8..68],
            b"x/              0           0     0     644     2         `\n"
        );
    }

    #[test]
    fn dates() {
        assert_eq!(format_date(0), "Jan  1 00:00 1970");
        assert_eq!(format_date(951_782_400), "Feb 29 00:00 2000");
        assert_eq!(mode_string(0o644), "rw-r--r--");
    }
}
