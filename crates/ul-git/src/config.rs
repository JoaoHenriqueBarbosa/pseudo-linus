//! Configuração do git: leitura com a gramática do `config.c` (seções, subseções, aspas, escapes,
//! continuação de linha, comentários) e edição que preserva o resto do arquivo, como o
//! `git_config_set_multivar_in_file`.

use crate::error::{Fail, R, error, warning};
use crate::os;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    System,
    Global,
    Local,
    Worktree,
    Command,
}

impl Scope {
    pub fn name(self) -> &'static str {
        match self {
            Scope::System => "system",
            Scope::Global => "global",
            Scope::Local => "local",
            Scope::Worktree => "worktree",
            Scope::Command => "command",
        }
    }
}

/// Uma variável lida.
#[derive(Clone, Debug)]
pub struct Entry {
    /// Chave canônica: seção e nome em minúsculas, subseção como escrita.
    pub key: String,
    /// `None` quando a linha não tem `=` (booleano verdadeiro).
    pub value: Option<Vec<u8>>,
    /// Início e fim (depois do `\n`) das linhas da variável no arquivo.
    pub span: (usize, usize),
    /// Índice da seção em `ConfigFile::sections`.
    pub section: usize,
}

#[derive(Clone, Debug)]
pub struct Section {
    /// `seção` ou `seção.subseção` canônicos.
    pub name: String,
    pub span: (usize, usize),
}

#[derive(Clone, Debug)]
pub struct ConfigFile {
    /// Como o caminho aparece em `--show-origin` (`.git/config`, `/root/.gitconfig`).
    pub path: Vec<u8>,
    pub scope: Scope,
    pub data: Vec<u8>,
    pub entries: Vec<Entry>,
    pub sections: Vec<Section>,
    /// Origem pra `--show-origin` de entradas da linha de comando.
    pub command_line: bool,
}

/// Erro de sintaxe: linha (1-based).
pub struct SyntaxError(pub usize);

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    line: usize,
    eof: bool,
}

impl Cursor<'_> {
    /// Próximo caractere, com `\r\n` virando `\n` e o fim virando `\n` (como o `get_next_char`).
    fn next(&mut self) -> u8 {
        if self.pos >= self.data.len() {
            self.eof = true;
            return b'\n';
        }
        let mut c = self.data[self.pos];
        self.pos += 1;
        if c == b'\r' && self.data.get(self.pos) == Some(&b'\n') {
            self.pos += 1;
            c = b'\n';
        }
        if c == b'\n' {
            self.line += 1;
        }
        c
    }
}

fn is_key_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-'
}

/// Lê o valor depois do `=` com as regras do git-config(1): espaço fora de aspas no começo e no
/// fim some (o de dentro fica), `#`/`;` fora de aspas começam comentário, `\` no fim da linha
/// continua, escapes `\n \t \b \" \\` e nenhum outro. `None` se a linha é inválida.
fn parse_value(cur: &mut Cursor<'_>) -> Option<Vec<u8>> {
    let mut value: Vec<u8> = Vec::new();
    // Espaço sem aspas visto depois do último caractere "de verdade": só entra se vier mais coisa.
    let mut held_ws: Vec<u8> = Vec::new();
    let mut in_quotes = false;
    let mut in_comment = false;
    let emit = |value: &mut Vec<u8>, held: &mut Vec<u8>, c: u8| {
        if !value.is_empty() {
            value.append(held);
        } else {
            held.clear();
        }
        value.push(c);
    };
    loop {
        let c = cur.next();
        if c == b'\n' {
            // Fim da linha (ou do arquivo): aspas abertas tornam a linha inválida.
            return if in_quotes { None } else { Some(value) };
        }
        if in_comment {
            continue;
        }
        if !in_quotes {
            if c == b' ' || c == b'\t' {
                held_ws.push(c);
                continue;
            }
            if c == b'#' || c == b';' {
                in_comment = true;
                continue;
            }
        }
        match c {
            b'"' => {
                // Abrir aspas também "fixa" o espaço guardado antes delas.
                if !value.is_empty() {
                    value.append(&mut held_ws);
                } else {
                    held_ws.clear();
                }
                in_quotes = !in_quotes;
            }
            b'\\' => {
                let decoded = match cur.next() {
                    b'\n' => continue,
                    b'n' => b'\n',
                    b't' => b'\t',
                    b'b' => 8,
                    b'"' => b'"',
                    b'\\' => b'\\',
                    _ => return None,
                };
                emit(&mut value, &mut held_ws, decoded);
            }
            _ => emit(&mut value, &mut held_ws, c),
        }
    }
}

