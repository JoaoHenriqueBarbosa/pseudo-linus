//! Parse de um comando com o `brush-parser` e extração do que importa pra priorizar o userland:
//! nomes de comando, flags, recursos de shell, padrões passados pra grep/sed/awk/jq.
//!
//! O AST é serializado pra JSON (feature `serde` do brush-parser) e percorrido de forma genérica, o que
//! deixa o código pequeno e tolerante a mudanças de forma do AST.

use std::collections::BTreeSet;

use brush_parser::ParserOptions;
use serde_json::Value;

/// Uma ocorrência de comando simples.
#[derive(Clone, Debug)]
pub struct Occurrence {
    pub name: String,
    pub flags: Vec<String>,
    /// Comando embrulhador (sudo, xargs, timeout...) quando este nome veio de dentro dele.
    pub via: Option<String>,
    /// Apareceu dentro de `$(...)` ou crase.
    pub in_substitution: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub parsed: bool,
    pub parse_error: Option<String>,
    pub occurrences: Vec<Occurrence>,
    pub features: BTreeSet<String>,
    pub pipeline_lengths: Vec<usize>,
    pub patterns: Vec<(String, String)>,
}

/// Limite de tamanho pra parse (heredocs gigantes não mudam a estatística).
const MAX_PARSE_BYTES: usize = 200_000;

/// Comandos que executam outro comando passado como argumento.
const WRAPPERS: &[&str] = &[
    "sudo", "env", "timeout", "nohup", "nice", "time", "exec", "xargs", "watch", "stdbuf", "ionice",
    "setsid", "taskset", "chrt", "command", "builtin", "unbuffer", "flock", "systemd-run",
];

/// Opções que consomem o argumento seguinte, por embrulhador.
fn wrapper_option_takes_value(wrapper: &str, opt: &str) -> bool {
    match wrapper {
        "xargs" => matches!(opt, "-n" | "-I" | "-P" | "-L" | "-d" | "-s" | "-a" | "-E" | "-l"),
        "sudo" => matches!(opt, "-u" | "-g" | "-C" | "-D" | "-h" | "-p" | "-U"),
        "nice" => matches!(opt, "-n"),
        "timeout" => matches!(opt, "-s" | "-k"),
        "watch" => matches!(opt, "-n"),
        "ionice" => matches!(opt, "-c" | "-n" | "-p"),
        "taskset" => false,
        "flock" => matches!(opt, "-w" | "-E"),
        "env" => matches!(opt, "-u" | "-C" | "-S"),
        "systemd-run" => matches!(opt, "-p" | "--unit" | "-u"),
        _ => false,
    }
}

pub fn analyze(command: &str) -> Analysis {
    let mut out = Analysis::default();
    analyze_into(command, false, 0, &mut out);
    out
}

fn analyze_into(command: &str, in_substitution: bool, depth: usize, out: &mut Analysis) {
    if depth > 8 {
        return;
    }
    let text = if command.len() > MAX_PARSE_BYTES { &command[..floor_char_boundary(command, MAX_PARSE_BYTES)] } else { command };
    let options = ParserOptions::default();
    let parsed = std::panic::catch_unwind(|| {
        let mut parser = brush_parser::Parser::new(std::io::Cursor::new(text.as_bytes()), &options);
        parser.parse_program()
    });
    let program = match parsed {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => {
            if depth == 0 {
                out.parse_error = Some(error_kind(&format!("{e:?}")));
            }
            return;
        }
        Err(_) => {
            if depth == 0 {
                out.parse_error = Some("panic".into());
            }
            return;
        }
    };
    if depth == 0 {
        out.parsed = true;
    }
    let Ok(json) = serde_json::to_value(&program) else { return };
    let mut w = Walker { out, in_substitution, depth, options: &options };
    w.walk(&json);
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn error_kind(debug: &str) -> String {
    debug.split(|c: char| !c.is_alphanumeric()).find(|s| !s.is_empty()).unwrap_or("unknown").to_string()
}

struct Walker<'a> {
    out: &'a mut Analysis,
    in_substitution: bool,
    depth: usize,
    options: &'a ParserOptions,
}

