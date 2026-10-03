//! Shim de CLI fino (nosso) sobre as crates: `tar t/x`, `gzip -l/-d/-t/-c`, `zcat`, `bzip2`, `xz`,
//! `lzip`, `zstd` e `unzip -l/-Z1/-p/-q`. Serve de evidência do que as crates entregam pro CLI e do que
//! fica com a gente (formatação, mensagens, classificação de erro, extração na árvore).

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use harness::{Candidate, Entry, Invocation, Outcome};

use super::archives::civil;
use super::codecs::{self, Codec, DecodeError, Format};

/// O shim com um backend escolhido por formato.
pub struct ArchiveCli {
    pub label: String,
    pub codecs: BTreeMap<Format, Box<dyn Codec>>,
}

impl ArchiveCli {
    /// Backends padrão (flate2, bzip2, lzma-rust2, structured-zstd), com trocas opcionais.
    pub fn with(label: &str, overrides: Vec<Box<dyn Codec>>) -> ArchiveCli {
        let mut map: BTreeMap<Format, Box<dyn Codec>> = Format::ALL.into_iter().map(|f| (f, codecs::default_for(f))).collect();
        for c in overrides {
            map.insert(c.format(), c);
        }
        ArchiveCli { label: label.to_string(), codecs: map }
    }

    fn codec(&self, f: Format) -> &dyn Codec {
        self.codecs[&f].as_ref()
    }

    fn decode(&self, f: Format, data: &[u8]) -> Result<Vec<u8>, DecodeError> {
        codecs::decode_guarded(self.codec(f), data)
    }
}

impl Candidate for ArchiveCli {
    fn name(&self) -> String {
        format!("archive-cli ({})", self.label)
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        let Some(prog) = inv.program() else {
            return Outcome::unsupported("caso script: o shim só roda argv");
        };
        let args: Vec<String> = inv.args().to_vec();
        match prog {
            "tar" => self.tar(&args, inv),
            "gzip" | "bzip2" | "xz" | "lzip" | "zstd" => self.compressor(prog, &args, inv),
            "zcat" => self.compressor("gzip", &prefixed("-dc", &args), inv),
            "bzcat" => self.compressor("bzip2", &prefixed("-dc", &args), inv),
            "xzcat" => self.compressor("xz", &prefixed("-dc", &args), inv),
            "zstdcat" => self.compressor("zstd", &prefixed("-dc", &args), inv),
            "unzip" => self.unzip(&args, inv),
            other => Outcome::unsupported(format!("programa {other} fora do shim")),
        }
    }
}

fn prefixed(flag: &str, args: &[String]) -> Vec<String> {
    let mut v = vec![flag.to_string()];
    v.extend(args.iter().cloned());
    v
}

fn read_fixture<'a>(inv: &'a Invocation, name: &str) -> Option<&'a [u8]> {
    inv.files.read(&crate::common::relative(name))
}

// --- compressores ---

#[derive(Default)]
struct Flags {
    decompress: bool,
    stdout: bool,
    test: bool,
    keep: bool,
    list: bool,
    verbose: bool,
    files: Vec<String>,
}

fn parse_flags(args: &[String]) -> Result<Flags, String> {
    let mut f = Flags::default();
    for a in args {
        if let Some(short) = a.strip_prefix('-').filter(|s| !s.is_empty() && !s.starts_with('-')) {
            for ch in short.chars() {
                match ch {
                    'd' => f.decompress = true,
                    'c' => f.stdout = true,
                    't' => f.test = true,
                    'k' => f.keep = true,
                    'l' => f.list = true,
                    'v' => f.verbose = true,
                    'n' | 'q' | 'f' => {}
                    other => return Err(format!("flag -{other}")),
                }
            }
        } else if a.starts_with("--") {
            return Err(format!("opção {a}"));
        } else {
            f.files.push(a.clone());
        }
    }
    Ok(f)
}

