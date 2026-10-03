//! Mini-shell pros casos `script` do corpus de sqlite e git.
//!
//! Não é o shell do pseudo-linus (isso é o F15): é só o bastante pra rodar, sobre um `MemTree`, os
//! encadeamentos que agentes escrevem em volta de `sqlite3` e `git`: `;`, `&&`, `||`, `|`, `<`, `>`,
//! `>>`, aspas simples e duplas, `$?`, `$VAR`, `VAR=$(cmd)` e os utilitários `echo`, `printf`, `cat`,
//! `rm`, `mkdir` e `ls`. Os programas de verdade (`sqlite3`, `git`) vêm de quem chama, via [`Programs`].

use std::collections::BTreeMap;

use harness::{Entry, MemTree};

/// Contexto de execução de um programa: o FS do caso, o ambiente e os fluxos padrão.
pub struct Ctx<'a> {
    pub fs: &'a mut MemTree,
    pub env: &'a BTreeMap<String, String>,
    pub stdin: &'a [u8],
    pub stdout: &'a mut Vec<u8>,
    pub stderr: &'a mut Vec<u8>,
}

/// Os programas que o shell sabe despachar além dos utilitários embutidos.
pub trait Programs {
    /// Roda `argv` e devolve o status de saída, ou `None` se o programa não existe.
    fn run(&mut self, argv: &[String], ctx: &mut Ctx<'_>) -> Option<i32>;
}

/// Resultado de um script.
#[derive(Debug, Default)]
pub struct ScriptOutcome {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status: i32,
}

/// Roda `script` sobre `fs`.
pub fn run_script(
    script: &str,
    fs: &mut MemTree,
    env: &BTreeMap<String, String>,
    stdin: &[u8],
    programs: &mut dyn Programs,
) -> Result<ScriptOutcome, String> {
    let list = parse(script)?;
    let mut shell = Shell { vars: env.clone(), status: 0, programs };
    let mut out = ScriptOutcome::default();
    shell.exec_list(&list, fs, stdin, &mut out.stdout, &mut out.stderr);
    out.status = shell.status;
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Part {
    Lit(String, bool),
    Var(String, bool),
    Status(bool),
    Subst(String, bool),
}

type Word = Vec<Part>;

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Word(Word),
    Seq,
    And,
    Or,
    Pipe,
    RedirOut,
    RedirAppend,
    RedirIn,
}

#[derive(Clone, Debug, Default)]
struct Command {
    assigns: Vec<(String, Word)>,
    words: Vec<Word>,
    /// (operador, alvo)
    redirs: Vec<(Tok, Word)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Conn {
    Seq,
    And,
    Or,
}

#[derive(Clone, Debug)]
struct List {
    /// Cada item: conector antes dele (o primeiro é `Seq`) e o pipeline.
    items: Vec<(Conn, Vec<Command>)>,
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    let mut word: Word = Vec::new();
    let mut in_word = false;
    let flush = |word: &mut Word, in_word: &mut bool, toks: &mut Vec<Tok>| {
        if *in_word {
            toks.push(Tok::Word(std::mem::take(word)));
            *in_word = false;
        }
    };
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' => {
                flush(&mut word, &mut in_word, &mut toks);
                i += 1;
            }
            '\n' | ';' => {
                flush(&mut word, &mut in_word, &mut toks);
                toks.push(Tok::Seq);
                i += 1;
            }
            '#' if !in_word => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '&' if chars.get(i + 1) == Some(&'&') => {
                flush(&mut word, &mut in_word, &mut toks);
                toks.push(Tok::And);
                i += 2;
            }
            '|' if chars.get(i + 1) == Some(&'|') => {
                flush(&mut word, &mut in_word, &mut toks);
                toks.push(Tok::Or);
                i += 2;
            }
            '|' => {
                flush(&mut word, &mut in_word, &mut toks);
                toks.push(Tok::Pipe);
                i += 1;
            }
            '>' if chars.get(i + 1) == Some(&'>') => {
                flush(&mut word, &mut in_word, &mut toks);
                toks.push(Tok::RedirAppend);
                i += 2;
            }
            '>' => {
                flush(&mut word, &mut in_word, &mut toks);
                toks.push(Tok::RedirOut);
                i += 1;
            }
            '<' => {
                flush(&mut word, &mut in_word, &mut toks);
                toks.push(Tok::RedirIn);
                i += 1;
            }
            '\'' => {
                let end = chars[i + 1..].iter().position(|&x| x == '\'').ok_or("aspa simples sem fechamento")?;
                word.push(Part::Lit(chars[i + 1..i + 1 + end].iter().collect(), true));
                in_word = true;
                i += end + 2;
            }
            '"' => {
                i += 1;
                let mut lit = String::new();
                loop {
                    let Some(&c) = chars.get(i) else { return Err("aspa dupla sem fechamento".into()) };
                    match c {
                        '"' => {
                            i += 1;
                            break;
                        }
                        '\\' if matches!(chars.get(i + 1), Some('"' | '\\' | '$' | '`')) => {
                            lit.push(chars[i + 1]);
                            i += 2;
                        }
                        '$' => {
                            if !lit.is_empty() {
                                word.push(Part::Lit(std::mem::take(&mut lit), true));
                            }
                            let (part, used) = lex_dollar(&chars[i..], true)?;
                            word.push(part);
                            i += used;
                        }
                        _ => {
                            lit.push(c);
                            i += 1;
                        }
                    }
                }
                word.push(Part::Lit(lit, true));
                in_word = true;
            }
            '$' => {
                let (part, used) = lex_dollar(&chars[i..], false)?;
                word.push(part);
                in_word = true;
                i += used;
            }
            '\\' => {
                if let Some(&n) = chars.get(i + 1)
                    && n != '\n'
                {
                    word.push(Part::Lit(n.to_string(), true));
                    in_word = true;
                }
                i += 2;
            }
            _ => {
                match word.last_mut() {
                    Some(Part::Lit(s, false)) => s.push(c),
                    _ => word.push(Part::Lit(c.to_string(), false)),
                }
                in_word = true;
                i += 1;
            }
        }
    }
    flush(&mut word, &mut in_word, &mut toks);
    Ok(toks)
}

