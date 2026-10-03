//! Front-end do yq (imitando o yq 3.4.3 do kislyuk), igual pra todos os candidatos: argumentos,
//! leitura dos arquivos, conversão YAML -> JSON com a semântica do Python (`json.dumps`), filtro e
//! impressão no formato do jq 1.7.1. Só a camada YAML (e, no candidato yqr, o motor do filtro) muda.

use harness::{Candidate, Invocation, Outcome};
use jaq_core::ValT;
use jaq_json::{Num, Val};

use super::jqout::{self, Indent, JqStyle};
use super::yaml_layers::{Node, YamlLayer, noyalib_load, val_to_node, yqr_to_node};
use crate::common;

/// Motor do filtro jq.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// jaq-core + jaq-std + jaq-json 3.x.
    Jaq,
    /// Motor próprio do yqr 0.8 (subconjunto de jq), com o noyalib como camada YAML.
    Yqr,
}

pub struct YqCandidate {
    pub layer: Box<dyn YamlLayer>,
    pub engine: Engine,
}

impl YqCandidate {
    pub fn display_name(&self) -> String {
        match self.engine {
            Engine::Jaq => self.layer.name().to_string(),
            Engine::Yqr => "yqr 0.8 (motor próprio + noyalib)".to_string(),
        }
    }
}

const USAGE: &str = "usage: yq [-h] [--yaml-output] [--yaml-roundtrip]
          [--yaml-output-grammar-version {1.1,1.2}] [--width WIDTH]
          [--indentless-lists] [--explicit-start] [--explicit-end]
          [--in-place] [--version]
          [jq_filter] [files ...]
";

#[derive(Default, Debug)]
struct Args {
    yaml_out: bool,
    raw: bool,
    join: bool,
    compact: bool,
    tab: bool,
    indent: Option<usize>,
    sort_keys: bool,
    ascii: bool,
    exit_status: bool,
    null_input: bool,
    slurp: bool,
    named: Vec<(String, Val)>,
    filter: Option<String>,
    files: Vec<String>,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut i = 0;
    let mut positional = Vec::new();
    while i < args.len() {
        let arg = args[i].as_str();
        let mut take = |n: usize| -> Result<Vec<String>, String> {
            let vals = args.get(i + 1..i + 1 + n).ok_or_else(|| format!("{arg}: faltam argumentos"))?.to_vec();
            i += n;
            Ok(vals)
        };
        match arg {
            "-y" | "--yaml-output" => a.yaml_out = true,
            "-Y" | "--yaml-roundtrip" => a.yaml_out = true,
            "-r" | "--raw-output" => a.raw = true,
            "-j" | "--join-output" => {
                a.raw = true;
                a.join = true
            }
            "-c" | "--compact-output" => a.compact = true,
            "--tab" => a.tab = true,
            "--indent" => {
                let v = take(1)?;
                a.indent = Some(v[0].parse().map_err(|_| "--indent inválido".to_string())?);
            }
            "-S" | "--sort-keys" => a.sort_keys = true,
            "-a" | "--ascii-output" => a.ascii = true,
            "-e" | "--exit-status" => a.exit_status = true,
            "-n" | "--null-input" => a.null_input = true,
            "-s" | "--slurp" => a.slurp = true,
            "-M" | "--monochrome-output" | "-C" | "--color-output" => {}
            "--arg" => {
                let v = take(2)?;
                a.named.push((v[0].clone(), Val::from(v[1].clone())));
            }
            "--argjson" => {
                let v = take(2)?;
                let val = jaq_json::read::parse_single(v[1].as_bytes()).map_err(|e| format!("--argjson: {e}"))?;
                a.named.push((v[0].clone(), val));
            }
            "-" => positional.push(arg.to_string()),
            s if s.starts_with("--") => return Err(format!("opção não suportada: {s}")),
            s if s.starts_with('-') && s.len() > 2 && s[1..].chars().all(|c| "rjcSaensMCyY".contains(c)) => {
                for c in s[1..].chars() {
                    match c {
                        'r' => a.raw = true,
                        'j' => {
                            a.raw = true;
                            a.join = true
                        }
                        'c' => a.compact = true,
                        'S' => a.sort_keys = true,
                        'a' => a.ascii = true,
                        'e' => a.exit_status = true,
                        'n' => a.null_input = true,
                        's' => a.slurp = true,
                        'y' | 'Y' => a.yaml_out = true,
                        _ => {}
                    }
                }
            }
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("opção não suportada: {s}")),
            s => positional.push(s.to_string()),
        }
        i += 1;
    }
    let mut positional = positional.into_iter();
    a.filter = positional.next();
    a.files = positional.collect();
    Ok(a)
}