impl ConfigFile {
    pub fn parse(path: Vec<u8>, scope: Scope, data: Vec<u8>) -> Result<ConfigFile, SyntaxError> {
        let mut f = ConfigFile { path, scope, data: Vec::new(), entries: Vec::new(), sections: Vec::new(), command_line: false };
        let mut cur = Cursor { data: &data, pos: 0, line: 1, eof: false };
        if data.starts_with(b"\xef\xbb\xbf") {
            cur.pos = 3;
        }
        let mut section: Option<String> = None;
        let mut line_start = cur.pos;
        loop {
            let at = cur.pos;
            let c = cur.next();
            if cur.eof {
                break;
            }
            if c == b'\n' {
                line_start = cur.pos;
                continue;
            }
            if c.is_ascii_whitespace() {
                continue;
            }
            if c == b'#' || c == b';' {
                loop {
                    let c = cur.next();
                    if c == b'\n' {
                        break;
                    }
                }
                line_start = cur.pos;
                continue;
            }
            if c == b'[' {
                let name = parse_section_header(&mut cur).ok_or(SyntaxError(cur.line))?;
                f.sections.push(Section { name: name.clone(), span: (at, cur.pos) });
                section = Some(name);
                line_start = cur.pos;
                continue;
            }
            if !c.is_ascii_alphabetic() {
                return Err(SyntaxError(cur.line));
            }
            let Some(sect) = section.clone() else { return Err(SyntaxError(cur.line)) };
            let mut name = vec![c.to_ascii_lowercase()];
            let mut c;
            loop {
                c = cur.next();
                if cur.eof || !is_key_char(c) {
                    break;
                }
                name.push(c.to_ascii_lowercase());
            }
            while c == b' ' || c == b'\t' {
                c = cur.next();
            }
            let value = if c == b'\n' {
                None
            } else if c == b'=' {
                Some(parse_value(&mut cur).ok_or(SyntaxError(cur.line))?)
            } else if c == b'#' || c == b';' {
                loop {
                    if cur.next() == b'\n' {
                        break;
                    }
                }
                None
            } else {
                return Err(SyntaxError(cur.line));
            };
            let line_no = cur.line;
            let _ = line_no;
            f.entries.push(Entry {
                key: format!("{sect}.{}", String::from_utf8_lossy(&name)),
                value,
                span: (line_start, cur.pos),
                section: f.sections.len().saturating_sub(1),
            });
            line_start = cur.pos;
            if cur.eof {
                break;
            }
        }
        f.data = data;
        Ok(f)
    }

    /// Lê e analisa um arquivo; ausente vira `None`.
    pub fn load(path: &[u8], display: Vec<u8>, scope: Scope) -> R<Option<ConfigFile>> {
        match os::read_opt(path) {
            Ok(Some(data)) => match ConfigFile::parse(display.clone(), scope, data) {
                Ok(f) => Ok(Some(f)),
                Err(SyntaxError(line)) => Err(Fail::Fatal(format!("bad config line {line} in file {}", os::lossy(&display)))),
            },
            Ok(None) => Ok(None),
            Err(sysabi::Errno::EISDIR) => Ok(None),
            Err(e) => Err(Fail::Fatal(format!("unable to access '{}': {}", os::lossy(&display), e.message()))),
        }
    }
}