/// Lê uma expansão começando em `$`; devolve a parte e quantos caracteres consumiu.
fn lex_dollar(chars: &[char], quoted: bool) -> Result<(Part, usize), String> {
    match chars.get(1) {
        Some('?') => Ok((Part::Status(quoted), 2)),
        Some('(') => {
            // Acha o parêntese que fecha, respeitando aspas e aninhamento.
            let mut depth = 0;
            let mut j = 1;
            let mut in_single = false;
            let mut in_double = false;
            while j < chars.len() {
                let c = chars[j];
                if in_single {
                    if c == '\'' {
                        in_single = false;
                    }
                } else if in_double {
                    if c == '"' {
                        in_double = false;
                    }
                } else if c == '\'' {
                    in_single = true;
                } else if c == '"' {
                    in_double = true;
                } else if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    depth -= 1;
                    if depth == 0 {
                        let inner: String = chars[2..j].iter().collect();
                        return Ok((Part::Subst(inner, quoted), j + 1));
                    }
                }
                j += 1;
            }
            Err("$( sem fechamento".into())
        }
        Some('{') => {
            let end = chars.iter().position(|&c| c == '}').ok_or("${ sem fechamento")?;
            Ok((Part::Var(chars[2..end].iter().collect(), quoted), end + 1))
        }
        Some(c) if c.is_ascii_alphabetic() || *c == '_' => {
            let mut j = 1;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            Ok((Part::Var(chars[1..j].iter().collect(), quoted), j))
        }
        _ => Ok((Part::Lit("$".into(), quoted), 1)),
    }
}

fn parse(src: &str) -> Result<List, String> {
    let toks = lex(src)?;
    let mut items: Vec<(Conn, Vec<Command>)> = Vec::new();
    let mut conn = Conn::Seq;
    let mut pipeline: Vec<Command> = Vec::new();
    let mut cmd = Command::default();
    let mut i = 0;
    let finish_cmd = |cmd: &mut Command, pipeline: &mut Vec<Command>| {
        if !cmd.words.is_empty() || !cmd.assigns.is_empty() || !cmd.redirs.is_empty() {
            pipeline.push(std::mem::take(cmd));
        }
    };
    while i < toks.len() {
        match &toks[i] {
            Tok::Word(w) => {
                if cmd.words.is_empty()
                    && let Some(Part::Lit(first, false)) = w.first()
                    && let Some(eq) = first.find('=')
                    && eq > 0
                    && first[..eq].chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    let mut value: Word = Vec::new();
                    let rest = &first[eq + 1..];
                    if !rest.is_empty() {
                        value.push(Part::Lit(rest.to_string(), false));
                    }
                    value.extend(w[1..].iter().cloned());
                    cmd.assigns.push((first[..eq].to_string(), value));
                    i += 1;
                    continue;
                }
                cmd.words.push(w.clone());
            }
            op @ (Tok::RedirOut | Tok::RedirAppend | Tok::RedirIn) => {
                let Some(Tok::Word(target)) = toks.get(i + 1) else { return Err("redirecionamento sem alvo".into()) };
                cmd.redirs.push((op.clone(), target.clone()));
                i += 1;
            }
            Tok::Pipe => finish_cmd(&mut cmd, &mut pipeline),
            Tok::Seq | Tok::And | Tok::Or => {
                finish_cmd(&mut cmd, &mut pipeline);
                if !pipeline.is_empty() {
                    items.push((conn, std::mem::take(&mut pipeline)));
                }
                conn = match &toks[i] {
                    Tok::And => Conn::And,
                    Tok::Or => Conn::Or,
                    _ => Conn::Seq,
                };
            }
        }
        i += 1;
    }
    finish_cmd(&mut cmd, &mut pipeline);
    if !pipeline.is_empty() {
        items.push((conn, pipeline));
    }
    Ok(List { items })
}

