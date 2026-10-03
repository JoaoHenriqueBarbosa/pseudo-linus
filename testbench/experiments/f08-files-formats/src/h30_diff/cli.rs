//! Front-end do `diff` (nosso, igual pra todos os candidatos): opções, leitura da fixture em memória,
//! binários, diretórios, cabeçalhos com data, mensagens de erro e exit codes do GNU diffutils 3.10.
//! O que muda entre candidatos é só o [`Renderer`], que produz o corpo da saída de um par de arquivos.

use harness::{Candidate, Entry, Invocation, Outcome};

use super::engines::{Engine, is_valid_alignment};
use super::gnu_format::{
    build_script, format_context, format_ed, format_normal, format_side_by_side, format_unified, mark_blank_changes,
};
use super::text::{Normalize, intern, split_lines};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Normal,
    Unified(usize),
    Context(usize),
    Ed,
    SideBySide { width: usize, suppress_common: bool },
}

#[derive(Clone, Debug)]
pub struct Opts {
    pub style: Style,
    pub brief: bool,
    pub report_identical: bool,
    pub recursive: bool,
    pub new_file: bool,
    pub text: bool,
    pub norm: Normalize,
    pub ignore_blank: bool,
    pub labels: Vec<String>,
    /// Opções como foram digitadas, pro cabeçalho "diff -ru a/x b/x" de diretórios.
    pub switches: Vec<String>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            style: Style::Normal,
            brief: false,
            report_identical: false,
            recursive: false,
            new_file: false,
            text: false,
            norm: Normalize::default(),
            ignore_blank: false,
            labels: Vec::new(),
            switches: Vec::new(),
        }
    }
}

/// Produz a saída de um par de arquivos de texto que diferem. `None` quando o renderizador não suporta
/// a combinação de opções (o caso conta como "unsupported"). O booleano diz se houve diferença visível
/// (com -B, um par só com linhas em branco mudadas não tem).
pub trait Renderer: Send + Sync {
    fn name(&self) -> String;
    fn krate(&self) -> (&'static str, &'static str);
    fn render(&self, opts: &Opts, a: &[u8], b: &[u8], header: (&str, &str)) -> Option<(Vec<u8>, bool)>;
}

/// Alinhamento de uma biblioteca + formatador GNU nosso.
pub struct GnuRenderer {
    pub engine: Box<dyn Engine>,
}

impl Renderer for GnuRenderer {
    fn name(&self) -> String {
        format!("{} + formatador GNU nosso", self.engine.label())
    }

    fn krate(&self) -> (&'static str, &'static str) {
        self.engine.krate()
    }

    fn render(&self, opts: &Opts, a: &[u8], b: &[u8], header: (&str, &str)) -> Option<(Vec<u8>, bool)> {
        let la = split_lines(a);
        let lb = split_lines(b);
        let i = intern(&la, &lb, &opts.norm);
        let (c0, c1) = self.engine.changes(&i.a, &i.b, &i.reps);
        if !is_valid_alignment(&i.a, &i.b, &c0, &c1) {
            return Some((b"<alinhamento invalido>\n".to_vec(), true));
        }
        let mut script = build_script(&c0, &c1);
        if opts.ignore_blank {
            mark_blank_changes(&mut script, &la, &lb);
        }
        if script.iter().all(|c| c.ignore) {
            return Some((Vec::new(), false));
        }
        let mut out = Vec::new();
        match opts.style {
            Style::Normal => format_normal(&mut out, &script, &la, &lb),
            Style::Unified(n) => {
                out.extend_from_slice(format!("--- {}\n+++ {}\n", header.0, header.1).as_bytes());
                format_unified(&mut out, &script, &la, &lb, n);
            }
            Style::Context(n) => {
                out.extend_from_slice(format!("*** {}\n--- {}\n", header.0, header.1).as_bytes());
                format_context(&mut out, &script, &la, &lb, n);
            }
            Style::Ed => format_ed(&mut out, &script, &lb),
            Style::SideBySide { width, suppress_common } => {
                format_side_by_side(&mut out, &script, &la, &lb, width, suppress_common)
            }
        }
        Some((out, true))
    }
}

pub struct DiffCandidate {
    pub renderer: Box<dyn Renderer>,
}