/// Saída acumulada de uma execução.
#[derive(Default)]
struct Run {
    stdout: String,
    stderr: String,
    /// Status de erro de execução do jq (5) ou do yq (1), se houve.
    exit: Option<i32>,
    outputs: usize,
    last: Option<Val>,
    yaml_docs: usize,
}

/// Conversão YAML -> JSON com a semântica do yq (Python `json.dumps` sobre o que o PyYAML carregou).
pub fn node_to_val(n: &Node) -> Result<Val, String> {
    Ok(match n {
        Node::Null => Val::Null,
        Node::Bool(b) => Val::Bool(*b),
        Node::Int(s) => Val::Num(Num::from_str_radix(s, 10).ok_or_else(|| format!("inteiro inválido {s}"))?),
        Node::Float(f) if f.is_finite() => Val::from_num(&jqout::python_repr(*f)).map_err(|e| e.to_string())?,
        Node::Float(f) => Val::from(*f),
        Node::Str(s) => Val::from(s.clone()),
        Node::Seq(items) => items.iter().map(node_to_val).collect::<Result<Val, _>>()?,
        Node::Map(pairs) => {
            // dict do Python: chave repetida mantém a posição e o texto da primeira e o valor da última,
            // e 1, 1.0 e True são a mesma chave.
            let mut entries: indexmap::IndexMap<String, (String, Val)> = indexmap::IndexMap::new();
            for (k, v) in pairs {
                let text = python_key(k)?;
                let value = node_to_val(v)?;
                match entries.get_mut(&python_key_identity(k)) {
                    Some(slot) => slot.1 = value,
                    None => {
                        entries.insert(python_key_identity(k), (text, value));
                    }
                }
            }
            let mut map = jaq_json::Map::default();
            for (_, (text, value)) in entries {
                map.insert(Val::from(text), value);
            }
            Val::obj(map)
        }
    })
}

/// Identidade de chave de dict do Python (igualdade numérica entre bool, int e float).
fn python_key_identity(k: &Node) -> String {
    match k {
        Node::Bool(b) => format!("n:{}", u8::from(*b)),
        Node::Int(s) => format!("n:{}", s.trim_start_matches('+')),
        Node::Float(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e18 => format!("n:{}", *f as i64),
        Node::Float(f) => format!("f:{}", jqout::python_repr(*f)),
        Node::Null => "null".into(),
        Node::Str(s) => format!("s:{s}"),
        Node::Seq(_) | Node::Map(_) => "composite".into(),
    }
}

/// Mesma conversão sem passar os floats pelo `repr` do Python (usado só pra emitir YAML).
pub fn node_to_val_plain(n: &Node) -> Val {
    match n {
        Node::Float(f) => Val::from(*f),
        Node::Seq(items) => items.iter().map(node_to_val_plain).collect(),
        Node::Map(pairs) => {
            let mut map = jaq_json::Map::default();
            for (k, v) in pairs {
                map.insert(Val::from(python_key(k).unwrap_or_default()), node_to_val_plain(v));
            }
            Val::obj(map)
        }
        other => node_to_val(other).unwrap_or(Val::Null),
    }
}

/// Chave de objeto como o `json.dumps` do Python escreve.
fn python_key(k: &Node) -> Result<String, String> {
    Ok(match k {
        Node::Str(s) => s.clone(),
        Node::Int(s) => s.trim_start_matches('+').to_string(),
        Node::Float(f) => jqout::python_repr(*f),
        Node::Bool(true) => "true".into(),
        Node::Bool(false) => "false".into(),
        Node::Null => "null".into(),
        Node::Seq(_) => return Err("TypeError: unhashable type: 'list'".into()),
        Node::Map(_) => return Err("TypeError: unhashable type: 'dict'".into()),
    })
}

impl Candidate for YqCandidate {
    fn name(&self) -> String {
        self.display_name()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        if inv.program() != Some("yq") {
            return Outcome::unsupported("só yq");
        }
        if std::env::var_os("F08_YAML_TRACE").is_some() {
            eprintln!("  caso {}", inv.case_id);
        }
        let args = match parse_args(inv.args()) {
            Ok(a) => a,
            Err(e) => return Outcome::unsupported(e),
        };
        // O argparse do yq abre os arquivos antes de tudo.
        for f in &args.files {
            if f != "-" && common_read(inv, f).is_none() {
                let stderr = format!(
                    "{USAGE}yq: error: argument files: can't open '{f}': [Errno 2] No such file or directory: '{f}'\n"
                );
                return Outcome::exited("", stderr, 2, inv.files.clone());
            }
        }
        let filter_text = args.filter.clone().unwrap_or_else(|| ".".into());
        let mut run = Run::default();
        let result = match self.engine {
            Engine::Jaq => self.run_jaq(inv, &args, &filter_text, &mut run),
            Engine::Yqr => self.run_yqr(inv, &args, &filter_text, &mut run),
        };
        if let Err(unsupported) = result {
            return Outcome::unsupported(unsupported);
        }
        let exit = match run.exit {
            Some(code) => code,
            None if args.exit_status => match (&run.last, run.outputs) {
                (_, 0) => 4,
                (Some(Val::Null) | Some(Val::Bool(false)), _) => 1,
                _ => 0,
            },
            None => 0,
        };
        Outcome::exited(run.stdout, run.stderr, exit, inv.files.clone())
    }
}

fn common_read<'a>(inv: &'a Invocation, path: &str) -> Option<&'a [u8]> {
    common::fixture_file(inv, path)
}

