//! `file`: front-end nosso (opções, symlink, diretório, vazio, inexistente, alinhamento dos nomes,
//! charset do `-i`) igual pra todos os motores; só muda quem identifica o conteúdo de arquivo regular.
//!
//! Motores: `pure-magic` + `magic-db` (banco pré-compilado embutido), `libmagic-rs` (com as regras
//! embutidas dele e com o mesmo magdir do `magic-db` carregado em texto), e dois detectores só de MIME
//! (`infer`, `file-format`) pra comparar no `--mime-type`.

use std::path::PathBuf;

use harness::{Bytes, Candidate, Entry, Invocation, MemTree, Outcome};

/// O que o motor diz de um arquivo regular não vazio.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detection {
    /// Descrição no formato do `file -b`. `None` quando o motor só sabe MIME.
    pub description: Option<String>,
    /// MIME no formato do `file --mime-type`. `None` quando o motor não informa.
    pub mime: Option<String>,
}

pub trait MagicEngine {
    fn name(&self) -> String;
    /// Identifica um arquivo regular não vazio. `Err` vira caso "unsupported".
    fn identify(&self, data: &[u8], file_name: &str) -> Result<Detection, String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Describe,
    MimeType,
    /// `-i`: "tipo; charset=x".
    Mime,
    MimeEncoding,
}

#[derive(Debug, PartialEq, Eq)]
struct Options {
    brief: bool,
    mode: Mode,
    follow: bool,
    separator: String,
    operands: Vec<String>,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut opts =
        Options { brief: false, mode: Mode::Describe, follow: false, separator: ":".into(), operands: Vec::new() };
    let mut i = 0;
    let mut only_operands = false;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if only_operands || a == "-" || !a.starts_with('-') {
            opts.operands.push(a.clone());
            continue;
        }
        match a.as_str() {
            "--" => only_operands = true,
            "--brief" => opts.brief = true,
            "--mime" => opts.mode = Mode::Mime,
            "--mime-type" => opts.mode = Mode::MimeType,
            "--mime-encoding" => opts.mode = Mode::MimeEncoding,
            "--dereference" => opts.follow = true,
            "--no-dereference" => opts.follow = false,
            "--separator" => {
                opts.separator = args.get(i).cloned().ok_or("--separator sem argumento")?;
                i += 1;
            }
            long if long.starts_with("--") => return Err(format!("opção não suportada: {long}")),
            short => {
                let letters: Vec<char> = short[1..].chars().collect();
                let mut j = 0;
                while j < letters.len() {
                    match letters[j] {
                        'b' => opts.brief = true,
                        'i' => opts.mode = Mode::Mime,
                        'L' => opts.follow = true,
                        'h' => opts.follow = false,
                        'F' => {
                            let rest: String = letters[j + 1..].iter().collect();
                            if rest.is_empty() {
                                opts.separator = args.get(i).cloned().ok_or("-F sem argumento")?;
                                i += 1;
                            } else {
                                opts.separator = rest;
                            }
                            j = letters.len();
                            continue;
                        }
                        other => return Err(format!("opção não suportada: -{other}")),
                    }
                    j += 1;
                }
            }
        }
    }
    Ok(opts)
}

/// O que existe num caminho da fixture.
#[derive(Debug, PartialEq, Eq)]
enum Node<'a> {
    Missing,
    Dir,
    Symlink { target: String, broken: bool },
    Regular(&'a [u8]),
}

