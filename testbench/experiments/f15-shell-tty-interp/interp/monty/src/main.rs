//! Monty (subconjunto de Python da pydantic) sobre o protocolo comum do F17.
//!
//! O Monty não faz I/O nenhum por conta própria: todo acesso a arquivo, ambiente, relógio e entropia
//! sai do interpretador como `RunProgress::OsCall` e quem responde é o host. Aqui o host é o nosso
//! FS em memória (`Snippet::files`); o resto é negado com a exceção padrão do Monty
//! (`OsFunctionCall::on_no_handler`), menos o ambiente, que é o nosso (vazio).

use std::collections::BTreeMap;

use f15_interp_common::{Engine, Snippet, SnippetResult, serve};
use monty::{MontyRun, RunProgress};
use monty_types::{
    CompileOptions, ExcType, ExtFunctionResult, MontyException, MontyFileHandle, MontyObject, NameLookupResult,
    OsFunctionCall, PrintWriter, ResourceLimits, ResourceTracker,
};

struct MontyEngine;

/// Diretório de trabalho do trecho. O Monty normaliza todo caminho relativo pra absoluto a partir
/// dele antes de entregar a chamada ao host.
const CWD: &str = "/work";

/// Caminho como chave do nosso FS: relativo ao cwd quando está dentro dele, absoluto quando não.
fn key(path: &str) -> String {
    let rel = path.strip_prefix(CWD).and_then(|r| r.strip_prefix('/'));
    let p = rel.unwrap_or(path);
    p.strip_prefix("./").unwrap_or(p).to_string()
}

fn not_found(path: &str) -> ExtFunctionResult {
    ExtFunctionResult::Error(MontyException::new(
        ExcType::FileNotFoundError,
        Some(format!("[Errno 2] No such file or directory: '{path}'")),
    ))
}

/// Responde uma chamada de OS com o nosso FS em memória.
fn handle_os(call: OsFunctionCall, files: &mut BTreeMap<String, String>) -> ExtFunctionResult {
    match call {
        OsFunctionCall::Open(args) => {
            let path = args.path.as_str().to_string();
            let k = key(&path);
            let mode = args.mode;
            if mode.is_append() {
                files.entry(k).or_default();
            } else if mode.writable() {
                files.insert(k, String::new());
            } else if !files.contains_key(&k) {
                return not_found(&path);
            }
            MontyObject::file_handle(MontyFileHandle { path, mode, position: 0 }).into()
        }
        OsFunctionCall::ReadText(p) => match files.get(&key(p.as_str())) {
            Some(text) => MontyObject::string(text.clone()).into(),
            None => not_found(p.as_str()),
        },
        OsFunctionCall::ReadBytes(p) => match files.get(&key(p.as_str())) {
            Some(text) => MontyObject::bytes(text.as_bytes().to_vec()).into(),
            None => not_found(p.as_str()),
        },
        OsFunctionCall::WriteText(a) => {
            let n = a.data.chars().count() as i64;
            files.insert(key(a.path.as_str()), a.data);
            MontyObject::int(n).into()
        }
        OsFunctionCall::AppendText(a) => {
            let n = a.data.chars().count() as i64;
            files.entry(key(a.path.as_str())).or_default().push_str(&a.data);
            MontyObject::int(n).into()
        }
        OsFunctionCall::Exists(p) | OsFunctionCall::IsFile(p) => {
            MontyObject::bool(files.contains_key(&key(p.as_str()))).into()
        }
        OsFunctionCall::IsDir(_) | OsFunctionCall::IsSymlink(_) => MontyObject::bool(false).into(),
        // O ambiente do trecho é o nosso, e é vazio: nada do host vaza.
        OsFunctionCall::Getenv(_) => MontyObject::none().into(),
        OsFunctionCall::GetEnviron => MontyObject::dict(Vec::new()).into(),
        other => ExtFunctionResult::Error(other.on_no_handler()),
    }
}

impl Engine for MontyEngine {
    fn name(&self) -> &'static str {
        "monty"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn language(&self) -> &'static str {
        "python"
    }

    fn run(&self, snippet: &Snippet) -> SnippetResult {
        let mut files = snippet.files.clone();
        let mut out = String::new();
        let error = run_monty(&snippet.code, &mut files, &mut out).err();
        SnippetResult { id: snippet.id.clone(), stdout: out, error, files, elapsed_us: 0 }
    }

    fn notes(&self) -> Vec<String> {
        vec![
            "Todo I/O sai como RunProgress::OsCall; o host responde open/read/write com o FS em memória e nega o resto (on_no_handler).".into(),
            "os.getenv/os.environ respondem com o ambiente nosso (vazio).".into(),
        ]
    }
}

fn run_monty(code: &str, files: &mut BTreeMap<String, String>, out: &mut String) -> Result<(), String> {
    let mut runner = MontyRun::new(code.to_owned(), "main.py", Vec::new(), CompileOptions::default())
        .map_err(|e| e.to_string())?;
    runner.set_cwd(CWD);
    let limits = ResourceLimits::default();
    let mut progress = runner.start(Vec::new(), ResourceTracker::new(limits), PrintWriter::CollectString(out, None));
    loop {
        let step = progress.map_err(|e| e.to_string())?;
        progress = match step {
            RunProgress::Complete(_) => return Ok(()),
            RunProgress::OsCall(call) => {
                call.resume_with(PrintWriter::CollectString(out, None), |fc| handle_os(fc, files))
            }
            RunProgress::NameLookup(lookup) => {
                lookup.resume(NameLookupResult::Undefined, PrintWriter::CollectString(out, None))
            }
            RunProgress::FunctionCall(call) => {
                let exc = MontyException::new(ExcType::RuntimeError, Some("função externa não oferecida".into()));
                call.abort(exc, PrintWriter::CollectString(out, None))
            }
            RunProgress::ResolveFutures(f) => {
                let exc = MontyException::new(ExcType::RuntimeError, Some("futures não oferecidas".into()));
                f.abort(exc, PrintWriter::CollectString(out, None))
            }
        };
    }
}

fn main() {
    serve(&MontyEngine);
}
