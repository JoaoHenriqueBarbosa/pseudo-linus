//! Golden do `fetch` offline contra o JavaScriptCore do bun: `tests/golden/fetch_offline_bun.tsv` sai de
//! `scripts/gen-fetch-offline-golden.js` (bun 1.4.2). Cada linha é `JSON(programa)<TAB>JSON(valor de R)`. O programa roda
//! como `/app/main.js` sobre um `ModuleFs` em memória com as fixtures do cabeçalho do gerador (`hello.txt`, `empty.txt`,
//! `data.json`, `bin.dat`, `sub/inner.txt`, `no-read.txt` ilegível, `link.txt` e os dois nomes com caracteres especiais), e
//! o diretório de trabalho (`process.cwd()` no gerador) vale `/app`. O UUID de `createObjectURL` vira `<uuid>` na saída,
//! como no gerador.
//!
//! Fora de escopo (LACUNAS declaradas, cada uma com o motivo):
//! - programas que usam `require('fs')` (linhas 244 a 247) ou `process.on('exit')` (338, 354, 358): o módulo `fs` e o
//!   `process` do porte não existem ainda (a leitura preguiçosa do `file://` é coberta pelos casos que não escrevem no
//!   disco);
//!
//! Os programas com `setTimeout` e `setImmediate` rodam: o corredor esvazia as microtarefas e depois roda o laço de
//! eventos (`evaluate_cjs_program_with_fs_running_timers`). O quirk de `FILE:///x` (linha 268) está reproduzido e coberto,
//! com variantes, em `tests/fetch_network_bun_golden.rs`.
mod common;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use common::{guarded, json_string};
use zjsc::api::eval::evaluate_cjs_program_with_fs_running_timers;
use zjsc::api::module_probe::{EntryKind, ModuleFs};

const GOLDEN: &str = include_str!("golden/fetch_offline_bun.tsv");

/// Linhas fora de escopo, por índice (base 0): 244 a 247 usam o módulo `fs` por `require`; 338, 354 e 358 usam
/// `process.on('exit')`. Lista explícita, sem filtro por substring do fonte.
const OUT_OF_SCOPE_ROWS: &[usize] = &[244, 245, 246, 247, 338, 354, 358];

/// Árvore de arquivos com bytes crus (o `MemoryFs` guarda `String`, e `bin.dat` tem os bytes 0 a 255).
#[derive(Debug)]
struct FixtureFs {
    files: HashMap<String, Vec<u8>>,
    directories: HashSet<String>,
    unreadable: HashSet<String>,
    links: HashMap<String, String>,
}

impl FixtureFs {
    fn new() -> Self {
        let mut files: HashMap<String, Vec<u8>> = HashMap::new();
        files.insert("/app/hello.txt".into(), b"hello\n".to_vec());
        files.insert("/app/empty.txt".into(), Vec::new());
        files.insert("/app/data.json".into(), b"{\"a\":1}".to_vec());
        files.insert("/app/bin.dat".into(), (0..=255u8).collect());
        files.insert("/app/sub/inner.txt".into(), b"inner".to_vec());
        files.insert("/app/no-read.txt".into(), b"secret".to_vec());
        files.insert("/app/sp ace.txt".into(), b"spaced".to_vec());
        files.insert("/app/\u{fc}.txt".into(), b"uml".to_vec());
        let directories = ["/", "/app", "/app/sub"].iter().map(|path| path.to_string()).collect();
        let unreadable = ["/app/no-read.txt".to_string()].into_iter().collect();
        let links = [("/app/link.txt".to_string(), "/app/hello.txt".to_string())].into_iter().collect();
        FixtureFs { files, directories, unreadable, links }
    }

    fn resolve(&self, path: &str) -> String {
        let trimmed = if path.len() > 1 { path.trim_end_matches('/') } else { path };
        self.links.get(trimmed).cloned().unwrap_or_else(|| trimmed.to_owned())
    }
}

impl ModuleFs for FixtureFs {
    fn read_file(&self, path: &str) -> Option<String> {
        self.read_bytes(path).and_then(|bytes| String::from_utf8(bytes).ok())
    }

    fn read_bytes(&self, path: &str) -> Option<Vec<u8>> {
        let resolved = self.resolve(path);
        if self.unreadable.contains(&resolved) {
            return None;
        }
        self.files.get(&resolved).cloned()
    }

    fn stat(&self, path: &str) -> Option<EntryKind> {
        let resolved = self.resolve(path);
        if self.files.contains_key(&resolved) {
            Some(EntryKind::File)
        } else if self.directories.contains(&resolved) {
            Some(EntryKind::Directory)
        } else {
            None
        }
    }

    fn realpath(&self, path: &str) -> Option<String> {
        self.stat(path).map(|_| self.resolve(path))
    }
}

/// O UUID aleatório do `createObjectURL` vira `<uuid>`, como o gerador faz com a saída do bun.
fn mask_uuids(text: &str) -> String {
    let bytes = text.as_bytes();
    let shape = [8usize, 4, 4, 4, 12];
    let total = 36;
    let mut out = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if index + total <= bytes.len() && is_uuid(&bytes[index..index + total], &shape) {
            out.push_str("<uuid>");
            index += total;
        } else {
            let length = text[index..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&text[index..index + length]);
            index += length;
        }
    }
    out
}

fn is_uuid(window: &[u8], shape: &[usize]) -> bool {
    let mut position = 0;
    for (group, &length) in shape.iter().enumerate() {
        if group > 0 {
            if window[position] != b'-' {
                return false;
            }
            position += 1;
        }
        if !window[position..position + length].iter().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
            return false;
        }
        position += length;
    }
    true
}

#[test]
fn fetch_offline_matches_bun() {
    let mut failures = Vec::new();
    let (mut total, mut skipped) = (0, 0);
    for (row, line) in GOLDEN.lines().filter(|line| !line.is_empty()).enumerate() {
        let mut columns = line.splitn(3, '\t');
        let source = json_string(columns.next().expect("fonte"));
        let expected = json_string(columns.next().expect("resultado"));
        if OUT_OF_SCOPE_ROWS.contains(&row) {
            skipped += 1;
            continue;
        }
        total += 1;
        let program = source.replace("var DIR = process.cwd();", "var DIR = '/app';");
        let units: Vec<u16> = program.encode_utf16().collect();
        match guarded(|| evaluate_cjs_program_with_fs_running_timers(Rc::new(FixtureFs::new()), &units, "/app/main.js", "R", &[])) {
            Ok(actual) => {
                let actual = mask_uuids(&actual);
                if actual != expected {
                    failures.push(format!("linha {row}\n    esperado {expected:?}\n    veio     {actual:?}"));
                }
            }
            Err(reason) => failures.push(format!("linha {row}\n    esperado {expected:?}\n    {reason}")),
        }
    }
    assert!(total >= 250, "só {total} casos rodaram ({skipped} fora de escopo)");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