impl ArchiveCli {
    fn compressor(&self, prog: &str, args: &[String], inv: &Invocation) -> Outcome {
        let flags = match parse_flags(args) {
            Ok(f) => f,
            Err(e) => return Outcome::unsupported(format!("{prog}: {e}")),
        };
        if flags.files.is_empty() {
            return Outcome::unsupported(format!("{prog}: stdin"));
        }
        if flags.list {
            return gzip_list(&flags, inv);
        }
        let mut stdout = Vec::new();
        let mut stderr = String::new();
        let mut exit = 0;
        let mut files = inv.files.clone();
        for name in &flags.files {
            let Some(data) = read_fixture(inv, name) else {
                stderr.push_str(&missing_message(prog, name));
                exit = exit.max(1);
                continue;
            };
            if !(flags.decompress || flags.test) {
                let fmt = format_of(prog, b"");
                let codec = self.codec(fmt);
                let level = codec.bench_level().unwrap_or(fmt.gnu_default_level());
                match codecs::encode_guarded(codec, data, level) {
                    Ok(mut enc) => {
                        if fmt == Format::Gzip && enc.len() > 9 {
                            enc[9] = 3; // SO Unix, como o gzip do GNU (cabeçalho é responsabilidade do CLI)
                        }
                        if flags.stdout {
                            stdout.extend_from_slice(&enc);
                        } else {
                            return Outcome::unsupported("compressão pra arquivo");
                        }
                    }
                    Err(e) => return Outcome::unsupported(format!("encode: {e:#}")),
                }
                continue;
            }
            let fmt = format_of(prog, data);
            match self.decode(fmt, data) {
                Ok(plain) => {
                    if flags.test {
                        continue;
                    }
                    if flags.stdout {
                        stdout.extend_from_slice(&plain);
                    } else {
                        let rel = crate::common::relative(name);
                        let Some(out_name) = strip_suffix(prog, &rel) else {
                            return Outcome::unsupported("sufixo desconhecido");
                        };
                        let mode = match files.get(&rel) {
                            Some(Entry::File { mode, .. }) => *mode,
                            _ => 0o644,
                        };
                        files.insert(&out_name, Entry::file(plain, mode));
                        if !flags.keep {
                            files.entries.remove(&rel);
                        }
                    }
                }
                Err(e) => {
                    let (msg, code) = decode_error_message(prog, name, data, &e);
                    stderr.push_str(&msg);
                    exit = exit.max(code);
                }
            }
        }
        Outcome::exited(stdout, stderr, exit, files)
    }
}

fn format_of(prog: &str, data: &[u8]) -> Format {
    match prog {
        "gzip" => Format::Gzip,
        "bzip2" => Format::Bzip2,
        "lzip" => Format::Lzip,
        "zstd" => Format::Zstd,
        // `xz -d` reconhece .xz e .lzma (byte de propriedades 0x5d no preset padrão) pelo conteúdo.
        _ if data.first() == Some(&0x5d) => Format::Lzma,
        _ => Format::Xz,
    }
}

fn strip_suffix(prog: &str, name: &str) -> Option<String> {
    let sfx: &[&str] = match prog {
        "gzip" => &[".gz", ".z"],
        "bzip2" => &[".bz2"],
        "xz" => &[".xz", ".lzma"],
        "lzip" => &[".lz"],
        _ => &[".zst"],
    };
    sfx.iter().find_map(|s| name.strip_suffix(s).map(str::to_string))
}

fn missing_message(prog: &str, name: &str) -> String {
    match prog {
        "bzip2" => format!("bzip2: Can't open input file {name}: No such file or directory.\n"),
        "zstd" => format!("zstd: can't stat {name} : No such file or directory -- ignored \n"),
        _ => format!("{prog}: {name}: No such file or directory\n"),
    }
}