// ---------------------------------------------------------------------------------------------
// Execução
// ---------------------------------------------------------------------------------------------

struct Shell<'p> {
    vars: BTreeMap<String, String>,
    status: i32,
    programs: &'p mut dyn Programs,
}

impl Shell<'_> {
    fn exec_list(&mut self, list: &List, fs: &mut MemTree, stdin: &[u8], out: &mut Vec<u8>, err: &mut Vec<u8>) {
        for (conn, pipeline) in &list.items {
            let run = match conn {
                Conn::Seq => true,
                Conn::And => self.status == 0,
                Conn::Or => self.status != 0,
            };
            if run {
                self.status = self.exec_pipeline(pipeline, fs, stdin, out, err);
            }
        }
    }

    fn exec_pipeline(
        &mut self,
        pipeline: &[Command],
        fs: &mut MemTree,
        stdin: &[u8],
        out: &mut Vec<u8>,
        err: &mut Vec<u8>,
    ) -> i32 {
        let mut input = stdin.to_vec();
        let mut status = 0;
        for (idx, cmd) in pipeline.iter().enumerate() {
            let last = idx + 1 == pipeline.len();
            let mut captured = Vec::new();
            status = self.exec_command(cmd, fs, &input, &mut captured, err);
            if last {
                out.extend_from_slice(&captured);
            } else {
                input = captured;
            }
        }
        status
    }

    fn expand_word(&mut self, word: &Word, fs: &mut MemTree, err: &mut Vec<u8>) -> Vec<String> {
        let mut fields: Vec<String> = Vec::new();
        let mut cur: Option<String> = None;
        for part in word {
            let (text, quoted) = match part {
                Part::Lit(s, q) => (s.clone(), *q || true),
                Part::Var(name, q) => (self.vars.get(name).cloned().unwrap_or_default(), *q),
                Part::Status(q) => (self.status.to_string(), *q),
                Part::Subst(script, q) => {
                    let mut sub_out = Vec::new();
                    let saved = self.status;
                    match parse(script) {
                        Ok(list) => self.exec_list(&list, fs, &[], &mut sub_out, err),
                        Err(e) => err.extend_from_slice(format!("bash: {e}\n").as_bytes()),
                    }
                    let _ = saved;
                    let mut text = String::from_utf8_lossy(&sub_out).into_owned();
                    while text.ends_with('\n') {
                        text.pop();
                    }
                    (text, *q)
                }
            };
            if quoted || matches!(part, Part::Lit(..)) {
                cur.get_or_insert_with(String::new).push_str(&text);
            } else {
                // Expansão sem aspas: quebra em campos por espaço em branco.
                let starts_ws = text.starts_with(char::is_whitespace);
                let ends_ws = text.ends_with(char::is_whitespace);
                let pieces: Vec<&str> = text.split_whitespace().collect();
                if pieces.is_empty() {
                    if starts_ws && let Some(c) = cur.take() {
                        fields.push(c);
                    }
                    continue;
                }
                for (k, piece) in pieces.iter().enumerate() {
                    if k == 0 && !starts_ws {
                        cur.get_or_insert_with(String::new).push_str(piece);
                    } else {
                        if let Some(c) = cur.take() {
                            fields.push(c);
                        }
                        cur = Some(piece.to_string());
                    }
                }
                if ends_ws && let Some(c) = cur.take() {
                    fields.push(c);
                }
            }
        }
        if let Some(c) = cur {
            fields.push(c);
        }
        fields
    }

    fn exec_command(&mut self, cmd: &Command, fs: &mut MemTree, stdin: &[u8], out: &mut Vec<u8>, err: &mut Vec<u8>) -> i32 {
        let mut argv = Vec::new();
        for w in &cmd.words {
            let fields = self.expand_word(w, fs, err);
            argv.extend(fields);
        }
        let mut assigned = Vec::new();
        for (name, value) in &cmd.assigns {
            let v = self.expand_word(value, fs, err).join(" ");
            assigned.push((name.clone(), v));
        }
        if argv.is_empty() {
            for (k, v) in assigned {
                self.vars.insert(k, v);
            }
            return self.status;
        }
        let mut input = stdin.to_vec();
        let mut out_target: Option<(String, bool)> = None;
        for (op, target) in &cmd.redirs {
            let path = self.expand_word(target, fs, err).join(" ");
            match op {
                Tok::RedirIn => {
                    if path == "/dev/null" {
                        input.clear();
                    } else {
                        match fs.read(&rel(&path)) {
                            Some(d) => input = d.to_vec(),
                            None => {
                                err.extend_from_slice(format!("bash: line 1: {path}: No such file or directory\n").as_bytes());
                                return 1;
                            }
                        }
                    }
                }
                Tok::RedirOut => out_target = Some((path, false)),
                Tok::RedirAppend => out_target = Some((path, true)),
                _ => {}
            }
        }
        let mut captured = Vec::new();
        let env = {
            let mut e = self.vars.clone();
            e.extend(assigned);
            e
        };
        let status = {
            let mut ctx = Ctx { fs, env: &env, stdin: &input, stdout: &mut captured, stderr: err };
            match builtin(&argv, &mut ctx) {
                Some(s) => s,
                None => match self.programs.run(&argv, &mut ctx) {
                    Some(s) => s,
                    None => {
                        ctx.stderr.extend_from_slice(format!("bash: line 1: {}: command not found\n", argv[0]).as_bytes());
                        127
                    }
                },
            }
        };
        match out_target {
            Some((path, _)) if path == "/dev/null" => {}
            Some((path, append)) => {
                let rel_path = rel(&path);
                let mut data = if append { fs.read(&rel_path).map(<[u8]>::to_vec).unwrap_or_default() } else { Vec::new() };
                data.extend_from_slice(&captured);
                write_file(fs, &rel_path, data);
            }
            None => out.extend_from_slice(&captured),
        }
        status
    }
}