impl Walker<'_> {
    fn feature(&mut self, name: &str) {
        self.out.features.insert(name.to_string());
    }

    fn walk(&mut self, v: &Value) {
        match v {
            Value::Object(map) => {
                // Pipeline: tem "seq".
                if let Some(Value::Array(seq)) = map.get("seq") {
                    self.out.pipeline_lengths.push(seq.len());
                    if seq.len() > 1 {
                        self.feature("pipeline");
                    }
                    if map.get("bang").and_then(Value::as_bool) == Some(true) {
                        self.feature("! pipeline");
                    }
                    if map.get("timed").is_some_and(|t| !t.is_null()) {
                        self.feature("time");
                    }
                }
                // Palavra: tem "value" (string) e "loc".
                if let (Some(Value::String(word)), true) = (map.get("value"), map.contains_key("loc")) {
                    self.word(word);
                }
                for (key, inner) in map {
                    match key.as_str() {
                        "Simple" => self.simple(inner),
                        "Function" => self.feature("function"),
                        "ExtendedTest" => self.feature("[[ ]]"),
                        "Arithmetic" => self.feature("(( ))"),
                        "ArithmeticForClause" => self.feature("for ((;;))"),
                        "BraceGroup" => self.feature("{ group }"),
                        "Subshell" => self.feature("( subshell )"),
                        "ForClause" => self.feature("for"),
                        "CaseClause" => self.feature("case"),
                        "IfClause" => self.feature("if"),
                        "WhileClause" => self.feature("while"),
                        "UntilClause" => self.feature("until"),
                        "Coprocess" => self.feature("coproc"),
                        "And" => self.feature("&&"),
                        "Or" => self.feature("||"),
                        "HereDocument" => self.feature("heredoc"),
                        "HereString" => self.feature("<<< here-string"),
                        "OutputAndError" => self.feature("&> redirect"),
                        "ProcessSubstitution" => self.feature("<() process substitution"),
                        "File" => self.redirect(inner),
                        "Array" => self.feature("array assignment"),
                        _ => {}
                    }
                    if key != "Simple" {
                        self.walk(inner);
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    if item.as_str() == Some("Async") {
                        self.feature("& background");
                    }
                    self.walk(item);
                }
            }
            _ => {}
        }
    }

    fn redirect(&mut self, inner: &Value) {
        // IoRedirect::File(fd, kind, target)
        if let Value::Array(parts) = inner {
            let fd = parts.first().and_then(Value::as_i64);
            let kind = parts.get(1).map(variant_name).unwrap_or_default();
            let target_is_fd = parts.get(2).map(variant_name).is_some_and(|t| t == "Fd" || t == "Duplicate");
            let label = match (fd, kind.as_str(), target_is_fd) {
                (Some(2), "DuplicateOutput", true) => "2>&1 style".to_string(),
                (Some(2), "Write" | "Append", _) => "2> redirect".to_string(),
                (_, "Write", _) => "> redirect".to_string(),
                (_, "Append", _) => ">> redirect".to_string(),
                (_, "Read", _) => "< redirect".to_string(),
                (_, k, _) => format!("redirect {k}"),
            };
            self.feature(&label);
        }
    }

    fn word(&mut self, raw: &str) {
        let Ok(pieces) = brush_parser::word::parse(raw, self.options) else { return };
        let Ok(json) = serde_json::to_value(&pieces) else { return };
        self.pieces(&json);
        let unquoted_text_has = |c: char| raw.contains(c);
        if (unquoted_text_has('*') || unquoted_text_has('?')) && !raw.starts_with('\'') && !raw.starts_with('"') {
            self.feature("glob");
        }
        if raw.contains('{') && (raw.contains("..") || raw.contains(',')) && !raw.contains("${") && !raw.starts_with('\'') && !raw.starts_with('"') {
            self.feature("brace expansion");
        }
    }

    fn pieces(&mut self, v: &Value) {
        match v {
            Value::Object(map) => {
                for (key, inner) in map {
                    match key.as_str() {
                        "CommandSubstitution" => {
                            self.feature("$( ) substitution");
                            if let Some(s) = inner.as_str() {
                                analyze_into(s, true, self.depth + 1, self.out);
                            }
                        }
                        "BackquotedCommandSubstitution" => {
                            self.feature("backtick substitution");
                            if let Some(s) = inner.as_str() {
                                analyze_into(s, true, self.depth + 1, self.out);
                            }
                        }
                        "ArithmeticExpression" => self.feature("$(( )) arithmetic"),
                        "AnsiCQuotedText" => self.feature("$'' ansi-c quote"),
                        "TildeExpansion" => self.feature("~ tilde"),
                        "ParameterExpansion" => {
                            let kind = variant_name(inner);
                            if kind == "Parameter" {
                                self.feature("$var");
                            } else {
                                self.feature(&format!("${{}} {kind}"));
                            }
                        }
                        _ => {}
                    }
                    self.pieces(inner);
                }
            }
            Value::Array(items) => items.iter().for_each(|i| self.pieces(i)),
            _ => {}
        }
    }

    fn simple(&mut self, inner: &Value) {
        // Prefixo: atribuições e redirecionamentos.
        if let Some(prefix) = inner.get("prefix") {
            if json_has_key(prefix, "AssignmentWord") {
                self.feature("VAR=value prefix/assignment");
            }
            self.walk(prefix);
        }
        let mut words: Vec<String> = Vec::new();
        if let Some(name) = inner.get("word_or_name") {
            if let Some(raw) = name.get("value").and_then(Value::as_str) {
                self.word(raw);
                words.push(raw.to_string());
            }
        }
        if let Some(suffix) = inner.get("suffix") {
            if let Value::Array(items) = suffix {
                for item in items {
                    if let Some(raw) = item.pointer("/Word/value").and_then(Value::as_str) {
                        words.push(raw.to_string());
                    }
                }
            }
            self.walk(suffix);
        }
        if words.is_empty() {
            if inner.get("prefix").is_some() {
                self.feature("bare assignment");
            }
            return;
        }
        let unquoted: Vec<String> = words.iter().map(|w| brush_parser::unquote_str(w)).collect();
        self.record(&unquoted, None);
    }

    fn record(&mut self, words: &[String], via: Option<String>) {
        let Some(first) = words.first() else { return };
        let name = normalize_name(first);
        let args = &words[1..];
        let inner_start = if WRAPPERS.contains(&name.as_str()) { wrapped_command_start(&name, args) } else { None };
        // As flags de um embrulhador são só as dele, não as do comando embrulhado.
        let own_args = &args[..inner_start.unwrap_or(args.len())];
        let flags: Vec<String> = own_args.iter().filter_map(|a| sanitize_flag(a)).collect();
        self.collect_patterns(&name, args);
        self.out.occurrences.push(Occurrence {
            name: name.clone(),
            flags,
            via: via.clone(),
            in_substitution: self.in_substitution,
        });
        if let Some(i) = inner_start {
            self.record(&args[i..], Some(name));
        }
    }

    fn collect_patterns(&mut self, name: &str, args: &[String]) {
        // O ripgrep usa sintaxe de regex do Rust, o grep usa BRE/ERE: ficam separados.
        let tool = match name {
            "grep" | "egrep" | "fgrep" | "zgrep" => "grep",
            "rg" => "rg",
            "sed" => "sed",
            "awk" | "gawk" | "mawk" => "awk",
            "jq" | "yq" => "jq",
            _ => return,
        };
        let takes_value: &[&str] = match tool {
            "grep" | "rg" => &["-A", "-B", "-C", "-m", "-f", "-d", "-D", "--include", "--exclude", "-g", "-t", "-T", "--max-count"],
            "sed" => &["-f", "-l"],
            "awk" => &["-F", "-v", "-f"],
            "jq" => &["--arg", "--argjson", "--slurpfile", "--rawfile", "--indent", "-L"],
            _ => &[],
        };
        let mut explicit = false;
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            if (a == "-e" || a == "--regexp" || a == "--expression") && i + 1 < args.len() {
                self.out.patterns.push((tool.into(), args[i + 1].clone()));
                explicit = true;
                i += 2;
                continue;
            }
            if takes_value.contains(&a.as_str()) {
                // --arg e --argjson consomem dois argumentos.
                i += if a == "--arg" || a == "--argjson" || a == "--slurpfile" || a == "--rawfile" { 3 } else { 2 };
                continue;
            }
            if a.starts_with('-') && a.len() > 1 {
                i += 1;
                continue;
            }
            if !explicit {
                self.out.patterns.push((tool.into(), a.clone()));
            }
            break;
        }
    }
}

