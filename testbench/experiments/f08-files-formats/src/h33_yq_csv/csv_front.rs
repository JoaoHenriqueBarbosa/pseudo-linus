//! Front-end de CSV sobre a crate `csv` 1.4. Não existe ferramenta GNU de CSV, então o mesmo leitor e
//! escritor imitam três referências do oráculo:
//! - `cut -d, -fLISTA` e `column -t -s,` (comparáveis byte a byte em CSV sem aspas);
//! - o módulo `csv` do Python 3.13 (leitura impressa como JSON e escrita com `csv.writer`), que é a
//!   referência de RFC 4180 disponível no oráculo.
//!
//! As configurações da crate são as que reproduzem a referência quando a crate permite (sem cabeçalho,
//! registros de tamanho variável, terminador escolhido); o que sobra de diferença é semântica da crate.

use harness::{Candidate, Invocation, Outcome};

use crate::common;

/// Programa Python dos casos de leitura (o front-end reconhece o texto literal).
pub const READ_PY: &str = "import csv, sys, json\nfor row in csv.reader(open(sys.argv[1], newline='', encoding='utf-8')):\n    print(json.dumps(row, ensure_ascii=False))\n";
/// Programa Python dos casos de escrita com terminador `\n`.
pub const WRITE_PY_LF: &str = "import csv, sys, json\nw = csv.writer(sys.stdout, lineterminator='\\n')\nfor line in sys.stdin:\n    w.writerow(json.loads(line))\n";
/// Programa Python dos casos de escrita com o terminador padrão (`\r\n`).
pub const WRITE_PY_DEFAULT: &str = "import csv, sys, json\nw = csv.writer(sys.stdout)\nfor line in sys.stdin:\n    w.writerow(json.loads(line))\n";

pub struct CsvCandidate;

impl Candidate for CsvCandidate {
    fn name(&self) -> String {
        "csv 1.4".into()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        let result = match inv.program() {
            Some("cut") => run_cut(inv),
            Some("column") => run_column(inv),
            Some("python3") => run_python(inv),
            _ => Err("programa não suportado".into()),
        };
        match result {
            Ok((stdout, exit)) => Outcome::exited(stdout, "", exit, inv.files.clone()),
            Err(why) => Outcome::unsupported(why),
        }
    }
}

fn input_bytes<'a>(inv: &'a Invocation, file: Option<&String>) -> Result<&'a [u8], String> {
    match file {
        None => Ok(&inv.stdin),
        Some(f) if f == "-" => Ok(&inv.stdin),
        Some(f) => common::fixture_file(inv, f).ok_or_else(|| format!("{f}: arquivo ausente")),
    }
}

/// Lê todos os registros com a configuração que mais se aproxima das referências.
pub fn read_records(data: &[u8]) -> Result<Vec<Vec<String>>, String> {
    let mut reader = csv::ReaderBuilder::new().has_headers(false).flexible(true).from_reader(data);
    let mut out = Vec::new();
    for rec in reader.byte_records() {
        let rec = rec.map_err(|e| e.to_string())?;
        out.push(rec.iter().map(|f| String::from_utf8_lossy(f).into_owned()).collect());
    }
    Ok(out)
}

/// Lista de campos do `cut -f` (1-based, intervalos abertos permitidos).
fn parse_field_list(spec: &str) -> Result<Vec<(usize, usize)>, String> {
    spec.split(',')
        .map(|part| {
            let parse = |s: &str| s.parse::<usize>().map_err(|_| format!("lista inválida: {spec}"));
            Ok(match part.split_once('-') {
                None => {
                    let n = parse(part)?;
                    (n, n)
                }
                Some(("", hi)) => (1, parse(hi)?),
                Some((lo, "")) => (parse(lo)?, usize::MAX),
                Some((lo, hi)) => (parse(lo)?, parse(hi)?),
            })
        })
        .collect()
}