impl Candidate for DiffCandidate {
    fn name(&self) -> String {
        self.renderer.name()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        if inv.script.is_some() || inv.program() != Some("diff") {
            return Outcome::unsupported("só casos argv de diff");
        }
        let mut run = Run { inv, renderer: self.renderer.as_ref(), out: Vec::new(), err: Vec::new(), unsupported: None };
        let status = run.main();
        if let Some(why) = run.unsupported {
            return Outcome::unsupported(why);
        }
        Outcome::exited(run.out, run.err, status, inv.files.clone())
    }
}

enum Node<'a> {
    File(&'a [u8]),
    Dir,
    Missing,
}

struct Run<'a> {
    inv: &'a Invocation,
    renderer: &'a dyn Renderer,
    out: Vec<u8>,
    err: Vec<u8>,
    unsupported: Option<String>,
}

const TRY_HELP: &str = "diff: Try 'diff --help' for more information.\n";

enum ArgError {
    Usage(String),
    Unsupported(String),
}

#[cfg(test)]
pub fn parse_args(args: &[String]) -> Result<(Opts, Vec<String>), String> {
    match parse_args_inner(args) {
        Ok(x) => Ok(x),
        Err(ArgError::Usage(m)) | Err(ArgError::Unsupported(m)) => Err(m),
    }
}

fn parse_number(s: &str) -> Result<usize, ArgError> {
    s.parse::<usize>().map_err(|_| ArgError::Usage(format!("diff: invalid context length '{s}'\n")))
}

fn parse_args_inner(args: &[String]) -> Result<(Opts, Vec<String>), ArgError> {
    let mut o = Opts::default();
    let mut operands = Vec::new();
    let mut i = 0;
    let mut only_operands = false;
    let mut width = 130usize;
    let mut suppress_common = false;
    let mut side = false;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        if only_operands || arg == "-" || !arg.starts_with('-') {
            operands.push(arg.clone());
            continue;
        }
        if arg == "--" {
            only_operands = true;
            continue;
        }
        o.switches.push(arg.clone());
        if let Some(long) = arg.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let mut take_value = |o: &mut Opts| -> Result<String, ArgError> {
                if let Some(v) = &value {
                    return Ok(v.clone());
                }
                match args.get(i) {
                    Some(v) => {
                        i += 1;
                        o.switches.push(v.clone());
                        Ok(v.clone())
                    }
                    None => Err(ArgError::Usage(format!("diff: option '--{name}' requires an argument\n"))),
                }
            };
            match name {
                "brief" => o.brief = true,
                "report-identical-files" => o.report_identical = true,
                "recursive" => o.recursive = true,
                "new-file" => o.new_file = true,
                "text" => o.text = true,
                "ignore-case" => o.norm.ignore_case = true,
                "ignore-all-space" => o.norm.ignore_all_space = true,
                "ignore-space-change" => o.norm.ignore_space_change = true,
                "ignore-blank-lines" => o.ignore_blank = true,
                "strip-trailing-cr" => o.norm.strip_trailing_cr = true,
                "minimal" => {}
                "normal" => o.style = Style::Normal,
                "ed" => o.style = Style::Ed,
                "side-by-side" => side = true,
                "suppress-common-lines" => suppress_common = true,
                "label" => {
                    let v = take_value(&mut o)?;
                    o.labels.push(v);
                }
                "unified" => o.style = Style::Unified(value.as_deref().map(parse_number).transpose()?.unwrap_or(3)),
                "context" => o.style = Style::Context(value.as_deref().map(parse_number).transpose()?.unwrap_or(3)),
                "width" => width = parse_number(&take_value(&mut o)?)?,
                _ => return Err(ArgError::Usage(format!("diff: unrecognized option '{arg}'\n"))),
            }
            continue;
        }
        // Agrupamento de opções curtas: -ruN, -U5, -U 5.
        let chars: Vec<char> = arg[1..].chars().collect();
        let mut k = 0;
        while k < chars.len() {
            let c = chars[k];
            k += 1;
            let mut arg_value = |o: &mut Opts| -> Result<String, ArgError> {
                if k < chars.len() {
                    let v: String = chars[k..].iter().collect();
                    k = chars.len();
                    Ok(v)
                } else {
                    match args.get(i) {
                        Some(v) => {
                            i += 1;
                            o.switches.push(v.clone());
                            Ok(v.clone())
                        }
                        None => Err(ArgError::Usage(format!("diff: option requires an argument -- '{c}'\n"))),
                    }
                }
            };
            match c {
                'a' => o.text = true,
                'b' => o.norm.ignore_space_change = true,
                'B' => o.ignore_blank = true,
                'c' => o.style = Style::Context(3),
                'd' => {}
                'e' => o.style = Style::Ed,
                'i' => o.norm.ignore_case = true,
                'N' => o.new_file = true,
                'q' => o.brief = true,
                'r' => o.recursive = true,
                's' => o.report_identical = true,
                'u' => o.style = Style::Unified(3),
                'w' => o.norm.ignore_all_space = true,
                'y' => side = true,
                'U' => o.style = Style::Unified(parse_number(&arg_value(&mut o)?)?),
                'C' => o.style = Style::Context(parse_number(&arg_value(&mut o)?)?),
                'W' => width = parse_number(&arg_value(&mut o)?)?,
                't' | 'T' | 'p' | 'F' | 'I' | 'x' | 'X' | 'S' | 'l' | 'n' | 'D' | 'E' | 'Z' | 'H' => {
                    return Err(ArgError::Unsupported(format!("opção -{c} não implementada no front-end")));
                }
                _ => return Err(ArgError::Usage(format!("diff: invalid option -- '{c}'\n"))),
            }
        }
    }
    if side {
        o.style = Style::SideBySide { width, suppress_common };
    }
    Ok((o, operands))
}