/// `[seção]`, `[seção "sub"]` ou `[seção.sub]` (formato antigo, sub em minúsculas).
fn parse_section_header(cur: &mut Cursor<'_>) -> Option<String> {
    let mut name = String::new();
    loop {
        let c = cur.next();
        if cur.eof || c == b'\n' {
            return None;
        }
        if c == b']' {
            return if name.is_empty() { None } else { Some(name) };
        }
        if c.is_ascii_whitespace() {
            // Subseção entre aspas.
            let mut c = c;
            while c.is_ascii_whitespace() {
                c = cur.next();
                if c == b'\n' {
                    return None;
                }
            }
            if c != b'"' {
                return None;
            }
            let mut sub = Vec::new();
            loop {
                let c = cur.next();
                if c == b'\n' {
                    return None;
                }
                if c == b'"' {
                    break;
                }
                if c == b'\\' {
                    let e = cur.next();
                    if e == b'\n' {
                        return None;
                    }
                    sub.push(e);
                    continue;
                }
                sub.push(c);
            }
            if cur.next() != b']' {
                return None;
            }
            name.push('.');
            name.push_str(&String::from_utf8_lossy(&sub));
            return Some(name);
        }
        if !c.is_ascii_alphanumeric() && c != b'-' && c != b'.' {
            return None;
        }
        name.push(c.to_ascii_lowercase() as char);
    }
}

/// Partes de uma chave dada pelo usuário: (seção como escrita, subseção, nome como escrito).
pub struct KeyParts {
    pub section: String,
    pub subsection: Option<String>,
    pub name: String,
}

impl KeyParts {
    pub fn canonical(&self) -> String {
        match &self.subsection {
            Some(s) => format!("{}.{}.{}", self.section.to_ascii_lowercase(), s, self.name.to_ascii_lowercase()),
            None => format!("{}.{}", self.section.to_ascii_lowercase(), self.name.to_ascii_lowercase()),
        }
    }

    pub fn section_canonical(&self) -> String {
        match &self.subsection {
            Some(s) => format!("{}.{}", self.section.to_ascii_lowercase(), s),
            None => self.section.to_ascii_lowercase(),
        }
    }
}

/// `git_config_parse_key`: valida e separa. Erro já no texto do git.
pub fn parse_key(key: &str) -> Result<KeyParts, String> {
    let first = key.find('.');
    let last = key.rfind('.');
    let (Some(first), Some(last)) = (first, last) else {
        return Err(format!("key does not contain a section: {key}"));
    };
    if last + 1 >= key.len() {
        return Err(format!("key does not contain variable name: {key}"));
    }
    if first == 0 {
        return Err(format!("key does not contain a section: {key}"));
    }
    let section = &key[..first];
    let name = &key[last + 1..];
    if !section.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
        return Err(format!("invalid key: {key}"));
    }
    if !name.as_bytes()[0].is_ascii_alphabetic() || !name.bytes().all(is_key_char) {
        return Err(format!("invalid key: {key}"));
    }
    let subsection = if first == last { None } else { Some(key[first + 1..last].to_string()) };
    if let Some(s) = &subsection
        && s.contains('\n')
    {
        return Err(format!("invalid key (newline): {key}"));
    }
    Ok(KeyParts { section: section.to_string(), subsection, name: name.to_string() })
}

/// Chave canônica de qualquer texto (sem validar).
pub fn canonical_key(key: &str) -> String {
    match parse_key(key) {
        Ok(k) => k.canonical(),
        Err(_) => key.to_ascii_lowercase(),
    }
}