/// Fonte de documentos: cada arquivo (ou stdin) vira uma lista de documentos ou um erro de YAML.
fn input_texts(inv: &Invocation, args: &Args) -> Vec<String> {
    if args.files.is_empty() {
        return vec![String::from_utf8_lossy(&inv.stdin).into_owned()];
    }
    args.files
        .iter()
        .map(|f| {
            if f == "-" {
                String::from_utf8_lossy(&inv.stdin).into_owned()
            } else {
                String::from_utf8_lossy(common_read(inv, f).unwrap_or_default()).into_owned()
            }
        })
        .collect()
}

impl YqCandidate {
    fn emit_output(&self, args: &Args, run: &mut Run, v: &Val) {
        run.outputs += 1;
        run.last = Some(v.clone());
        if args.yaml_out {
            if run.yaml_docs > 0 {
                run.stdout.push_str("---\n");
            }
            run.yaml_docs += 1;
            match self.layer.emit(&val_to_node(v)) {
                Ok(text) => run.stdout.push_str(&text),
                Err(e) => {
                    run.stderr.push_str(&format!("yq: Error running jq: {e}.\n"));
                    run.exit = Some(1);
                }
            }
            return;
        }
        match v {
            Val::TStr(b) if args.raw => run.stdout.push_str(&String::from_utf8_lossy(b)),
            _ => {
                let indent = if args.compact {
                    Indent::Compact
                } else if args.tab {
                    Indent::Tab
                } else {
                    Indent::Spaces(args.indent.unwrap_or(2))
                };
                let style = JqStyle { indent, sort_keys: args.sort_keys, ascii: args.ascii };
                jqout::write_value(&mut run.stdout, v, &style);
            }
        }
        if !args.join {
            run.stdout.push('\n');
        }
    }

    /// Lê os documentos de todos os arquivos, já convertidos pra JSON. Erro de YAML encerra com 1.
    fn load_docs(&self, inv: &Invocation, args: &Args, run: &mut Run) -> Option<Vec<Val>> {
        let mut out = Vec::new();
        for text in input_texts(inv, args) {
            let nodes = match self.engine {
                Engine::Jaq => self.layer.load(&text),
                Engine::Yqr => noyalib_load(&text).map(|docs| docs.iter().map(yqr_to_node).collect()),
            };
            let nodes = match nodes {
                Ok(n) => n,
                Err(e) => {
                    run.stderr.push_str(&format!("yq: Error running jq: {}.\n", e.trim_end()));
                    run.exit = Some(1);
                    return None;
                }
            };
            for node in &nodes {
                match node_to_val(node) {
                    Ok(v) => out.push(v),
                    Err(e) => {
                        run.stderr.push_str(&format!("yq: Error running jq: {e}.\n"));
                        run.exit = Some(1);
                        return None;
                    }
                }
            }
        }
        Some(out)
    }

