//! O `package.json` do projeto: o que `bun run` lê dele (nome, versão e a tabela `scripts`, na ordem do arquivo).
//!
//! Medido no bun 1.4.2: o arquivo vem do diretório de trabalho ou do primeiro ancestral que o tem; o parser aceita
//! comentários e vírgula final; um arquivo que não parseia vale como ausente; só contam os scripts de valor texto não
//! vazio (número, `null`, lista e `""` não aparecem na listagem nem rodam).

/// Um `package.json` lido.
pub struct Package {
    /// O diretório do arquivo: onde os scripts rodam.
    pub dir: String,
    pub name: Option<String>,
    pub version: Option<String>,
    /// Os scripts de valor texto não vazio, na ordem do arquivo.
    pub scripts: Vec<(String, String)>,
}

impl Package {
    /// O texto do script `name`, se existe.
    pub fn script(&self, name: &str) -> Option<&str> {
        self.scripts.iter().find(|(key, _)| key == name).map(|(_, text)| text.as_str())
    }
}

/// Onde termina o comentário (`//` até o fim da linha, `/* */`) que começa em `index`, se há um.
fn comment_end(chars: &[char], index: usize) -> Option<usize> {
    match (chars.get(index), chars.get(index + 1)) {
        (Some('/'), Some('/')) => Some(chars[index..].iter().position(|c| *c == '\n').map_or(chars.len(), |offset| index + offset)),
        (Some('/'), Some('*')) => {
            let close = (index + 2..chars.len().saturating_sub(1)).find(|at| chars[*at] == '*' && chars[*at + 1] == '/');
            Some(close.map_or(chars.len(), |at| at + 2))
        }
        _ => None,
    }
}

/// O primeiro índice a partir de `index` que não é espaço nem comentário.
fn skip_trivia(chars: &[char], mut index: usize) -> usize {
    while index < chars.len() {
        if chars[index].is_whitespace() {
            index += 1;
        } else if let Some(end) = comment_end(chars, index) {
            index = end;
        } else {
            break;
        }
    }
    index
}

/// Tira o que o parser do bun aceita e o JSON estrito não: os comentários e a vírgula que antecede `}` ou `]`.
fn strip_extensions(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let (mut index, mut in_string) = (0, false);
    while index < chars.len() {
        let current = chars[index];
        if in_string {
            out.push(current);
            if current == '\\' && index + 1 < chars.len() {
                out.push(chars[index + 1]);
                index += 1;
            } else if current == '"' {
                in_string = false;
            }
        } else if let Some(end) = comment_end(&chars, index) {
            index = end;
            continue;
        } else if current == ',' && matches!(chars.get(skip_trivia(&chars, index + 1)), Some('}' | ']')) {
            // A vírgula final some.
        } else {
            in_string = current == '"';
            out.push(current);
        }
        index += 1;
    }
    out
}

/// Lê `text` como o bun lê um `package.json`; `None` se não parseia.
fn parse(dir: &str, text: &str) -> Option<Package> {
    let value: serde_json::Value = serde_json::from_str(&strip_extensions(text)).ok()?;
    let text_of = |key: &str| value.get(key).and_then(|field| field.as_str()).map(str::to_string);
    let scripts = value
        .get("scripts")
        .and_then(|scripts| scripts.as_object())
        .map(|table| table.iter().filter_map(|(key, body)| body.as_str().filter(|body| !body.is_empty()).map(|body| (key.clone(), body.to_string()))).collect())
        .unwrap_or_default();
    Some(Package { dir: dir.to_string(), name: text_of("name"), version: text_of("version"), scripts })
}

/// O `package.json` de `cwd` ou do primeiro ancestral que o tem.
pub fn find(cwd: &str) -> Option<Package> {
    let mut dir = cwd.trim_end_matches('/').to_string();
    loop {
        let path = format!("{dir}/package.json");
        if sysio::fs::exists(&path) {
            let text = sysio::fs::read_to_string(&path).ok()?;
            return parse(if dir.is_empty() { "/" } else { &dir }, &text);
        }
        if dir.is_empty() {
            return None;
        }
        dir.truncate(dir.rfind('/').unwrap_or(0));
    }
}