impl<'a> Run<'a> {
    fn main(&mut self) -> i32 {
        let (opts, operands) = match parse_args_inner(self.inv.args()) {
            Ok(x) => x,
            Err(ArgError::Usage(msg)) => {
                self.err.extend_from_slice(msg.as_bytes());
                self.err.extend_from_slice(TRY_HELP.as_bytes());
                return 2;
            }
            Err(ArgError::Unsupported(why)) => {
                self.unsupported = Some(why);
                return 2;
            }
        };
        if operands.len() < 2 {
            let after = operands.last().cloned().unwrap_or_else(|| "diff".to_string());
            self.err.extend_from_slice(format!("diff: missing operand after '{after}'\n").as_bytes());
            self.err.extend_from_slice(TRY_HELP.as_bytes());
            return 2;
        }
        if operands.len() > 2 {
            self.err.extend_from_slice(format!("diff: extra operand '{}'\n", operands[2]).as_bytes());
            self.err.extend_from_slice(TRY_HELP.as_bytes());
            return 2;
        }
        let (p0, p1) = (operands[0].clone(), operands[1].clone());
        let n0 = self.node(&p0);
        let n1 = self.node(&p1);
        let mut status = 0;
        for (p, n) in [(&p0, &n0), (&p1, &n1)] {
            if matches!(n, Node::Missing) {
                self.err.extend_from_slice(format!("diff: {p}: No such file or directory\n").as_bytes());
                status = 2;
            }
        }
        if status == 2 {
            return 2;
        }
        match (n0, n1) {
            (Node::Dir, Node::Dir) => self.diff_dirs(&opts, &p0, &p1),
            (Node::File(d0), Node::Dir) => {
                let q1 = join(&p1, basename(&p0));
                match self.node(&q1) {
                    Node::File(d1) => self.compare_files(&opts, &p0, &q1, d0, d1, false),
                    _ => {
                        self.err.extend_from_slice(format!("diff: {q1}: No such file or directory\n").as_bytes());
                        2
                    }
                }
            }
            (Node::Dir, Node::File(d1)) => {
                let q0 = join(&p0, basename(&p1));
                match self.node(&q0) {
                    Node::File(d0) => self.compare_files(&opts, &q0, &p1, d0, d1, false),
                    _ => {
                        self.err.extend_from_slice(format!("diff: {q0}: No such file or directory\n").as_bytes());
                        2
                    }
                }
            }
            (Node::File(d0), Node::File(d1)) => self.compare_files(&opts, &p0, &p1, d0, d1, false),
            _ => 2,
        }
    }