/// Valor no formato do arquivo (com aspas e escapes quando precisa), como o `write_pair`.
pub fn quote_value(value: &[u8]) -> Vec<u8> {
    let need_quote = value.first() == Some(&b' ')
        || value.last() == Some(&b' ')
        || value.iter().any(|c| *c == b';' || *c == b'#');
    let mut out = Vec::new();
    if need_quote {
        out.push(b'"');
    }
    for &c in value {
        match c {
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'"' | b'\\' => {
                out.push(b'\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    if need_quote {
        out.push(b'"');
    }
    out
}

fn section_header(k: &KeyParts) -> Vec<u8> {
    let mut out = format!("[{}", k.section).into_bytes();
    if let Some(s) = &k.subsection {
        out.extend_from_slice(b" \"");
        for c in s.bytes() {
            if c == b'"' || c == b'\\' {
                out.push(b'\\');
            }
            out.push(c);
        }
        out.push(b'"');
    }
    out.extend_from_slice(b"]\n");
    out
}

fn entry_line(name: &str, value: Option<&[u8]>) -> Vec<u8> {
    let mut out = format!("\t{name}").into_bytes();
    if let Some(v) = value {
        out.extend_from_slice(b" = ");
        out.extend_from_slice(&quote_value(v));
    }
    out.push(b'\n');
    out
}

/// Resultado de uma edição.
pub enum Edit {
    Ok(Vec<u8>),
    /// Código de saída do `git config` (5: chave com vários valores ou ausente no unset).
    Fail(i32),
}

/// Filtro de valor (`value-pattern` do `git config`).
pub type ValueFilter<'a> = &'a dyn Fn(Option<&[u8]>) -> bool;

/// Edita o conteúdo de um arquivo de configuração.
pub struct Editor<'a> {
    pub file: &'a ConfigFile,
}

impl Editor<'_> {
    fn matches(&self, canon: &str, filter: Option<ValueFilter<'_>>) -> Vec<usize> {
        self.file
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.key == canon && filter.is_none_or(|f| f(e.value.as_deref())))
            .map(|(i, _)| i)
            .collect()
    }

    /// `git config k v` (ou `--add`, `--replace-all`).
    pub fn set(&self, key: &KeyParts, value: Option<&[u8]>, add: bool, replace_all: bool, filter: Option<ValueFilter<'_>>) -> Edit {
        let canon = key.canonical();
        let data = &self.file.data;
        let line = entry_line(&key.name, value);
        let found = if add { Vec::new() } else { self.matches(&canon, filter) };
        if found.len() > 1 && !replace_all {
            warning(&format!("{canon} has multiple values"));
            error(&format!("cannot overwrite multiple values with a single value\n       Use a regexp, --add or --replace-all to change {canon}."));
            return Edit::Fail(5);
        }
        if !found.is_empty() {
            let mut out = Vec::new();
            let mut pos = 0;
            for (n, &i) in found.iter().enumerate() {
                let (s, e) = self.file.entries[i].span;
                out.extend_from_slice(&data[pos..s]);
                if n + 1 == found.len() {
                    out.extend_from_slice(&line);
                }
                pos = e;
            }
            // Se a entrada era a última linha sem `\n`, o `\n` da linha nova já fecha o arquivo.
            out.extend_from_slice(&data[pos..]);
            return Edit::Ok(out);
        }
        let sect = key.section_canonical();
        // Insere depois da última variável da última seção com esse nome.
        let last_section = self.file.sections.iter().enumerate().rev().find(|(_, s)| s.name == sect).map(|(i, _)| i);
        if let Some(si) = last_section {
            let insert_at = self
                .file
                .entries
                .iter()
                .filter(|e| e.section == si)
                .map(|e| e.span.1)
                .max()
                .unwrap_or(self.file.sections[si].span.1);
            let mut out = data[..insert_at].to_vec();
            if !out.is_empty() && !out.ends_with(b"\n") {
                out.push(b'\n');
            }
            out.extend_from_slice(&line);
            out.extend_from_slice(&data[insert_at..]);
            return Edit::Ok(out);
        }
        let mut out = data.clone();
        if !out.is_empty() && !out.ends_with(b"\n") {
            out.push(b'\n');
        }
        out.extend_from_slice(&section_header(key));
        out.extend_from_slice(&line);
        Edit::Ok(out)
    }

    /// `git config --unset` / `--unset-all`.
    pub fn unset(&self, key: &KeyParts, all: bool, filter: Option<ValueFilter<'_>>) -> Edit {
        let canon = key.canonical();
        let found = self.matches(&canon, filter);
        if found.is_empty() {
            return Edit::Fail(5);
        }
        if found.len() > 1 && !all {
            warning(&format!("{canon} has multiple values"));
            return Edit::Fail(5);
        }
        let mut remove: Vec<(usize, usize)> = found.iter().map(|&i| self.file.entries[i].span).collect();
        // Seções que ficaram sem variáveis (e sem comentários) somem também.
        for (si, s) in self.file.sections.iter().enumerate() {
            let in_section: Vec<usize> = self.file.entries.iter().enumerate().filter(|(_, e)| e.section == si).map(|(i, _)| i).collect();
            if in_section.is_empty() || !in_section.iter().all(|i| found.contains(i)) {
                continue;
            }
            let end = self.section_end(si);
            let body_ok = {
                let mut ok = true;
                let mut pos = s.span.1;
                let mut spans: Vec<(usize, usize)> = in_section.iter().map(|&i| self.file.entries[i].span).collect();
                spans.sort();
                for (a, b) in spans {
                    if !self.file.data[pos..a].iter().all(|c| c.is_ascii_whitespace()) {
                        ok = false;
                    }
                    pos = b;
                }
                ok && self.file.data[pos..end].iter().all(|c| c.is_ascii_whitespace())
            };
            if body_ok {
                remove.retain(|(a, _)| *a < s.span.0 || *a >= end);
                remove.push((s.span.0, end));
            }
        }
        remove.sort();
        Edit::Ok(cut(&self.file.data, &remove))
    }

    /// Fim do bloco da seção `si` (começo da próxima seção ou fim do arquivo).
    fn section_end(&self, si: usize) -> usize {
        self.file.sections.get(si + 1).map(|s| s.span.0).unwrap_or(self.file.data.len())
    }

    /// `--remove-section`; `None` se a seção não existe.
    pub fn remove_section(&self, name: &str) -> Option<Vec<u8>> {
        let mut spans = Vec::new();
        for (si, s) in self.file.sections.iter().enumerate() {
            if s.name == name {
                spans.push((s.span.0, self.section_end(si)));
            }
        }
        if spans.is_empty() {
            return None;
        }
        Some(cut(&self.file.data, &spans))
    }

    /// `--rename-section`; `None` se a seção não existe.
    pub fn rename_section(&self, old: &str, new: &KeyParts) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let mut pos = 0;
        let mut any = false;
        for s in &self.file.sections {
            if s.name == old {
                out.extend_from_slice(&self.file.data[pos..s.span.0]);
                let mut h = section_header(new);
                // Mantém o que vinha depois do `]` na mesma linha.
                let hdr = &self.file.data[s.span.0..s.span.1];
                if !hdr.ends_with(b"\n") {
                    h.pop();
                }
                out.extend_from_slice(&h);
                pos = s.span.1;
                any = true;
            }
        }
        if !any {
            return None;
        }
        out.extend_from_slice(&self.file.data[pos..]);
        Some(out)
    }
}