/// Caminho relativo ao diretório do caso.
pub fn rel(path: &str) -> String {
    let p = path.strip_prefix(harness::CASE_DIR).unwrap_or(path);
    p.trim_start_matches('/').trim_start_matches("./").to_string()
}

/// Grava um arquivo preservando o modo, se ele já existe (0644 se é novo).
pub fn write_file(fs: &mut MemTree, path: &str, data: Vec<u8>) {
    let mode = match fs.get(path) {
        Some(Entry::File { mode, .. }) => *mode,
        _ => 0o644,
    };
    fs.insert(path, Entry::file(data, mode));
}

/// Remove `path` e tudo abaixo dele.
pub fn remove_tree(fs: &mut MemTree, path: &str) -> bool {
    let path = rel(path);
    let prefix = format!("{path}/");
    let before = fs.entries.len();
    fs.entries.retain(|k, _| k != &path && !k.starts_with(&prefix));
    before != fs.entries.len()
}

fn builtin(argv: &[String], ctx: &mut Ctx<'_>) -> Option<i32> {
    let args = &argv[1..];
    Some(match argv[0].as_str() {
        "true" | ":" => 0,
        "false" => 1,
        "echo" => {
            let (newline, args) = match args.first().map(String::as_str) {
                Some("-n") => (false, &args[1..]),
                _ => (true, args),
            };
            ctx.stdout.extend_from_slice(args.join(" ").as_bytes());
            if newline {
                ctx.stdout.push(b'\n');
            }
            0
        }
        "printf" => {
            let Some(fmt) = args.first() else { return Some(1) };
            ctx.stdout.extend_from_slice(&printf(fmt, &args[1..]));
            0
        }
        "cat" => {
            if args.is_empty() {
                ctx.stdout.extend_from_slice(ctx.stdin);
                return Some(0);
            }
            let mut status = 0;
            for a in args {
                match ctx.fs.read(&rel(a)) {
                    Some(d) => ctx.stdout.extend_from_slice(d),
                    None => {
                        ctx.stderr.extend_from_slice(format!("cat: {a}: No such file or directory\n").as_bytes());
                        status = 1;
                    }
                }
            }
            status
        }
        "rm" => {
            let mut force = false;
            let mut status = 0;
            for a in args {
                if a.starts_with('-') {
                    force |= a.contains('f');
                    continue;
                }
                if !remove_tree(ctx.fs, a) && !force {
                    ctx.stderr.extend_from_slice(format!("rm: cannot remove '{a}': No such file or directory\n").as_bytes());
                    status = 1;
                }
            }
            status
        }
        "mkdir" => {
            for a in args.iter().filter(|a| !a.starts_with('-')) {
                ctx.fs.insert(&rel(a), Entry::dir(0o755));
            }
            0
        }
        "ls" => {
            let dirs: Vec<String> = args.iter().filter(|a| !a.starts_with('-')).cloned().collect();
            let dir = dirs.first().map(|d| rel(d)).unwrap_or_default();
            let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
            let mut names: Vec<&str> = ctx
                .fs
                .entries
                .keys()
                .filter_map(|k| k.strip_prefix(prefix.as_str()))
                .filter(|rest| !rest.is_empty() && !rest.contains('/') && !rest.starts_with('.'))
                .collect();
            names.sort();
            for n in names {
                ctx.stdout.extend_from_slice(n.as_bytes());
                ctx.stdout.push(b'\n');
            }
            0
        }
        _ => return None,
    })
}