/// Onde começa o comando executado por um embrulhador (sudo, xargs, timeout...), se houver.
fn wrapped_command_start(name: &str, args: &[String]) -> Option<usize> {
    if (name == "command" || name == "builtin") && args.first().is_some_and(|a| a == "-v" || a == "-V") {
        return None;
    }
    let mut i = 0;
    let mut skipped_positional = false;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            i += 1;
            break;
        }
        if a.starts_with('-') && a.len() > 1 {
            i += if wrapper_option_takes_value(name, a) { 2 } else { 1 };
            continue;
        }
        if name == "env" && a.contains('=') {
            i += 1;
            continue;
        }
        // timeout DURAÇÃO cmd, taskset MÁSCARA cmd.
        if (name == "timeout" || name == "taskset") && !skipped_positional {
            skipped_positional = true;
            i += 1;
            continue;
        }
        break;
    }
    (i < args.len()).then_some(i)
}

fn variant_name(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(map) => map.keys().next().cloned().unwrap_or_default(),
        _ => String::new(),
    }
}

fn json_has_key(v: &Value, key: &str) -> bool {
    match v {
        Value::Object(map) => map.contains_key(key) || map.values().any(|x| json_has_key(x, key)),
        Value::Array(items) => items.iter().any(|x| json_has_key(x, key)),
        _ => false,
    }
}