/// Mensagem e exit code no formato de cada ferramenta GNU. É aqui que a classificação de erro da crate
/// vira texto; quando a crate não distingue, a mensagem sai errada e o caso falha (evidência).
fn decode_error_message(prog: &str, name: &str, data: &[u8], e: &DecodeError) -> (String, i32) {
    match prog {
        "gzip" => {
            let what = if data.len() < 2 || data[0] != 0x1f || data[1] != 0x8b {
                "not in gzip format".to_string()
            } else {
                match e {
                    DecodeError::Truncated(_) => "unexpected end of file".into(),
                    DecodeError::Checksum(_) => "invalid compressed data--crc error".into(),
                    _ => "invalid compressed data--format violated".into(),
                }
            };
            (format!("\ngzip: {name}: {what}\n"), 1)
        }
        "bzip2" => match e {
            DecodeError::Truncated(_) => (
                format!(
                    "bzip2: {name}: file ends unexpectedly\n\nYou can use the `bzip2recover' program to attempt to recover\ndata from undamaged sections of corrupted files.\n\n"
                ),
                2,
            ),
            _ => (format!("bzip2: {name}: data integrity (CRC) error in data\n"), 2),
        },
        "xz" => {
            let what = match e {
                DecodeError::Truncated(_) => "Unexpected end of input",
                DecodeError::Format(_) if codecs::sniff(data).is_none() && data.first() != Some(&0x5d) => {
                    "File format not recognized"
                }
                _ => "Compressed data is corrupt",
            };
            (format!("xz: {name}: {what}\n"), 1)
        }
        "zstd" => {
            let what = match e {
                DecodeError::Truncated(_) => "Read error (39) : premature end ",
                _ => "Decoding error (36) : Data corruption detected ",
            };
            (format!("{name} : {what}\n"), 1)
        }
        _ => (format!("{prog}: {name}: {}\n", e.message()), 1),
    }
}

/// `gzip -l` e `gzip -lv`, com a mesma conta do gzip 1.13 (inclusive a da linha de totais, que usa
/// o tamanho de cabeçalho do último arquivo).
fn gzip_list(flags: &Flags, inv: &Invocation) -> Outcome {
    let mut out = String::new();
    let mut err = String::new();
    let mut exit = 0;
    let (mut total_in, mut total_out, mut last_header) = (0u64, 0u64, 0u64);
    let mut first = true;
    let mut listed = 0;
    for name in &flags.files {
        let Some(data) = read_fixture(inv, name) else {
            err.push_str(&format!("gzip: {name}: No such file or directory\n"));
            exit = 1;
            continue;
        };
        let hdr = match codecs::gzip_header_len(data) {
            Ok(h) if data.len() >= h + 8 => h as u64,
            _ => {
                err.push_str(&format!("\ngzip: {name}: not in gzip format\n"));
                exit = 1;
                continue;
            }
        };
        let n = data.len();
        let crc = u32::from_le_bytes([data[n - 8], data[n - 7], data[n - 6], data[n - 5]]);
        let isize = u32::from_le_bytes([data[n - 4], data[n - 3], data[n - 2], data[n - 1]]) as u64;
        let comp = n as u64;
        let header_bytes = hdr + 8;
        if first {
            if flags.verbose {
                out.push_str("method  crc     date  time  ");
            }
            out.push_str("         compressed        uncompressed  ratio uncompressed_name\n");
            first = false;
        }
        if flags.verbose {
            let mtime = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as u64;
            let (_, m, d, h, mi, _) = civil(mtime);
            out.push_str(&format!("defla {crc:08x} {} {d:2} {h:02}:{mi:02} ", month_abbrev(m)));
        }
        let display = name.strip_suffix(".gz").map(str::to_string).unwrap_or_else(|| name.clone());
        out.push_str(&format!("{comp:>19} {isize:>19} {} {display}\n", ratio(isize as i64 - (comp - header_bytes) as i64, isize)));
        total_in += comp;
        total_out += isize;
        last_header = header_bytes;
        listed += 1;
    }
    if listed > 1 {
        if flags.verbose {
            out.push_str("                            ");
        }
        out.push_str(&format!(
            "{total_in:>19} {total_out:>19} {} (totals)\n",
            ratio(total_out as i64 - (total_in - last_header) as i64, total_out)
        ));
    }
    Outcome::exited(out, err, exit, inv.files.clone())
}