fn cut(data: &[u8], spans: &[(usize, usize)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pos = 0;
    for &(a, b) in spans {
        if a < pos {
            pos = pos.max(b);
            continue;
        }
        out.extend_from_slice(&data[pos..a]);
        pos = b;
    }
    out.extend_from_slice(&data[pos.min(data.len())..]);
    out
}

// ---- conjunto de arquivos ---------------------------------------------------------------------

/// Toda a configuração visível, em ordem de precedência crescente.
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub files: Vec<ConfigFile>,
}

impl Config {
    /// Todas as entradas, na ordem (a última vence).
    pub fn entries(&self) -> impl Iterator<Item = (&ConfigFile, &Entry)> {
        self.files.iter().flat_map(|f| f.entries.iter().map(move |e| (f, e)))
    }

    /// Valor cru da última ocorrência (`Some(None)` = presente sem `=`).
    pub fn raw(&self, key: &str) -> Option<Option<&[u8]>> {
        let canon = canonical_key(key);
        let mut found = None;
        for (_, e) in self.entries() {
            if e.key == canon {
                found = Some(e.value.as_deref());
            }
        }
        found
    }

    pub fn get_all(&self, key: &str) -> Vec<Option<&[u8]>> {
        let canon = canonical_key(key);
        self.entries().filter(|(_, e)| e.key == canon).map(|(_, e)| e.value.as_deref()).collect()
    }

