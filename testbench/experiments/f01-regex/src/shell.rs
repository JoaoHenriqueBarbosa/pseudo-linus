//! Leitura aproximada de linhas de comando de shell, só pra recuperar as flags com que os agentes
//! chamaram `grep` e `sed` (o `patterns.jsonl` do E08 guarda o padrão, não as flags). Nada aqui é
//! executado.

/// Quebra uma linha de comando em comandos simples (listas de palavras já sem aspas).
pub fn simple_commands(line: &str) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let flush_word = |word: &mut String, in_word: &mut bool, words: &mut Vec<String>| {
        if *in_word {
            words.push(std::mem::take(word));
            *in_word = false;
        }
    };
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' => {
                in_word = true;
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    word.push(chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '"' => {
                in_word = true;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() && "\"\\$`\n".contains(chars[i + 1]) {
                        if chars[i + 1] != '\n' {
                            word.push(chars[i + 1]);
                        }
                        i += 2;
                        continue;
                    }
                    word.push(chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '\\' => {
                in_word = true;
                if i + 1 < chars.len() {
                    if chars[i + 1] != '\n' {
                        word.push(chars[i + 1]);
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            ' ' | '\t' => {
                flush_word(&mut word, &mut in_word, &mut words);
                i += 1;
            }
            '|' | ';' | '&' | '\n' | '(' | ')' | '`' => {
                flush_word(&mut word, &mut in_word, &mut words);
                if !words.is_empty() {
                    out.push(std::mem::take(&mut words));
                }
                i += 1;
            }
            '$' if chars.get(i + 1) == Some(&'(') => {
                flush_word(&mut word, &mut in_word, &mut words);
                if !words.is_empty() {
                    out.push(std::mem::take(&mut words));
                }
                i += 2;
            }
            '<' if chars.get(i + 1) == Some(&'<') => {
                // Here-doc: o corpo não é comando; para a análise da linha aqui.
                flush_word(&mut word, &mut in_word, &mut words);
                break;
            }
            _ => {
                in_word = true;
                word.push(c);
                i += 1;
            }
        }
    }
    flush_word(&mut word, &mut in_word, &mut words);
    if !words.is_empty() {
        out.push(words);
    }
    out
}

/// Pula atribuições de ambiente e prefixos como `sudo`, `time`, `xargs -0`, `timeout 5`.
pub fn command_words(words: &[String]) -> &[String] {
    let mut i = 0;
    while i < words.len() {
        let w = words[i].as_str();
        if w.contains('=') && !w.starts_with('-') && w.split('=').next().is_some_and(|k| k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')) {
            i += 1;
            continue;
        }
        match w {
            "sudo" | "time" | "command" | "nice" | "env" | "exec" | "!" | "then" | "do" | "else" | "{" => i += 1,
            "timeout" => {
                i += 1;
                while i < words.len() && words[i].starts_with('-') {
                    i += 1;
                }
                i += 1;
            }
            "xargs" => {
                i += 1;
                while i < words.len() && words[i].starts_with('-') {
                    let opt = words[i].as_str();
                    i += 1;
                    if matches!(opt, "-I" | "-n" | "-P" | "-d" | "-L" | "-s" | "-a" | "-E") {
                        i += 1;
                    }
                }
            }
            _ => break,
        }
    }
    &words[i.min(words.len())..]
}

fn basename(w: &str) -> &str {
    w.rsplit('/').next().unwrap_or(w)
}

/// Uma chamada de grep: modo (`G`, `E`, `F`, `P`), caixa e padrões.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrepCall {
    pub mode: char,
    pub icase: bool,
    pub patterns: Vec<String>,
}

pub fn parse_grep(words: &[String]) -> Option<GrepCall> {
    let words = command_words(words);
    let prog = basename(words.first()?);
    let mut mode = match prog {
        "grep" => 'G',
        "egrep" => 'E',
        "fgrep" => 'F',
        _ => return None,
    };
    let mut icase = false;
    let mut patterns = Vec::new();
    let mut operands = Vec::new();
    let mut i = 1;
    let mut opts_done = false;
    const LONG_WITH_ARG: &[&str] = &[
        "regexp", "file", "max-count", "after-context", "before-context", "context", "include", "exclude",
        "exclude-dir", "exclude-from", "label", "binary-files", "devices", "directories", "group-separator",
    ];
    while i < words.len() {
        let w = &words[i];
        i += 1;
        if opts_done || !w.starts_with('-') || w == "-" {
            operands.push(w.clone());
            continue;
        }
        if w == "--" {
            opts_done = true;
            continue;
        }
        if let Some(long) = w.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            match name {
                "extended-regexp" => mode = 'E',
                "fixed-strings" => mode = 'F',
                "basic-regexp" => mode = 'G',
                "perl-regexp" => mode = 'P',
                "ignore-case" => icase = true,
                "no-ignore-case" => icase = false,
                _ => {}
            }
            if LONG_WITH_ARG.contains(&name) {
                let v = match value {
                    Some(v) => Some(v),
                    None => {
                        i += 1;
                        words.get(i - 1).cloned()
                    }
                };
                if name == "regexp" {
                    patterns.extend(v);
                }
            }
            continue;
        }
        let cluster: Vec<char> = w[1..].chars().collect();
        let mut j = 0;
        while j < cluster.len() {
            let c = cluster[j];
            j += 1;
            match c {
                'E' | 'F' | 'G' | 'P' => mode = c,
                'i' | 'y' => icase = true,
                'e' | 'f' | 'm' | 'A' | 'B' | 'C' | 'd' | 'D' => {
                    let rest: String = cluster[j..].iter().collect();
                    let v = if !rest.is_empty() {
                        Some(rest)
                    } else {
                        i += 1;
                        words.get(i - 1).cloned()
                    };
                    if c == 'e' {
                        patterns.extend(v);
                    }
                    break;
                }
                _ => {}
            }
        }
    }
    if patterns.is_empty() {
        patterns.push(operands.first()?.clone());
    }
    Some(GrepCall { mode, icase, patterns })
}

/// Uma chamada de sed: ERE ou não, e os scripts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SedCall {
    pub ere: bool,
    pub scripts: Vec<String>,
}