fn join_link(base: &str, target: &str) -> String {
    if target.starts_with('/') {
        return crate::common::relative(target);
    }
    let parent = match base.rfind('/') {
        Some(p) => &base[..p],
        None => "",
    };
    let mut parts: Vec<&str> = parent.split('/').filter(|s| !s.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn lookup<'a>(tree: &'a MemTree, path: &str, follow: bool) -> Node<'a> {
    let mut current = crate::common::relative(path);
    for _ in 0..40 {
        match tree.get(&current) {
            None => return Node::Missing,
            Some(Entry::Dir { .. }) => return Node::Dir,
            Some(Entry::File { data, .. }) => {
                return match data {
                    Some(d) => Node::Regular(d.as_slice()),
                    None => Node::Missing,
                };
            }
            Some(Entry::Symlink { target }) => {
                let next = join_link(&current, target);
                if !follow {
                    let broken = matches!(lookup(tree, &next, true), Node::Missing);
                    return Node::Symlink { target: target.clone(), broken };
                }
                current = next;
            }
        }
    }
    Node::Missing
}

/// Charset no estilo do `encoding.c` do libmagic (só os casos que o corpus cobre: us-ascii, utf-8,
/// iso-8859-1, utf-16 com BOM e binary).
pub fn charset(data: &[u8]) -> &'static str {
    if data.starts_with(&[0xff, 0xfe]) {
        return "utf-16le";
    }
    if data.starts_with(&[0xfe, 0xff]) {
        return "utf-16be";
    }
    let text_byte = |b: u8| matches!(b, 0x07..=0x0d | 0x1b | 0x20..=0x7e);
    if data.iter().all(|&b| text_byte(b)) {
        return "us-ascii";
    }
    if std::str::from_utf8(data).is_ok() && data.iter().all(|&b| b >= 0x80 || text_byte(b)) {
        return "utf-8";
    }
    if data.iter().all(|&b| text_byte(b) || b >= 0xa0) {
        return "iso-8859-1";
    }
    "binary"
}

/// Linha de resultado de um operando, sem o nome.
fn classify(engine: &dyn MagicEngine, opts: &Options, display: &str, node: Node<'_>) -> Result<String, String> {
    let fixed = |desc: &str, mime: &str| -> String {
        match opts.mode {
            Mode::Describe => desc.to_string(),
            Mode::MimeType => mime.to_string(),
            Mode::Mime => format!("{mime}; charset=binary"),
            Mode::MimeEncoding => "binary".to_string(),
        }
    };
    Ok(match node {
        Node::Missing => format!("cannot open `{display}' (No such file or directory)"),
        Node::Dir => fixed("directory", "inode/directory"),
        Node::Symlink { target, broken } => {
            let desc = if broken {
                format!("broken symbolic link to {target}")
            } else {
                format!("symbolic link to {target}")
            };
            fixed(&desc, "inode/symlink")
        }
        Node::Regular([]) => fixed("empty", "inode/x-empty"),
        Node::Regular(data) => {
            let det = engine.identify(data, display)?;
            match opts.mode {
                Mode::Describe => det.description.ok_or("motor sem descrição (só MIME)")?,
                Mode::MimeType => det.mime.unwrap_or_else(|| "(sem MIME)".into()),
                Mode::Mime => format!("{}; charset={}", det.mime.unwrap_or_else(|| "(sem MIME)".into()), charset(data)),
                Mode::MimeEncoding => charset(data).to_string(),
            }
        }
    })
}

/// O `file` sobre um motor.
pub struct FileCli<E: MagicEngine> {
    pub engine: E,
}

impl<E: MagicEngine> Candidate for FileCli<E> {
    fn name(&self) -> String {
        self.engine.name()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        if inv.script.is_some() {
            return Outcome::unsupported("caso script");
        }
        let opts = match parse_args(inv.args()) {
            Ok(o) => o,
            Err(e) => return Outcome::unsupported(e),
        };
        let names: Vec<String> =
            opts.operands.iter().map(|o| if o == "-" { "/dev/stdin".to_string() } else { o.clone() }).collect();
        let width = names.iter().map(|n| n.chars().count()).max().unwrap_or(0);
        let mut out = String::new();
        for (operand, name) in opts.operands.iter().zip(&names) {
            let node = if operand == "-" { Node::Regular(&inv.stdin) } else { lookup(&inv.files, operand, opts.follow) };
            let line = match classify(&self.engine, &opts, name, node) {
                Ok(l) => l,
                Err(e) => return Outcome::unsupported(e),
            };
            if opts.brief {
                out.push_str(&line);
            } else {
                let pad = " ".repeat(width - name.chars().count());
                out.push_str(&format!("{name}{}{pad} {line}", opts.separator));
            }
            out.push('\n');
        }
        Outcome::exited(Bytes::from(out), Bytes::default(), 0, inv.files.clone())
    }
}

