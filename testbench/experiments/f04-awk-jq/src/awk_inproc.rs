//! `rawk-core` 0.6 em processo. A crate só expõe `Awk::new(programa).run(linhas, arquivo, FS)`, que
//! devolve as linhas de saída: não há `-v`, nem operandos de atribuição, nem vários arquivos, nem
//! saída sem quebra de linha. O adaptador faz o que o `rawk-cli` faz (junta as linhas com `\n`) e
//! marca como não suportado o que a API não permite expressar.

use harness::{Candidate, Entry, Invocation, Outcome};

pub struct RawkCore;

fn read_fixture(inv: &Invocation, path: &str) -> Result<Vec<u8>, String> {
    match inv.files.get(path) {
        Some(Entry::File { data: Some(d), .. }) => Ok(d.0.clone()),
        Some(_) => Err(format!("{path}: não é arquivo regular")),
        None => Err(format!("{path}: No such file or directory")),
    }
}

impl Candidate for RawkCore {
    fn name(&self) -> String {
        "rawk-core 0.6.0".into()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        if inv.script.is_some() {
            return Outcome::unsupported("rawk-core é biblioteca: não roda pipelines de shell");
        }
        if !matches!(inv.program(), Some("awk") | Some("gawk")) {
            return Outcome::unsupported("caso de outra ferramenta");
        }
        let args = inv.args();
        let mut fs: Option<String> = None;
        let mut program: Option<String> = None;
        let mut operands: Vec<String> = Vec::new();
        let mut i = 0;
        let mut opts_done = false;
        while i < args.len() {
            let a = &args[i];
            if !opts_done && program.is_none() {
                if a == "--" {
                    opts_done = true;
                    i += 1;
                    continue;
                }
                if a == "-F" {
                    let Some(v) = args.get(i + 1) else { return Outcome::unsupported("-F sem valor") };
                    fs = Some(v.clone());
                    i += 2;
                    continue;
                }
                if let Some(v) = a.strip_prefix("-F") {
                    fs = Some(v.to_string());
                    i += 1;
                    continue;
                }
                if a == "-f" {
                    let Some(path) = args.get(i + 1) else { return Outcome::unsupported("-f sem valor") };
                    match read_fixture(inv, path) {
                        Ok(src) => program = Some(String::from_utf8_lossy(&src).into_owned()),
                        Err(e) => return Outcome::exited("", format!("rawk: {e}\n"), 2, inv.files.clone()),
                    }
                    i += 2;
                    if args.get(i).is_some_and(|x| x == "-f") {
                        return Outcome::unsupported("rawk-core aceita um programa só (vários -f)");
                    }
                    continue;
                }
                if a.starts_with('-') && a.len() > 1 {
                    return Outcome::unsupported(format!("opção não suportada pela API do rawk-core: {a}"));
                }
            }
            if program.is_none() {
                program = Some(a.clone());
            } else {
                operands.push(a.clone());
            }
            i += 1;
        }
        let Some(program) = program else { return Outcome::unsupported("sem programa") };
        if operands.iter().any(|o| o.contains('=') && !o.starts_with('=')) {
            return Outcome::unsupported("operandos de atribuição (var=valor) não existem na API");
        }
        if operands.len() > 1 {
            return Outcome::unsupported("a API do rawk-core aceita um arquivo de entrada só");
        }
        let (input, filename) = match operands.first().map(String::as_str) {
            None | Some("-") => (inv.stdin.clone(), None),
            Some(f) => match read_fixture(inv, f) {
                Ok(d) => (d, Some(f.to_string())),
                Err(e) => return Outcome::exited("", format!("rawk: {e}\n"), 2, inv.files.clone()),
            },
        };
        let text = String::from_utf8_lossy(&input);
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        let awk = match rawk_core::awk::Awk::new(program) {
            Ok(a) => a,
            Err(e) => return Outcome::exited("", format!("Error: {e}\n"), 1, inv.files.clone()),
        };
        let (out_lines, runtime_error) = awk.run(lines, filename, fs);
        let mut stdout = String::new();
        for l in out_lines {
            stdout.push_str(&l);
            stdout.push('\n');
        }
        let stderr = runtime_error.map(|e| format!("rawk: {e}\n")).unwrap_or_default();
        Outcome::exited(stdout, stderr, 0, inv.files.clone())
    }
}
