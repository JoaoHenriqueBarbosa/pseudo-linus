//! Índice dos arquivos do diretório e a renderização das páginas da Simple API.

use crate::zip;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use ul_common::codec::hex_lower;
use ul_common::hash::Sha256;

const API_VERSION: &str = "1.1";

/// Formato de resposta negociado pelo cabeçalho Accept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Json,
    HtmlV1,
    Html,
}

impl Format {
    pub(crate) fn content_type(self) -> &'static str {
        match self {
            Format::Json => "application/vnd.pypi.simple.v1+json",
            Format::HtmlV1 => "application/vnd.pypi.simple.v1+html",
            Format::Html => "text/html",
        }
    }
}

pub(crate) struct CoreMetadata {
    pub bytes: Vec<u8>,
    pub sha256: String,
}

pub(crate) struct Entry {
    pub filename: String,
    pub path: PathBuf,
    pub size: u64,
    pub sha256: String,
    pub version: String,
    pub requires_python: Option<String>,
    pub metadata: Option<CoreMetadata>,
}

#[derive(Default)]
pub(crate) struct Index {
    /// Nome normalizado do projeto para os nomes de arquivo, em ordem.
    projects: BTreeMap<String, BTreeSet<String>>,
    /// Arquivo servido em `/files/` pelo nome.
    pub(crate) files: BTreeMap<String, Entry>,
}

/// Normalização do PEP 503: `re.sub(r"[-_.]+", "-", name).lower()`.
pub(crate) fn normalize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut in_sep = false;
    for c in name.chars() {
        if matches!(c, '-' | '_' | '.') {
            if !in_sep {
                out.push('-');
                in_sep = true;
            }
        } else {
            in_sep = false;
            out.extend(c.to_lowercase());
        }
    }
    out
}

pub(crate) fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub(crate) fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = b.get(i + 1..i + 3)?;
            if !h.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            out.push(u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn hash_file(path: &Path) -> io::Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((hex_lower(&hasher.finalize()), size))
}

/// Valor do primeiro cabeçalho `key` do METADATA (RFC 822, termina na linha em branco).
fn header_value(meta: &[u8], key: &str) -> Option<String> {
    let text = String::from_utf8_lossy(meta);
    for line in text.lines() {
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':')
            && k.eq_ignore_ascii_case(key)
        {
            let v = v.trim();
            return (!v.is_empty()).then(|| v.to_string());
        }
    }
    None
}

fn wheel_metadata(path: &Path) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    zip::read_entry(&mut file, |name| {
        name.strip_suffix("/METADATA")
            .is_some_and(|dir| dir.ends_with(".dist-info") && !dir.contains('/'))
    })
    .ok()
    .flatten()
}

/// Nome e versão de um arquivo que não é wheel: `nome-versao.tar.gz` ou `.zip`.
fn sdist_name_version(filename: &str) -> (String, String) {
    let stem = filename
        .strip_suffix(".tar.gz")
        .or_else(|| filename.strip_suffix(".zip"))
        .unwrap_or(filename);
    match stem.rsplit_once('-') {
        Some((n, v)) => (n.to_string(), v.to_string()),
        None => (stem.to_string(), String::new()),
    }
}

fn is_distribution(name: &str) -> bool {
    name.ends_with(".whl") || name.ends_with(".tar.gz") || name.ends_with(".zip")
}

impl Index {
    pub(crate) fn load(dir: &Path) -> io::Result<Index> {
        let mut names = Vec::new();
        for item in std::fs::read_dir(dir)? {
            let item = item?;
            let Ok(name) = item.file_name().into_string() else {
                continue;
            };
            if is_distribution(&name) && std::fs::metadata(item.path())?.is_file() {
                names.push(name);
            }
        }
        names.sort();
        let mut index = Index::default();
        for name in names {
            index.add(dir.join(&name), name)?;
        }
        Ok(index)
    }