pub fn parse_sed(words: &[String]) -> Option<SedCall> {
    let words = command_words(words);
    if basename(words.first()?) != "sed" {
        return None;
    }
    let mut ere = false;
    let mut scripts = Vec::new();
    let mut operands = Vec::new();
    let mut has_file_script = false;
    let mut i = 1;
    while i < words.len() {
        let w = &words[i];
        i += 1;
        if !w.starts_with('-') || w == "-" {
            operands.push(w.clone());
            continue;
        }
        if let Some(long) = w.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            match name {
                "regexp-extended" => ere = true,
                "expression" => {
                    let v = value.or_else(|| {
                        i += 1;
                        words.get(i - 1).cloned()
                    });
                    scripts.extend(v);
                }
                "file" => has_file_script = true,
                "line-length" if value.is_none() => i += 1,
                _ => {}
            }
            continue;
        }
        let cluster: Vec<char> = w[1..].chars().collect();
        let mut j = 0;
        while j < cluster.len() {
            let c = cluster[j];
            j += 1;
            match c {
                'E' | 'r' => ere = true,
                'i' => break,
                'e' | 'f' | 'l' => {
                    let rest: String = cluster[j..].iter().collect();
                    let v = if !rest.is_empty() {
                        Some(rest)
                    } else {
                        i += 1;
                        words.get(i - 1).cloned()
                    };
                    if c == 'e' {
                        scripts.extend(v);
                    } else if c == 'f' {
                        has_file_script = true;
                    }
                    break;
                }
                _ => {}
            }
        }
    }
    if scripts.is_empty() && !has_file_script {
        scripts.push(operands.first()?.clone());
    }
    Some(SedCall { ere, scripts })
}

/// Uma regex dentro de um script de sed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SedRegex {
    pub pattern: String,
    pub icase: bool,
}