fn extension(name: &str) -> Option<&str> {
    let base = name.rsplit('/').next()?;
    let (stem, ext) = base.rsplit_once('.')?;
    (!stem.is_empty()).then_some(ext)
}

/// `pure-magic` com o banco do `magic-db`.
pub struct PureMagic {
    db: Result<pure_magic::MagicDb, String>,
    /// `first_magic` (ordem de força, como o libmagic) ou `best_magic`.
    best: bool,
}

impl PureMagic {
    pub fn new(best: bool) -> PureMagic {
        PureMagic { db: magic_db::load().map_err(|e| format!("magic_db::load: {e}")), best }
    }
}

impl MagicEngine for PureMagic {
    fn name(&self) -> String {
        let api = if self.best { "best_magic" } else { "first_magic" };
        format!("pure-magic 0.4.1 + magic-db 0.6.0 ({api})")
    }

    fn identify(&self, data: &[u8], file_name: &str) -> Result<Detection, String> {
        let db = self.db.as_ref().map_err(Clone::clone)?;
        let ext = extension(file_name);
        let m = if self.best { db.best_magic_slice(data, ext) } else { db.first_magic_slice(data, ext) }
            .map_err(|e| format!("pure-magic: {e}"))?;
        Ok(Detection { description: Some(m.message()), mime: Some(m.mime_type().to_string()) })
    }
}

/// De onde o `libmagic-rs` tira as regras.
pub enum LibmagicRules {
    Builtin,
    /// Diretório de magic em texto (o `src/magdir` do `magic-db`).
    Dir(PathBuf),
}

pub struct LibmagicRs {
    db: Result<libmagic_rs::MagicDatabase, String>,
    label: &'static str,
}

impl LibmagicRs {
    pub fn new(rules: LibmagicRules) -> LibmagicRs {
        let config = libmagic_rs::EvaluationConfig::default().with_mime_types(true);
        let (db, label) = match rules {
            LibmagicRules::Builtin => {
                (libmagic_rs::MagicDatabase::with_builtin_rules_and_config(config), "regras embutidas")
            }
            LibmagicRules::Dir(dir) => {
                (libmagic_rs::MagicDatabase::load_from_file_with_config(dir, config), "magdir do magic-db")
            }
        };
        LibmagicRs { db: db.map_err(|e| format!("libmagic-rs: {e}")), label }
    }

    pub fn load_error(&self) -> Option<&str> {
        self.db.as_ref().err().map(String::as_str)
    }
}

impl MagicEngine for LibmagicRs {
    fn name(&self) -> String {
        format!("libmagic-rs 0.12.6 ({})", self.label)
    }

    fn identify(&self, data: &[u8], _file_name: &str) -> Result<Detection, String> {
        let db = self.db.as_ref().map_err(Clone::clone)?;
        let r = db.evaluate_buffer(data).map_err(|e| format!("libmagic-rs: {e}"))?;
        Ok(Detection { description: Some(r.description), mime: r.mime_type })
    }
}

/// Classe de texto no estilo do `encoding.c` + `ascmagic.c` do libmagic: o "code" que entra antes de
/// "text" na descrição.
pub fn text_code(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0xff, 0xfe]) {
        return Some("Unicode text, UTF-16, little-endian");
    }
    if data.starts_with(&[0xfe, 0xff]) {
        return Some("Unicode text, UTF-16, big-endian");
    }
    let bom = data.starts_with(&[0xef, 0xbb, 0xbf]);
    match charset(data) {
        "us-ascii" => Some("ASCII"),
        "utf-8" if bom => Some("Unicode text, UTF-8 (with BOM)"),
        "utf-8" => Some("Unicode text, UTF-8"),
        "iso-8859-1" => Some("ISO-8859"),
        _ => None,
    }
}