    fn run_jaq(&self, inv: &Invocation, args: &Args, filter_text: &str, run: &mut Run) -> Result<(), String> {
        use jaq_core::load::{Arena, File, Loader};
        use jaq_core::{Compiler, Ctx, Vars, data};

        let defs = jaq_core::defs().chain(jaq_std::defs()).chain(jaq_json::defs());
        let funs = jaq_core::funs().chain(jaq_std::funs()).chain(jaq_json::funs());
        let loader = Loader::new(defs);
        let arena = Arena::default();
        let var_names: Vec<String> = args.named.iter().map(|(n, _)| format!("${n}")).collect();
        let compiled = loader
            .load(&arena, File { code: filter_text, path: () })
            .map_err(|e| format!("{e:?}"))
            .and_then(|modules| {
                Compiler::default()
                    .with_funs(funs)
                    .with_global_vars(var_names.iter().map(String::as_str))
                    .compile(modules)
                    .map_err(|e| format!("{e:?}"))
            });
        let filter = match compiled {
            Ok(f) => f,
            Err(e) => {
                run.stderr.push_str(&format!("jq: error: {e}\njq: 1 compile error\n"));
                run.exit = Some(3);
                return Ok(());
            }
        };
        let vars: Vec<Val> = args.named.iter().map(|(_, v)| v.clone()).collect();
        let ctx = Ctx::<data::JustLut<Val>>::new(&filter.lut, Vars::new(vars));
        let inputs: Vec<Val> = if args.null_input {
            vec![Val::Null]
        } else {
            let Some(docs) = self.load_docs(inv, args, run) else { return Ok(()) };
            if args.slurp { vec![docs.into_iter().collect()] } else { docs }
        };
        for (idx, input) in inputs.into_iter().enumerate() {
            for out in filter.id.run((ctx.clone(), input)) {
                match out {
                    Ok(v) => self.emit_output(args, run, &v),
                    Err(exn) => {
                        let msg = match exn.get_err() {
                            Ok(err) => error_text(err),
                            Err(exn) => match exn.get_halt() {
                                Ok(code) => {
                                    run.exit = Some(code);
                                    return Ok(());
                                }
                                Err(_) => "exceção desconhecida".into(),
                            },
                        };
                        let at = if args.null_input { "<unknown>".to_string() } else { format!("<stdin>:{}", idx + 1) };
                        run.stderr.push_str(&format!("jq: error (at {at}): {msg}\n"));
                        run.exit = Some(5);
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn run_yqr(&self, inv: &Invocation, args: &Args, filter_text: &str, run: &mut Run) -> Result<(), String> {
        if !args.named.is_empty() {
            return Err("yqr não tem --arg/--argjson".into());
        }
        let ast = match yqr::parser::parse(filter_text) {
            Ok(a) => a,
            Err(e) => {
                run.stderr.push_str(&format!("jq: error: {e}\njq: 1 compile error\n"));
                run.exit = Some(3);
                return Ok(());
            }
        };
        let inputs: Vec<Val> = if args.null_input {
            vec![Val::Null]
        } else {
            let Some(docs) = self.load_docs(inv, args, run) else { return Ok(()) };
            if args.slurp { vec![docs.into_iter().collect()] } else { docs }
        };
        for (idx, input) in inputs.into_iter().enumerate() {
            let value = super::yaml_layers::node_to_yqr(&val_to_node(&input));
            match yqr::eval::eval(&ast, &value) {
                Ok(outs) => {
                    for o in outs {
                        match node_to_val(&yqr_to_node(&o)) {
                            Ok(v) => self.emit_output(args, run, &v),
                            Err(e) => {
                                run.stderr.push_str(&format!("jq: error (at <stdin>:{}): {e}\n", idx + 1));
                                run.exit = Some(5);
                            }
                        }
                    }
                }
                Err(e) => {
                    run.stderr.push_str(&format!("jq: error (at <stdin>:{}): {e}\n", idx + 1));
                    run.exit = Some(5);
                }
            }
        }
        Ok(())
    }
}

/// Mensagem de erro de execução no estilo do jq: `error("x")` mostra só o texto.
fn error_text(err: jaq_core::Error<Val>) -> String {
    let text = err.to_string();
    match err.into_val() {
        Val::TStr(b) => String::from_utf8_lossy(&b).into_owned(),
        _ => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_split_filter_files_and_flags() {
        let argv: Vec<String> =
            ["-c", "--arg", "v", "3", "{a: $v}", "m.yaml", "-"].iter().map(|s| s.to_string()).collect();
        let a = parse_args(&argv).unwrap();
        assert!(a.compact);
        assert_eq!(a.filter.as_deref(), Some("{a: $v}"));
        assert_eq!(a.files, vec!["m.yaml".to_string(), "-".to_string()]);
        assert_eq!(a.named.len(), 1);
    }

    #[test]
    fn python_semantics_for_keys_and_floats() {
        let node = Node::Map(vec![
            (Node::Int("1".into()), Node::Float(1e16)),
            (Node::Bool(false), Node::Float(1000.0)),
            (Node::Null, Node::Float(f64::INFINITY)),
        ]);
        let v = node_to_val(&node).unwrap();
        let mut s = String::new();
        jqout::write_value(&mut s, &v, &JqStyle { indent: Indent::Compact, ..JqStyle::default() });
        assert_eq!(s, r#"{"1":1E+16,"false":1000.0,"null":1.7976931348623157e+308}"#);
        let bad = Node::Map(vec![(Node::Seq(vec![]), Node::Null)]);
        assert!(node_to_val(&bad).unwrap_err().contains("unhashable type: 'list'"));
    }
}