fn run_cut(inv: &Invocation) -> Result<(String, i32), String> {
    let mut fields = None;
    let mut file = None;
    for arg in inv.args() {
        match arg.as_str() {
            "-d," => {}
            s if s.starts_with("-f") => fields = Some(parse_field_list(&s[2..])?),
            s if s.starts_with('-') && s != "-" => return Err(format!("opção de cut não suportada: {s}")),
            s => file = Some(s.to_string()),
        }
    }
    let ranges = fields.ok_or("cut sem -f")?;
    let wanted = |i: usize| ranges.iter().any(|(lo, hi)| (*lo..=*hi).contains(&(i + 1)));
    let mut out = String::new();
    for rec in read_records(input_bytes(inv, file.as_ref())?)? {
        if rec.len() == 1 {
            // Linha sem delimitador: o cut imprime a linha inteira.
            out.push_str(&rec[0]);
        } else {
            let picked: Vec<&str> =
                rec.iter().enumerate().filter(|(i, _)| wanted(*i)).map(|(_, f)| f.as_str()).collect();
            out.push_str(&picked.join(","));
        }
        out.push('\n');
    }
    Ok((out, 0))
}

fn run_column(inv: &Invocation) -> Result<(String, i32), String> {
    let mut file = None;
    for arg in inv.args() {
        match arg.as_str() {
            "-t" | "-s," => {}
            s if s.starts_with('-') && s != "-" => return Err(format!("opção de column não suportada: {s}")),
            s => file = Some(s.to_string()),
        }
    }
    let rows = read_records(input_bytes(inv, file.as_ref())?)?;
    let ncols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0usize; ncols];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let mut out = String::new();
    for row in &rows {
        let mut line = String::new();
        for (i, width) in widths.iter().enumerate() {
            let cell = row.get(i).map(String::as_str).unwrap_or("");
            line.push_str(cell);
            if i + 1 < ncols {
                line.push_str(&" ".repeat(width - cell.chars().count() + 2));
            }
        }
        out.push_str(&line);
        out.push('\n');
    }
    Ok((out, 0))
}

/// `json.dumps(row, ensure_ascii=False)` do Python.
pub fn python_json_row(row: &[String]) -> String {
    let cells: Vec<String> = row.iter().map(|c| python_json_str(c)).collect();
    format!("[{}]", cells.join(", "))
}

fn python_json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn run_python(inv: &Invocation) -> Result<(String, i32), String> {
    let args = inv.args();
    if args.first().map(String::as_str) != Some("-c") {
        return Err("python3 sem -c".into());
    }
    let program = args.get(1).map(String::as_str).unwrap_or("");
    if program == READ_PY {
        let file = args.get(2).ok_or("leitura sem arquivo")?;
        let mut out = String::new();
        for row in read_records(input_bytes(inv, Some(file))?)? {
            out.push_str(&python_json_row(&row));
            out.push('\n');
        }
        return Ok((out, 0));
    }
    let terminator = if program == WRITE_PY_LF {
        csv::Terminator::Any(b'\n')
    } else if program == WRITE_PY_DEFAULT {
        csv::Terminator::CRLF
    } else {
        return Err("programa Python desconhecido".into());
    };
    let mut writer = csv::WriterBuilder::new()
        .terminator(terminator)
        .quote_style(csv::QuoteStyle::Necessary)
        .flexible(true)
        .from_writer(Vec::new());
    for line in String::from_utf8_lossy(&inv.stdin).lines() {
        let row: Vec<String> = serde_json::from_str(line).map_err(|e| format!("json: {e}"))?;
        writer.write_record(&row).map_err(|e| e.to_string())?;
    }
    let bytes = writer.into_inner().map_err(|e| e.to_string())?;
    Ok((String::from_utf8_lossy(&bytes).into_owned(), 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_lists() {
        assert_eq!(parse_field_list("2").unwrap(), vec![(2, 2)]);
        assert_eq!(parse_field_list("1,3").unwrap(), vec![(1, 1), (3, 3)]);
        assert_eq!(parse_field_list("2-").unwrap(), vec![(2, usize::MAX)]);
        assert_eq!(parse_field_list("-2").unwrap(), vec![(1, 2)]);
    }

    #[test]
    fn python_json_format() {
        let row = vec!["a\"b".to_string(), "são\n".to_string(), String::new()];
        assert_eq!(python_json_row(&row), r#"["a\"b", "são\n", ""]"#);
    }

    #[test]
    fn reader_handles_quotes_and_ragged_rows() {
        let recs = read_records(b"a,\"b,c\"\nd\n").unwrap();
        assert_eq!(recs, vec![vec!["a".to_string(), "b,c".to_string()], vec!["d".to_string()]]);
    }
}