/// Texto "decodificado" pra varredura de linhas (UTF-16 vira as unidades de 16 bits).
fn scan_units(data: &[u8]) -> Vec<u32> {
    if data.starts_with(&[0xff, 0xfe]) || data.starts_with(&[0xfe, 0xff]) {
        let le = data[0] == 0xff;
        return data[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| if le { u16::from_le_bytes(c) } else { u16::from_be_bytes(c) } as u32)
            .collect();
    }
    data.iter().map(|&b| b as u32).collect()
}

/// Sufixos do `ascmagic.c`: linhas longas, terminadores, escapes e overstriking.
pub fn text_suffixes(data: &[u8]) -> String {
    const MAXLINELEN: usize = 300;
    let units = scan_units(data);
    let (mut n_crlf, mut n_cr, mut n_lf) = (0usize, 0usize, 0usize);
    let mut seen_cr = false;
    // Igual ao laço do ascmagic.c: o CR final não conta, e o comprimento é medido desde o último fim
    // de linha (começa em "-1").
    let mut last_line_end: isize = -1;
    let mut longest = 0usize;
    let mut escapes = false;
    let mut backspace = false;
    for (i, &u) in units.iter().enumerate() {
        let i = i as isize;
        if u == u32::from(b'\n') {
            if seen_cr {
                n_crlf += 1;
            } else {
                n_lf += 1;
            }
            last_line_end = i;
        } else if seen_cr {
            n_cr += 1;
        }
        seen_cr = u == u32::from(b'\r');
        if seen_cr {
            last_line_end = i;
        }
        let ll = (i - last_line_end) as usize;
        if ll > MAXLINELEN {
            longest = longest.max(ll);
        }
        escapes |= u == 0x1b;
        backspace |= u == 0x08;
    }
    let mut out = String::new();
    if longest > MAXLINELEN {
        out.push_str(&format!(", with very long lines ({longest})"));
    }
    if n_crlf == 0 && n_cr == 0 && n_lf == 0 {
        out.push_str(", with no line terminators");
    } else if n_crlf != 0 || n_cr != 0 {
        let mut kinds = Vec::new();
        if n_crlf != 0 {
            kinds.push("CRLF");
        }
        if n_cr != 0 {
            kinds.push("CR");
        }
        if n_lf != 0 {
            kinds.push("LF");
        }
        out.push_str(&format!(", with {} line terminators", kinds.join(", ")));
    }
    if escapes {
        out.push_str(", with escape sequences");
    }
    if backspace {
        out.push_str(", with overstriking");
    }
    out
}

/// Descrições que os motores dão pra texto sem regra específica (o "default" deles).
const DEFAULT_TEXT: &[&str] = &["ASCII text", "UTF-8 text", "Unknown text", "UTF-8 Unicode text", "data", "text"];

/// Camada nossa por cima de qualquer motor: reescreve a parte de texto da descrição como o
/// `ascmagic.c` do libmagic ("S text executable" vira "S, ASCII text executable", com os sufixos de
/// terminador e de linha longa) e dá `text/plain` pra texto que o motor chamou de binário.
pub struct WithAscmagic<E: MagicEngine>(pub E);

impl<E: MagicEngine> MagicEngine for WithAscmagic<E> {
    fn name(&self) -> String {
        format!("{} + ascmagic nosso", self.0.name())
    }