/// `printf` do bash, restrito a `%s`, `%d`, `%%` e às escapes comuns.
pub fn printf(fmt: &str, args: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut idx = 0;
    loop {
        let mut used_arg = false;
        let chars: Vec<char> = fmt.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '\\' && i + 1 < chars.len() {
                let mapped = match chars[i + 1] {
                    'n' => Some('\n'),
                    't' => Some('\t'),
                    '\\' => Some('\\'),
                    '"' => Some('"'),
                    '\'' => Some('\''),
                    'r' => Some('\r'),
                    _ => None,
                };
                match mapped {
                    Some(m) => {
                        let mut buf = [0; 4];
                        out.extend_from_slice(m.encode_utf8(&mut buf).as_bytes());
                        i += 2;
                    }
                    None => {
                        out.push(b'\\');
                        i += 1;
                    }
                }
                continue;
            }
            if c == '%' && i + 1 < chars.len() {
                match chars[i + 1] {
                    '%' => out.push(b'%'),
                    's' | 'd' | 'b' => {
                        let arg = args.get(idx).cloned().unwrap_or_default();
                        idx += 1;
                        used_arg = true;
                        out.extend_from_slice(arg.as_bytes());
                    }
                    other => {
                        out.push(b'%');
                        let mut buf = [0; 4];
                        out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
                    }
                }
                i += 2;
                continue;
            }
            let mut buf = [0; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            i += 1;
        }
        if !used_arg || idx >= args.len() {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echoer;

    impl Programs for Echoer {
        fn run(&mut self, argv: &[String], ctx: &mut Ctx<'_>) -> Option<i32> {
            if argv[0] == "args" {
                for a in &argv[1..] {
                    ctx.stdout.extend_from_slice(format!("[{a}]").as_bytes());
                }
                ctx.stdout.push(b'\n');
                return Some(0);
            }
            if argv[0] == "fail" {
                return Some(3);
            }
            None
        }
    }

    fn run(script: &str, fs: &mut MemTree) -> ScriptOutcome {
        run_script(script, fs, &BTreeMap::new(), b"", &mut Echoer).unwrap()
    }

    #[test]
    fn quoting_substitution_and_status() {
        let mut fs = MemTree::new();
        let out = run(
            r#"args 'a b' "c $X" d\ e; fail; echo "exit=$?"; t=$(echo hi there); args $t "$t"; fail || echo ok && echo yes"#,
            &mut fs,
        );
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            "[a b][c ][d e]\nexit=3\n[hi][there][hi there]\nok\nyes\n"
        );
    }

    #[test]
    fn redirects_and_pipes() {
        let mut fs = MemTree::new();
        let out = run("printf 'x\\ny\\n' > f.txt; printf 'z\\n' >> f.txt; cat f.txt | cat; cat < f.txt; rm f.txt; cat f.txt", &mut fs);
        assert_eq!(String::from_utf8(out.stdout).unwrap(), "x\ny\nz\nx\ny\nz\n");
        assert_eq!(out.status, 1);
        assert!(fs.get("f.txt").is_none());
    }
}