    /// Valor como texto; booleano implícito vira `None` aqui (quem quer saber usa `raw`).
    pub fn get(&self, key: &str) -> Option<String> {
        match self.raw(key) {
            Some(Some(v)) => Some(String::from_utf8_lossy(v).into_owned()),
            _ => None,
        }
    }

    pub fn get_bytes(&self, key: &str) -> Option<Vec<u8>> {
        match self.raw(key) {
            Some(Some(v)) => Some(v.to_vec()),
            _ => None,
        }
    }

    /// Booleano com o erro do git pra valor inválido.
    pub fn get_bool(&self, key: &str) -> R<Option<bool>> {
        match self.raw(key) {
            None => Ok(None),
            Some(v) => match parse_bool(v) {
                Some(b) => Ok(Some(b)),
                None => Err(Fail::Fatal(format!(
                    "bad boolean config value '{}' for '{}'",
                    String::from_utf8_lossy(v.unwrap_or_default()),
                    key
                ))),
            },
        }
    }

    pub fn bool_or(&self, key: &str, default: bool) -> R<bool> {
        Ok(self.get_bool(key)?.unwrap_or(default))
    }

    pub fn get_int(&self, key: &str) -> R<Option<i64>> {
        match self.raw(key) {
            None => Ok(None),
            Some(None) => Err(Fail::Fatal(format!("missing value for '{key}'"))),
            Some(Some(v)) => match parse_int(v) {
                Some(n) => Ok(Some(n)),
                None => Err(Fail::Fatal(format!("bad numeric config value '{}' for '{}': invalid unit", String::from_utf8_lossy(v), key))),
            },
        }
    }
}

/// `git_parse_maybe_bool`.
pub fn parse_bool(v: Option<&[u8]>) -> Option<bool> {
    let Some(v) = v else { return Some(true) };
    let s = String::from_utf8_lossy(v).to_ascii_lowercase();
    match s.as_str() {
        "true" | "yes" | "on" => Some(true),
        "false" | "no" | "off" | "" => Some(false),
        _ => parse_int(v).map(|n| n != 0),
    }
}

/// Inteiro com sufixo `k`, `m`, `g`.
pub fn parse_int(v: &[u8]) -> Option<i64> {
    let s = std::str::from_utf8(v).ok()?.trim();
    if s.is_empty() {
        return None;
    }
    let (num, mult) = match s.as_bytes()[s.len() - 1].to_ascii_lowercase() {
        b'k' => (&s[..s.len() - 1], 1024i64),
        b'm' => (&s[..s.len() - 1], 1024 * 1024),
        b'g' => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s, 1),
    };
    let n: i64 = if let Some(h) = num.strip_prefix("0x").or_else(|| num.strip_prefix("0X")) {
        i64::from_str_radix(h, 16).ok()?
    } else if num.len() > 1 && num.starts_with('0') && num.bytes().all(|c| c.is_ascii_digit()) {
        i64::from_str_radix(&num[1..], 8).ok()?
    } else {
        num.parse().ok()?
    };
    n.checked_mul(mult)
}