    /// Resolve um caminho do caso na fixture (segue symlinks; "-" é a entrada padrão).
    fn node(&self, path: &str) -> Node<'a> {
        let inv: &'a Invocation = self.inv;
        if path == "-" {
            return Node::File(&inv.stdin);
        }
        if path.starts_with('/') && !path.starts_with(harness::CASE_DIR) {
            return Node::Missing;
        }
        let mut rel = crate::common::relative(path);
        for _ in 0..40 {
            if rel.is_empty() {
                return Node::Dir;
            }
            match inv.files.get(&rel) {
                Some(Entry::File { data: Some(d), .. }) => return Node::File(d.as_slice()),
                Some(Entry::File { .. }) => return Node::Missing,
                Some(Entry::Dir { .. }) => return Node::Dir,
                Some(Entry::Symlink { target }) => {
                    let parent = rel.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or_default();
                    rel = if target.starts_with('/') {
                        crate::common::relative(target)
                    } else {
                        crate::common::relative(&format!("{parent}/{target}"))
                    };
                }
                None => return Node::Missing,
            }
        }
        Node::Missing
    }

    fn children(&self, dir: &str) -> Vec<String> {
        let rel = crate::common::relative(dir);
        let prefix = if rel.is_empty() { String::new() } else { format!("{rel}/") };
        let mut names: Vec<String> = self
            .inv
            .files
            .entries
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix))
            .filter(|rest| !rest.is_empty() && !rest.contains('/'))
            .map(str::to_string)
            .collect();
        names.sort();
        names.dedup();
        names
    }

    fn diff_dirs(&mut self, opts: &Opts, d0: &str, d1: &str) -> i32 {
        let mut names = self.children(d0);
        names.extend(self.children(d1));
        names.sort();
        names.dedup();
        let mut status = 0;
        for name in names {
            let p0 = join(d0, &name);
            let p1 = join(d1, &name);
            let n0 = self.node(&p0);
            let n1 = self.node(&p1);
            let s = match (n0, n1) {
                (Node::Missing, Node::File(d)) if opts.new_file => self.compare_new_file(opts, &p0, &p1, None, Some(d)),
                (Node::File(d), Node::Missing) if opts.new_file => self.compare_new_file(opts, &p0, &p1, Some(d), None),
                (Node::Missing, _) => {
                    self.out.extend_from_slice(format!("Only in {}: {name}\n", trim_slash(d1)).as_bytes());
                    1
                }
                (_, Node::Missing) => {
                    self.out.extend_from_slice(format!("Only in {}: {name}\n", trim_slash(d0)).as_bytes());
                    1
                }
                (Node::Dir, Node::Dir) => {
                    if opts.recursive {
                        self.diff_dirs(opts, &p0, &p1)
                    } else {
                        self.out.extend_from_slice(format!("Common subdirectories: {p0} and {p1}\n").as_bytes());
                        0
                    }
                }
                (Node::File(a), Node::File(b)) => self.compare_files(opts, &p0, &p1, a, b, true),
                (Node::File(_), Node::Dir) => {
                    self.out.extend_from_slice(
                        format!("File {p0} is a regular file while file {p1} is a directory\n").as_bytes(),
                    );
                    1
                }
                (Node::Dir, Node::File(_)) => {
                    self.out.extend_from_slice(
                        format!("File {p0} is a directory while file {p1} is a regular file\n").as_bytes(),
                    );
                    1
                }
            };
            status = status.max(s);
        }
        status
    }

    fn compare_new_file(&mut self, opts: &Opts, p0: &str, p1: &str, a: Option<&[u8]>, b: Option<&[u8]>) -> i32 {
        let t0 = if a.is_some() { harness::FIXTURE_MTIME as i64 } else { 0 };
        let t1 = if b.is_some() { harness::FIXTURE_MTIME as i64 } else { 0 };
        self.compare_files_at(opts, p0, p1, a.unwrap_or(b""), b.unwrap_or(b""), true, (t0, t1))
    }

    fn compare_files(&mut self, opts: &Opts, p0: &str, p1: &str, a: &[u8], b: &[u8], in_dir: bool) -> i32 {
        let t = harness::FIXTURE_MTIME as i64;
        self.compare_files_at(opts, p0, p1, a, b, in_dir, (t, t))
    }

    #[allow(clippy::too_many_arguments)]
    fn compare_files_at(
        &mut self,
        opts: &Opts,
        p0: &str,
        p1: &str,
        a: &[u8],
        b: &[u8],
        in_dir: bool,
        times: (i64, i64),
    ) -> i32 {
        let binary = !opts.text && (has_nul(a) || has_nul(b));
        if a == b {
            if opts.report_identical {
                self.out.extend_from_slice(format!("Files {p0} and {p1} are identical\n").as_bytes());
            }
            return 0;
        }
        if binary {
            let msg = if opts.brief { "Files" } else { "Binary files" };
            self.out.extend_from_slice(format!("{msg} {p0} and {p1} differ\n").as_bytes());
            return 1;
        }
        let la = split_lines(a);
        let lb = split_lines(b);
        let i = intern(&la, &lb, &opts.norm);
        if i.a == i.b {
            if opts.report_identical {
                self.out.extend_from_slice(format!("Files {p0} and {p1} are identical\n").as_bytes());
            }
            return 0;
        }
        if opts.brief {
            self.out.extend_from_slice(format!("Files {p0} and {p1} differ\n").as_bytes());
            return 1;
        }
        let tz = self.inv.full_env().get("TZ").cloned().unwrap_or_default();
        let h0 = match opts.labels.first() {
            Some(l) => l.clone(),
            None => format!("{p0}\t{}", format_mtime(times.0, &tz)),
        };
        let h1 = match opts.labels.get(1) {
            Some(l) => l.clone(),
            None => format!("{p1}\t{}", format_mtime(times.1, &tz)),
        };
        let Some((body, differs)) = self.renderer.render(opts, a, b, (&h0, &h1)) else {
            self.unsupported = Some(format!("{}: estilo {:?} não suportado", self.renderer.name(), opts.style));
            return 2;
        };
        if !differs {
            return 0;
        }
        if in_dir {
            let mut line = String::from("diff");
            for s in &opts.switches {
                line.push(' ');
                line.push_str(s);
            }
            self.out.extend_from_slice(format!("{line} {p0} {p1}\n").as_bytes());
        }
        self.out.extend_from_slice(&body);
        1
    }
}