fn ratio(num: i64, den: u64) -> String {
    let r = if den == 0 { 0.0 } else { 100.0 * num as f64 / den as f64 };
    format!("{r:5.1}%")
}

fn month_abbrev(m: u32) -> &'static str {
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(m as usize).saturating_sub(1) % 12]
}

// --- tar ---

struct TarArgs {
    list: bool,
    extract: bool,
    verbose: bool,
    numeric: bool,
    file: Option<String>,
}

fn parse_tar(args: &[String]) -> Result<TarArgs, String> {
    let mut t = TarArgs { list: false, extract: false, verbose: false, numeric: false, file: None };
    let mut want_file = false;
    for (i, a) in args.iter().enumerate() {
        if want_file {
            t.file = Some(a.clone());
            want_file = false;
            continue;
        }
        if a == "--numeric-owner" {
            t.numeric = true;
            continue;
        }
        if a.starts_with("--") {
            return Err(format!("opção {a}"));
        }
        let bundle = if i == 0 { a.trim_start_matches('-') } else if let Some(b) = a.strip_prefix('-') { b } else {
            return Err(format!("membro {a}"));
        };
        for ch in bundle.chars() {
            match ch {
                't' => t.list = true,
                'x' => t.extract = true,
                'v' => t.verbose = true,
                'f' => want_file = true,
                'z' | 'j' | 'J' | 'a' => {}
                other => return Err(format!("flag {other}")),
            }
        }
    }
    Ok(t)
}