/// Nome de comando normalizado, sem vazar caminhos locais do dono.
pub fn normalize_name(raw: &str) -> String {
    if raw.contains('$') || raw.contains('`') {
        return "<dynamic>".into();
    }
    if raw.is_empty() {
        return "<empty>".into();
    }
    if raw.contains('/') {
        let system = ["/usr/bin/", "/bin/", "/usr/sbin/", "/sbin/", "/usr/local/bin/"];
        if system.iter().any(|p| raw.starts_with(p)) {
            return raw.rsplit('/').next().unwrap_or(raw).to_string();
        }
        return "<path-script>".into();
    }
    raw.to_string()
}

/// Flag sem valor (corta em `=`), descartando coisa longa ou com cara de segredo.
fn sanitize_flag(arg: &str) -> Option<String> {
    if !arg.starts_with('-') || arg == "-" || arg == "--" {
        return None;
    }
    let flag = arg.split('=').next().unwrap_or(arg);
    if flag.len() > 32 || flag.chars().any(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')) {
        return None;
    }
    // `-0` é flag de verdade (xargs -0, sort -z não); os outros números são a forma antiga de -n N.
    if flag == "-0" {
        return Some(flag.to_string());
    }
    if flag[1..].chars().all(|c| c.is_ascii_digit() || c == '.') && flag.len() > 1 && !flag.starts_with("--") {
        // head -5, tail -20: forma antiga de -n N; registra genericamente.
        return Some("-<N>".into());
    }
    Some(flag.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_commands_features_and_patterns() {
        let a = analyze("set -euo pipefail; find . -name '*.rs' | xargs grep -n 'fn main' | head -5 && echo \"$(date +%F)\" > out.txt 2>&1");
        assert!(a.parsed);
        let names: Vec<&str> = a.occurrences.iter().map(|o| o.name.as_str()).collect();
        for n in ["set", "find", "xargs", "grep", "head", "echo", "date"] {
            assert!(names.contains(&n), "{n} em {names:?}");
        }
        let grep = a.occurrences.iter().find(|o| o.name == "grep").unwrap();
        assert_eq!(grep.via.as_deref(), Some("xargs"));
        assert!(a.features.contains("pipeline"), "{:?}", a.features);
        assert!(a.features.contains("&&"));
        assert!(a.features.contains("$( ) substitution"));
        assert!(a.patterns.iter().any(|(t, p)| t == "grep" && p == "fn main"), "{:?}", a.patterns);
        let date = a.occurrences.iter().find(|o| o.name == "date").unwrap();
        assert!(date.in_substitution);
    }

    #[test]
    fn heredoc_and_control_flow() {
        let a = analyze("cat <<'EOF' > f.txt\nhello\nEOF\nfor f in *.txt; do wc -l \"$f\"; done\nif [[ -f x ]]; then echo y; fi");
        assert!(a.parsed);
        for f in ["heredoc", "for", "if", "[[ ]]", "glob"] {
            assert!(a.features.contains(f), "{f} em {:?}", a.features);
        }
    }

    #[test]
    fn local_paths_are_not_leaked() {
        assert_eq!(normalize_name("/usr/bin/grep"), "grep");
        assert_eq!(normalize_name("./deploy.sh"), "<path-script>");
        assert_eq!(normalize_name("/home/x/bin/tool"), "<path-script>");
        assert_eq!(normalize_name("$CMD"), "<dynamic>");
    }
}