/// Extrai as regexes de endereços (`/re/`, `\cREc`) e de comandos `s` de um script de sed.
pub fn sed_script_regexes(script: &str) -> Vec<SedRegex> {
    let s: Vec<char> = script.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    // Lê até o delimitador não escapado; `\delim` vira `delim` (como o match_slash do sed).
    let read_delimited = |i: &mut usize, delim: char| -> Option<String> {
        let mut text = String::new();
        while *i < s.len() {
            let c = s[*i];
            if c == '\\' && *i + 1 < s.len() {
                let n = s[*i + 1];
                if n == delim {
                    text.push(n);
                } else {
                    text.push('\\');
                    text.push(n);
                }
                *i += 2;
                continue;
            }
            if c == '[' {
                // Delimitador dentro de colchetes não fecha a regex.
                text.push(c);
                *i += 1;
                let start = *i;
                while *i < s.len() && (s[*i] != ']' || *i == start || (*i == start + 1 && s[start] == '^')) {
                    text.push(s[*i]);
                    *i += 1;
                }
                if *i < s.len() {
                    text.push(']');
                    *i += 1;
                }
                continue;
            }
            if c == delim {
                *i += 1;
                return Some(text);
            }
            if c == '\n' {
                return None;
            }
            text.push(c);
            *i += 1;
        }
        None
    };
    let skip_to_end = |i: &mut usize| {
        while *i < s.len() && s[*i] != '\n' {
            *i += 1;
        }
    };
    let skip_label = |i: &mut usize| {
        while *i < s.len() && s[*i] != '\n' && s[*i] != ';' && s[*i] != '}' {
            *i += 1;
        }
    };
    while i < s.len() {
        let c = s[i];
        if c.is_whitespace() || c == ';' || c == '}' || c == '{' || c == '!' {
            i += 1;
            continue;
        }
        // Endereços.
        let mut is_address = false;
        if c == '/' || (c == '\\' && i + 1 < s.len()) {
            let delim = if c == '/' { '/' } else { s[i + 1] };
            i += if c == '/' { 1 } else { 2 };
            let Some(re) = read_delimited(&mut i, delim) else { break };
            let mut icase = false;
            while i < s.len() && matches!(s[i], 'I' | 'M') {
                icase |= s[i] == 'I';
                i += 1;
            }
            if !re.is_empty() {
                out.push(SedRegex { pattern: re, icase });
            }
            is_address = true;
        } else if c.is_ascii_digit() || c == '$' {
            while i < s.len() && (s[i].is_ascii_digit() || s[i] == '$' || s[i] == '~') {
                i += 1;
            }
            is_address = true;
        }
        if is_address {
            if i < s.len() && s[i] == ',' {
                i += 1;
                if i < s.len() && (s[i] == '+' || s[i] == '~') {
                    i += 1;
                }
            }
            continue;
        }
        i += 1;
        match c {
            's' => {
                let Some(&delim) = s.get(i) else { break };
                i += 1;
                let Some(re) = read_delimited(&mut i, delim) else { break };
                // Substituição: até o próximo delimitador não escapado.
                while i < s.len() && s[i] != delim {
                    if s[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
                let mut icase = false;
                while i < s.len() && !matches!(s[i], ';' | '\n' | '}' | ' ') {
                    match s[i] {
                        'I' | 'i' => icase = true,
                        'w' => {
                            skip_to_end(&mut i);
                            break;
                        }
                        _ => {}
                    }
                    i += 1;
                }
                if !re.is_empty() {
                    out.push(SedRegex { pattern: re, icase });
                }
            }
            'y' => {
                let Some(&delim) = s.get(i) else { break };
                i += 1;
                let _ = read_delimited(&mut i, delim);
                let _ = read_delimited(&mut i, delim);
            }
            'a' | 'i' | 'c' | 'r' | 'R' | 'w' | 'W' | 'e' => skip_to_end(&mut i),
            'b' | 't' | 'T' | ':' => skip_label(&mut i),
            'q' | 'Q' | 'l' | 'L' => {
                while i < s.len() && (s[i].is_ascii_digit() || s[i] == ' ') {
                    i += 1;
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<String> {
        simple_commands(s).remove(0)
    }

    #[test]
    fn splits_pipelines_and_quotes() {
        let cmds = simple_commands(r#"cd /x && grep -rnE "foo|bar" src | head -5; echo 'a b'"#);
        assert_eq!(cmds.len(), 4);
        assert_eq!(cmds[1], vec!["grep", "-rnE", "foo|bar", "src"]);
        assert_eq!(cmds[3], vec!["echo", "a b"]);
    }

    #[test]
    fn grep_flags() {
        let g = parse_grep(&w(r#"grep -rniE 'a(b|c)' ."#)).unwrap();
        assert_eq!((g.mode, g.icase, g.patterns.clone()), ('E', true, vec!["a(b|c)".to_string()]));
        let g = parse_grep(&w(r#"grep -n -e foo -e "ba\|r" -A 2 f"#)).unwrap();
        assert_eq!(g.patterns, vec!["foo", "ba\\|r"]);
        assert_eq!(g.mode, 'G');
        let g = parse_grep(&w(r#"xargs -0 grep -l --include=*.rs 'x\+'"#)).unwrap();
        assert_eq!(g.patterns, vec!["x\\+"]);
        assert_eq!(parse_grep(&w("fgrep -i abc")).unwrap().mode, 'F');
    }

    #[test]
    fn sed_flags_and_regexes() {
        let s = parse_sed(&w(r#"sed -E -n 's/^v([0-9]+)/\1/p' f"#)).unwrap();
        assert!(s.ere);
        assert_eq!(s.scripts, vec![r"s/^v([0-9]+)/\1/p"]);
        let r = sed_script_regexes(r"/^#/d; s|a/b|c|gI; \%x%p; 1,/end/{s/[/]x//}; y/ab/cd/");
        let pats: Vec<&str> = r.iter().map(|x| x.pattern.as_str()).collect();
        assert_eq!(pats, vec!["^#", "a/b", "x", "end", "[/]x"]);
        assert!(r[1].icase);
        assert!(sed_script_regexes("1,60p").is_empty());
        assert_eq!(sed_script_regexes(r"s/a\/b/x/")[0].pattern, "a/b");
    }
}