impl ArchiveCli {
    fn tar(&self, args: &[String], inv: &Invocation) -> Outcome {
        let t = match parse_tar(args) {
            Ok(t) => t,
            Err(e) => return Outcome::unsupported(format!("tar: {e}")),
        };
        let Some(file) = t.file.clone() else {
            return Outcome::unsupported("tar sem -f");
        };
        let Some(raw) = read_fixture(inv, &file) else {
            return Outcome::exited(
                "",
                format!("tar: {file}: Cannot open: No such file or directory\ntar: Error is not recoverable: exiting now\n"),
                2,
                inv.files.clone(),
            );
        };
        let data = match codecs::sniff(raw) {
            Some(f) => match self.decode(f, raw) {
                Ok(d) => d,
                Err(e) => return Outcome::unsupported(format!("tar: descompressão {}: {e:?}", f.label())),
            },
            None => raw.to_vec(),
        };
        let mut ar = tar::Archive::new(Cursor::new(data));
        let entries = match ar.entries() {
            Ok(e) => e,
            Err(e) => return Outcome::unsupported(format!("tar: {e}")),
        };
        let mut out = String::new();
        let mut files = inv.files.clone();
        let mut ugswidth = 19usize;
        for entry in entries {
            let mut e = match entry {
                Ok(e) => e,
                Err(err) => return Outcome::unsupported(format!("tar: entrada: {err}")),
            };
            let name = String::from_utf8_lossy(&e.path_bytes()).into_owned();
            let h = e.header().clone();
            let ty = h.entry_type();
            let link = e.link_name_bytes().map(|l| String::from_utf8_lossy(&l).into_owned());
            let mut content = Vec::new();
            if let Err(err) = e.read_to_end(&mut content) {
                return Outcome::unsupported(format!("tar: dados: {err}"));
            }
            let is_dir = ty.is_dir() || (ty.is_file() && name.ends_with('/'));
            let mode = h.mode().unwrap_or(0) & 0o7777;
            if t.list {
                if !t.verbose {
                    out.push_str(&quote_name(&name));
                    out.push('\n');
                    continue;
                }
                let tchar = if is_dir {
                    'd'
                } else if ty.is_symlink() {
                    'l'
                } else if ty.is_hard_link() {
                    'h'
                } else {
                    '-'
                };
                let uname = h.username().ok().flatten().filter(|s| !s.is_empty() && !t.numeric).map(str::to_string);
                let gname = h.groupname().ok().flatten().filter(|s| !s.is_empty() && !t.numeric).map(str::to_string);
                let user = uname.unwrap_or_else(|| h.uid().unwrap_or(0).to_string());
                let group = gname.unwrap_or_else(|| h.gid().unwrap_or(0).to_string());
                let size = h.size().unwrap_or(0).to_string();
                // Mesma conta do list.c do GNU tar: "user/group size" com o espaço do meio.
                let pad = user.len() + 1 + group.len() + 1 + size.len();
                ugswidth = ugswidth.max(pad);
                let (y, mo, d, hh, mi, _) = civil(h.mtime().unwrap_or(0));
                out.push_str(&format!(
                    "{} {user}/{group} {size:>w$} {y:04}-{mo:02}-{d:02} {hh:02}:{mi:02} {}",
                    mode_string(tchar, mode),
                    quote_name(&name),
                    w = ugswidth - pad + size.len()
                ));
                if ty.is_symlink() {
                    out.push_str(&format!(" -> {}", quote_name(link.as_deref().unwrap_or(""))));
                } else if ty.is_hard_link() {
                    out.push_str(&format!(" link to {}", quote_name(link.as_deref().unwrap_or(""))));
                }
                out.push('\n');
            } else if t.extract {
                let path = name.trim_end_matches('/').to_string();
                if is_dir {
                    files.insert(&path, Entry::dir(mode));
                } else if ty.is_symlink() {
                    files.insert(&path, Entry::symlink(link.unwrap_or_default()));
                } else if ty.is_hard_link() {
                    let target = link.unwrap_or_default();
                    match files.get(&target).cloned() {
                        Some(entry) => files.insert(&path, entry),
                        None => return Outcome::unsupported(format!("hardlink pra {target} ausente")),
                    }
                } else {
                    files.insert(&path, Entry::file(content, mode));
                }
            }
        }
        Outcome::exited(out, "", 0, files)
    }
}

fn mode_string(t: char, mode: u32) -> String {
    let mut s: Vec<char> = vec![t];
    for (bit, ch) in [(0o400, 'r'), (0o200, 'w'), (0o100, 'x'), (0o40, 'r'), (0o20, 'w'), (0o10, 'x'), (0o4, 'r'), (0o2, 'w'), (0o1, 'x')] {
        s.push(if mode & bit != 0 { ch } else { '-' });
    }
    for (bit, idx, set, unset) in [(0o4000, 3, 's', 'S'), (0o2000, 6, 's', 'S'), (0o1000, 9, 't', 'T')] {
        if mode & bit != 0 {
            s[idx] = if s[idx] == 'x' { set } else { unset };
        }
    }
    s.into_iter().collect()
}

/// Estilo `escape` do quotearg (padrão do GNU tar): barra invertida e controle escapados.
fn quote_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\{:03o}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// --- unzip ---