fn has_nul(data: &[u8]) -> bool {
    data[..data.len().min(1 << 16)].contains(&0)
}

fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') { format!("{dir}{name}") } else { format!("{dir}/{name}") }
}

fn trim_slash(dir: &str) -> &str {
    let t = dir.trim_end_matches('/');
    if t.is_empty() { dir } else { t }
}

fn basename(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path)
}

/// Data do cabeçalho: "2026-01-15 12:00:00.000000000 +0000" no fuso do caso (tzdb embutido do jiff).
pub fn format_mtime(secs: i64, tz_name: &str) -> String {
    let tz = if tz_name.is_empty() || tz_name == "UTC" || tz_name == "UTC0" {
        jiff::tz::TimeZone::UTC
    } else {
        jiff::tz::TimeZoneDatabase::bundled().get(tz_name).unwrap_or(jiff::tz::TimeZone::UTC)
    };
    let ts = jiff::Timestamp::from_second(secs).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    ts.to_zoned(tz).strftime("%Y-%m-%d %H:%M:%S.000000000 %z").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clusters_and_long_options() {
        let args: Vec<String> = ["-ruN", "--label=x", "-U", "1", "a", "b"].iter().map(|s| s.to_string()).collect();
        let (o, ops) = parse_args(&args).ok().unwrap();
        assert!(o.recursive && o.new_file);
        assert_eq!(o.style, Style::Unified(1));
        assert_eq!(o.labels, vec!["x".to_string()]);
        assert_eq!(ops, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(o.switches, vec!["-ruN", "--label=x", "-U", "1"]);
    }

    #[test]
    fn header_time_is_fixture_mtime() {
        assert_eq!(format_mtime(harness::FIXTURE_MTIME as i64, "UTC"), "2026-01-15 12:00:00.000000000 +0000");
        assert_eq!(format_mtime(0, "UTC"), "1970-01-01 00:00:00.000000000 +0000");
    }
}