/// Lê `GIT_CONFIG_PARAMETERS` (`'k'='v' 'k2'`), como o git passa `-c` pros filhos.
pub fn parse_config_parameters(s: &[u8]) -> Vec<(String, Option<Vec<u8>>)> {
    let mut out = Vec::new();
    let mut i = 0;
    let read_sq = |i: &mut usize| -> Option<Vec<u8>> {
        // Uma palavra em aspas simples do sq_quote: '...' com '\'' pra aspas internas.
        let mut w = Vec::new();
        let mut any = false;
        while *i < s.len() {
            match s[*i] {
                b'\'' => {
                    any = true;
                    *i += 1;
                    while *i < s.len() && s[*i] != b'\'' {
                        w.push(s[*i]);
                        *i += 1;
                    }
                    *i += 1;
                }
                b'\\' if *i + 1 < s.len() => {
                    w.push(s[*i + 1]);
                    *i += 2;
                    any = true;
                }
                _ => break,
            }
        }
        any.then_some(w)
    };
    while i < s.len() {
        while i < s.len() && s[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= s.len() {
            break;
        }
        let Some(first) = read_sq(&mut i) else { break };
        if s.get(i) == Some(&b'=') {
            i += 1;
            let v = read_sq(&mut i).unwrap_or_default();
            out.push((String::from_utf8_lossy(&first).into_owned(), Some(v)));
        } else {
            // Formato antigo: 'k=v' numa palavra só.
            match first.iter().position(|c| *c == b'=') {
                Some(eq) => out.push((String::from_utf8_lossy(&first[..eq]).into_owned(), Some(first[eq + 1..].to_vec()))),
                None => out.push((String::from_utf8_lossy(&first).into_owned(), None)),
            }
        }
    }
    out
}

/// `sq_quote`: `'texto'` com `'` virando `'\''`.
pub fn sq_quote(s: &[u8]) -> Vec<u8> {
    let mut out = vec![b'\''];
    for &c in s {
        if c == b'\'' || c == b'!' {
            out.extend_from_slice(b"'\\");
            out.push(c);
            out.push(b'\'');
        } else {
            out.push(c);
        }
    }
    out.push(b'\'');
    out
}

/// Arquivo de entradas da linha de comando (`-c k=v`).
pub fn command_line_file(items: &[(String, Option<Vec<u8>>)]) -> ConfigFile {
    let mut f = ConfigFile { path: Vec::new(), scope: Scope::Command, data: Vec::new(), entries: Vec::new(), sections: Vec::new(), command_line: true };
    for (k, v) in items {
        f.entries.push(Entry { key: canonical_key(k), value: v.clone(), span: (0, 0), section: 0 });
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> ConfigFile {
        ConfigFile::parse(b"x".to_vec(), Scope::Local, s.as_bytes().to_vec()).ok().unwrap()
    }

    #[test]
    fn values_and_sections() {
        let f = parse("[core]\n\tbare = false\n\tFoo = \" x;y \" ; c\n[Sec \"Sub\"]\n Key = a \\\n b\nflag\n[a.B]\nk=v # c\n");
        let kv: Vec<(String, Option<String>)> =
            f.entries.iter().map(|e| (e.key.clone(), e.value.as_ref().map(|v| String::from_utf8_lossy(v).into_owned()))).collect();
        assert_eq!(
            kv,
            vec![
                ("core.bare".into(), Some("false".into())),
                ("core.foo".into(), Some(" x;y ".into())),
                ("sec.Sub.key".into(), Some("a  b".into())),
                ("sec.Sub.flag".into(), None),
                ("a.b.k".into(), Some("v".into())),
            ]
        );
    }

    #[test]
    fn edit_set_and_unset() {
        let f = parse("[core]\n\tbare = false\n");
        let k = parse_key("user.Name").ok().unwrap();
        let Edit::Ok(out) = (Editor { file: &f }).set(&k, Some(b"A B"), false, false, None) else { panic!() };
        assert_eq!(String::from_utf8(out.clone()).unwrap(), "[core]\n\tbare = false\n[user]\n\tName = A B\n");
        let f2 = parse(std::str::from_utf8(&out).unwrap());
        let Edit::Ok(out2) = (Editor { file: &f2 }).unset(&k, false, None) else { panic!() };
        assert_eq!(String::from_utf8(out2).unwrap(), "[core]\n\tbare = false\n");
        let k = parse_key("core.Foo").ok().unwrap();
        let Edit::Ok(out) = (Editor { file: &f }).set(&k, Some(b" x;y "), false, false, None) else { panic!() };
        assert_eq!(String::from_utf8(out).unwrap(), "[core]\n\tbare = false\n\tFoo = \" x;y \"\n");
    }

    #[test]
    fn config_parameters() {
        let v = parse_config_parameters(b"'user.name'='A B' 'x.y'");
        assert_eq!(v[0], ("user.name".into(), Some(b"A B".to_vec())));
        assert_eq!(v[1], ("x.y".into(), None));
    }
}