    fn add(&mut self, path: PathBuf, filename: String) -> io::Result<()> {
        let (sha256, size) = hash_file(&path)?;
        let mut project;
        let mut version;
        let mut requires_python = None;
        let mut metadata = None;
        if filename.ends_with(".whl") {
            let mut parts = filename.split('-');
            project = parts.next().unwrap_or_default().to_string();
            version = parts.next().unwrap_or_default().to_string();
            if let Some(bytes) = wheel_metadata(&path) {
                if let Some(n) = header_value(&bytes, "Name") {
                    project = n;
                }
                if let Some(v) = header_value(&bytes, "Version") {
                    version = v;
                }
                requires_python = header_value(&bytes, "Requires-Python");
                metadata = Some(CoreMetadata {
                    sha256: hex_lower(&ul_common::hash::sha256(&bytes)),
                    bytes,
                });
            }
        } else {
            (project, version) = sdist_name_version(&filename);
        }
        self.projects
            .entry(normalize(&project))
            .or_default()
            .insert(filename.clone());
        self.files.insert(
            filename.clone(),
            Entry {
                filename,
                path,
                size,
                sha256,
                version,
                requires_python,
                metadata,
            },
        );
        Ok(())
    }

    pub(crate) fn render_root(&self, format: Format) -> String {
        if format == Format::Json {
            let projects: Vec<String> = self
                .projects
                .keys()
                .map(|p| format!("{{\"name\":{}}}", json_string(p)))
                .collect();
            return format!(
                "{{\"meta\":{{\"api-version\":\"{API_VERSION}\"}},\"projects\":[{}]}}",
                projects.join(",")
            );
        }
        let mut out = html_head("Simple index");
        for name in self.projects.keys() {
            out.push_str(&format!(
                "    <a href=\"/simple/{}/\">{}</a><br/>\n",
                percent_encode(name),
                html_escape(name)
            ));
        }
        out.push_str("  </body>\n</html>\n");
        out
    }

    pub(crate) fn render_project(&self, normalized: &str, format: Format) -> Option<String> {
        let entries: Vec<&Entry> = self
            .projects
            .get(normalized)?
            .iter()
            .filter_map(|f| self.files.get(f))
            .collect();
        Some(if format == Format::Json {
            project_json(normalized, &entries)
        } else {
            project_html(normalized, &entries)
        })
    }
}

fn html_head(title: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html>\n  <head>\n    <meta name=\"pypi:repository-version\" content=\"{API_VERSION}\">\n    <title>{t}</title>\n  </head>\n  <body>\n    <h1>{t}</h1>\n",
        t = html_escape(title)
    )
}

fn project_html(name: &str, entries: &[&Entry]) -> String {
    let mut out = html_head(&format!("Links for {name}"));
    for e in entries {
        out.push_str(&format!(
            "    <a href=\"../../files/{}#sha256={}\"",
            percent_encode(&e.filename),
            e.sha256
        ));
        if let Some(rp) = &e.requires_python {
            out.push_str(&format!(" data-requires-python=\"{}\"", html_escape(rp)));
        }
        if let Some(m) = &e.metadata {
            out.push_str(&format!(
                " data-dist-info-metadata=\"sha256={h}\" data-core-metadata=\"sha256={h}\"",
                h = m.sha256
            ));
        }
        out.push_str(&format!(">{}</a><br/>\n", html_escape(&e.filename)));
    }
    out.push_str("  </body>\n</html>\n");
    out
}

fn project_json(name: &str, entries: &[&Entry]) -> String {
    let files: Vec<String> = entries
        .iter()
        .map(|e| {
            let mut f = format!(
                "{{\"filename\":{},\"url\":{},\"hashes\":{{\"sha256\":\"{}\"}},\"size\":{}",
                json_string(&e.filename),
                json_string(&format!("../../files/{}", percent_encode(&e.filename))),
                e.sha256,
                e.size
            );
            if let Some(rp) = &e.requires_python {
                f.push_str(&format!(",\"requires-python\":{}", json_string(rp)));
            }
            if let Some(m) = &e.metadata {
                f.push_str(&format!(
                    ",\"core-metadata\":{{\"sha256\":\"{h}\"}},\"data-dist-info-metadata\":{{\"sha256\":\"{h}\"}}",
                    h = m.sha256
                ));
            }
            f.push('}');
            f
        })
        .collect();
    let versions: BTreeSet<&str> = entries
        .iter()
        .map(|e| e.version.as_str())
        .filter(|v| !v.is_empty())
        .collect();
    let versions: Vec<String> = versions.into_iter().map(json_string).collect();
    format!(
        "{{\"meta\":{{\"api-version\":\"{API_VERSION}\"}},\"name\":{},\"files\":[{}],\"versions\":[{}]}}",
        json_string(name),
        files.join(","),
        versions.join(",")
    )
}