impl ArchiveCli {
    fn unzip(&self, args: &[String], inv: &Invocation) -> Outcome {
        let mut mode = "extract";
        let mut rest = Vec::new();
        for a in args {
            match a.as_str() {
                "-l" => mode = "list",
                "-Z1" => mode = "names",
                "-p" => mode = "pipe",
                "-q" => {}
                other if other.starts_with('-') => return Outcome::unsupported(format!("unzip {other}")),
                other => rest.push(other.to_string()),
            }
        }
        let Some(zipname) = rest.first().cloned() else {
            return Outcome::unsupported("unzip sem arquivo");
        };
        let members: Vec<String> = rest[1..].to_vec();
        let Some(data) = read_fixture(inv, &zipname) else {
            return Outcome::exited(
                "",
                format!("unzip:  cannot find or open {zipname}, {zipname}.zip or {zipname}.ZIP.\n"),
                9,
                inv.files.clone(),
            );
        };
        let mut ar = match zip::ZipArchive::new(Cursor::new(data)) {
            Ok(a) => a,
            Err(e) => return Outcome::unsupported(format!("unzip: {e}")),
        };
        let mut out = Vec::new();
        let mut files = inv.files.clone();
        let (mut total, mut count) = (0u64, 0usize);
        if mode == "list" {
            out.extend_from_slice(
                format!("Archive:  {zipname}\n  Length      Date    Time    Name\n---------  ---------- -----   ----\n").as_bytes(),
            );
        }
        for i in 0..ar.len() {
            let mut f = match ar.by_index(i) {
                Ok(f) => f,
                Err(e) => return Outcome::unsupported(format!("unzip: {e}")),
            };
            let name = f.name().to_string();
            match mode {
                "list" => {
                    let (y, mo, d, h, mi) = match f.last_modified() {
                        Some(dt) => (dt.year(), dt.month(), dt.day(), dt.hour(), dt.minute()),
                        None => (1980, 1, 1, 0, 0),
                    };
                    out.extend_from_slice(
                        format!("{:>9}  {y:04}-{mo:02}-{d:02} {h:02}:{mi:02}   {name}\n", f.size()).as_bytes(),
                    );
                    total += f.size();
                    count += 1;
                }
                "names" => out.extend_from_slice(format!("{name}\n").as_bytes()),
                "pipe" => {
                    if members.iter().any(|m| m == &name) {
                        let _ = f.read_to_end(&mut out);
                    }
                }
                _ => {
                    let unix = f.unix_mode().unwrap_or(0);
                    let is_dir = f.is_dir();
                    let is_link = f.is_symlink();
                    let mut content = Vec::new();
                    if let Err(e) = f.read_to_end(&mut content) {
                        return Outcome::unsupported(format!("unzip: {e}"));
                    }
                    let path = name.trim_end_matches('/').to_string();
                    // Sem -K o unzip não restaura setuid, setgid nem sticky.
                    let perm = unix & 0o777;
                    if is_dir {
                        files.insert(&path, Entry::dir(if perm == 0 { 0o755 } else { perm }));
                    } else if is_link {
                        files.insert(&path, Entry::symlink(String::from_utf8_lossy(&content).into_owned()));
                    } else {
                        files.insert(&path, Entry::file(content, if perm == 0 { 0o644 } else { perm }));
                    }
                }
            }
        }
        if mode == "list" {
            let plural = if count == 1 { "" } else { "s" };
            out.extend_from_slice(
                format!("---------                     -------\n{total:>9}                     {count} file{plural}\n").as_bytes(),
            );
        }
        Outcome::exited(out, "", 0, files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_string_handles_special_bits() {
        assert_eq!(mode_string('-', 0o4755), "-rwsr-xr-x");
        assert_eq!(mode_string('d', 0o1777), "drwxrwxrwt");
        assert_eq!(mode_string('-', 0o4644), "-rwSr--r--");
        assert_eq!(mode_string('l', 0o777), "lrwxrwxrwx");
    }

    #[test]
    fn ratio_matches_gzip_format() {
        assert_eq!(ratio(-2, 12), "-16.7%");
        assert_eq!(ratio(252, 492), " 51.2%");
    }

    #[test]
    fn tar_args_accept_bundles() {
        let t = parse_tar(&["tvf".into(), "a.tar".into()]).unwrap();
        assert!(t.list && t.verbose && t.file.as_deref() == Some("a.tar"));
        let t = parse_tar(&["--numeric-owner".into(), "-tvf".into(), "a.tar".into()]).unwrap();
        assert!(t.numeric && t.list);
    }
}