    fn identify(&self, data: &[u8], file_name: &str) -> Result<Detection, String> {
        let mut det = self.0.identify(data, file_name)?;
        let Some(code) = text_code(data) else { return Ok(det) };
        let suffixes = text_suffixes(data);
        let generic_mime = matches!(det.mime.as_deref(), None | Some("application/octet-stream"));
        if let Some(desc) = det.description.take() {
            let already_coded = ["ASCII text", "UTF-8 text", "Unicode text", "ISO-8859 text"].iter().any(|c| desc.contains(c))
                && !DEFAULT_TEXT.contains(&desc.as_str());
            let rewritten = if already_coded {
                desc
            } else if DEFAULT_TEXT.contains(&desc.as_str()) {
                format!("{code} text{suffixes}")
            } else if let Some(stem) = desc.strip_suffix(" text") {
                format!("{stem}, {code} text{suffixes}")
            } else if let Some(stem) = desc.strip_suffix(" text executable") {
                format!("{stem}, {code} text executable{suffixes}")
            } else {
                desc
            };
            det.description = Some(rewritten);
        }
        if generic_mime {
            det.mime = Some("text/plain".into());
        }
        Ok(det)
    }
}

/// Fallback de MIME do nosso front-end pra detectores que não classificam texto.
fn text_or_binary(data: &[u8]) -> &'static str {
    if charset(data) == "binary" { "application/octet-stream" } else { "text/plain" }
}

/// `infer`: assinaturas de bytes, só MIME.
pub struct Infer;

impl MagicEngine for Infer {
    fn name(&self) -> String {
        "infer 0.22 (só MIME)".into()
    }

    fn identify(&self, data: &[u8], _file_name: &str) -> Result<Detection, String> {
        let mime = infer::get(data).map(|t| t.mime_type().to_string()).unwrap_or_else(|| text_or_binary(data).into());
        Ok(Detection { description: None, mime: Some(mime) })
    }
}

/// `file-format`: assinaturas e leitores de contêiner, só MIME.
pub struct FileFormat;

impl MagicEngine for FileFormat {
    fn name(&self) -> String {
        "file-format 0.29 (só MIME)".into()
    }

    fn identify(&self, data: &[u8], _file_name: &str) -> Result<Detection, String> {
        let fmt = file_format::FileFormat::from_bytes(data);
        let mime = match fmt {
            file_format::FileFormat::ArbitraryBinaryData => text_or_binary(data).to_string(),
            other => other.media_type().to_string(),
        };
        Ok(Detection { description: None, mime: Some(mime) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed;
    impl MagicEngine for Fixed {
        fn name(&self) -> String {
            "fixo".into()
        }
        fn identify(&self, _data: &[u8], _file_name: &str) -> Result<Detection, String> {
            Ok(Detection { description: Some("ASCII text".into()), mime: Some("text/plain".into()) })
        }
    }

    fn inv(args: &[&str], files: MemTree) -> Invocation {
        Invocation {
            case_id: "t".into(),
            argv: std::iter::once("file").chain(args.iter().copied()).map(String::from).collect(),
            script: None,
            stdin: b"hi\n".to_vec(),
            files,
            env: Default::default(),
            faketime: None,
        }
    }

    fn tree() -> MemTree {
        let mut t = MemTree::new();
        t.insert("a.txt", Entry::file("hello\n", 0o644));
        t.insert("sub", Entry::dir(0o755));
        t.insert("link", Entry::symlink("a.txt"));
        t.insert("dangling", Entry::symlink("nowhere"));
        t.insert("empty", Entry::file("", 0o644));
        t
    }

    #[test]
    fn parses_clusters_and_separator() {
        let args: Vec<String> = ["-bL", "-F", " =>", "--mime-type", "x"].iter().map(|s| s.to_string()).collect();
        let o = parse_args(&args).unwrap();
        assert!(o.brief && o.follow);
        assert_eq!(o.separator, " =>");
        assert_eq!(o.mode, Mode::MimeType);
        assert_eq!(o.operands, vec!["x"]);
        assert!(parse_args(&["-z".to_string()]).is_err());
    }

    #[test]
    fn aligns_names_like_gnu() {
        let cli = FileCli { engine: Fixed };
        let out = cli.run(&inv(&["a.txt", "sub", "missing"], tree()));
        assert_eq!(
            String::from_utf8(out.stdout.0).unwrap(),
            "a.txt:   ASCII text\nsub:     directory\nmissing: cannot open `missing' (No such file or directory)\n"
        );
    }

    #[test]
    fn symlinks_with_and_without_follow() {
        let cli = FileCli { engine: Fixed };
        let plain = cli.run(&inv(&["link", "dangling"], tree()));
        assert_eq!(
            String::from_utf8(plain.stdout.0).unwrap(),
            "link:     symbolic link to a.txt\ndangling: broken symbolic link to nowhere\n"
        );
        let follow = cli.run(&inv(&["-L", "link", "-i", "empty"], tree()));
        assert_eq!(
            String::from_utf8(follow.stdout.0).unwrap(),
            "link:  text/plain; charset=us-ascii\nempty: inode/x-empty; charset=binary\n"
        );
    }

    #[test]
    fn charset_classes() {
        assert_eq!(charset(b"plain\n"), "us-ascii");
        assert_eq!(charset("olá\n".as_bytes()), "utf-8");
        assert_eq!(charset(b"caf\xe9\n"), "iso-8859-1");
        assert_eq!(charset(b"\xff\xfeh\x00"), "utf-16le");
        assert_eq!(charset(b"\x00\x01\x02"), "binary");
    }

    #[test]
    fn ascmagic_suffixes_match_libmagic_rules() {
        assert_eq!(text_suffixes(b"a\nb\n"), "");
        assert_eq!(text_suffixes(b"a\r\nb\r\n"), ", with CRLF line terminators");
        assert_eq!(text_suffixes(b"a\rb\r"), ", with CR line terminators");
        assert_eq!(text_suffixes(b"a\r\nb\n"), ", with CRLF, LF line terminators");
        assert_eq!(text_suffixes(b"no newline"), ", with no line terminators");
        let long = format!("{}\n", "a".repeat(361));
        assert_eq!(text_suffixes(long.as_bytes()), ", with very long lines (361)");
        assert_eq!(text_suffixes(b"\x1b[1m\n"), ", with escape sequences");
    }

    struct Raw(&'static str, &'static str);
    impl MagicEngine for Raw {
        fn name(&self) -> String {
            "cru".into()
        }
        fn identify(&self, _data: &[u8], _file_name: &str) -> Result<Detection, String> {
            Ok(Detection { description: Some(self.0.into()), mime: Some(self.1.into()) })
        }
    }

    #[test]
    fn ascmagic_layer_rewrites_text_descriptions() {
        let e = WithAscmagic(Raw("Bourne-Again shell script text executable", "text/x-shellscript"));
        let d = e.identify(b"#!/bin/bash\r\necho\r\n", "x").unwrap();
        assert_eq!(
            d.description.unwrap(),
            "Bourne-Again shell script, ASCII text executable, with CRLF line terminators"
        );
        let e = WithAscmagic(Raw("data", "application/octet-stream"));
        let d = e.identify(b"caf\xe9\n", "x").unwrap();
        assert_eq!(d.description.unwrap(), "ISO-8859 text");
        assert_eq!(d.mime.unwrap(), "text/plain");
        let e = WithAscmagic(Raw("CSV ASCII text", "text/csv"));
        assert_eq!(e.identify(b"a,b\n1,2\n", "x").unwrap().description.unwrap(), "CSV ASCII text");
        let e = WithAscmagic(Raw("JSON text data", "application/json"));
        assert_eq!(e.identify(b"{}\n", "x").unwrap().description.unwrap(), "JSON text data");
        let e = WithAscmagic(Raw("PNG image data", "image/png"));
        assert_eq!(e.identify(b"\x89PNG\x00", "x").unwrap().description.unwrap(), "PNG image data");
    }

    #[test]
    fn link_paths_resolve_relative_to_parent() {
        assert_eq!(join_link("d/link", "../x"), "x");
        assert_eq!(join_link("d/link", "y"), "d/y");
        assert_eq!(join_link("link", "/work/case/z"), "z");
    }
}
