//! `python3` mínimo: não há interpretador de Python aqui. O programa reconhece, por inteiro e pelo
//! texto, os programas `python3 -c` que a bancada usa pra medir o módulo `csv` (Debian 13, Python
//! 3.13.5) e os executa sobre a porta de `_csv.c` e de `json` deste diretório:
//!
//! - leitura: `csv.reader(open(sys.argv[1], newline='', encoding='utf-8'))`, cada registro em JSON
//!   (`json.dumps(row, ensure_ascii=False)`);
//! - escrita: `csv.writer(sys.stdout[, lineterminator='...'])`, uma linha JSON de `sys.stdin` por
//!   registro.
//!
//! Erros que o Python levantaria nesses programas (arquivo ausente, UTF-8 inválido, `_csv.Error`,
//! JSON inválido) saem como o traceback dele, com o mesmo texto e a mesma saída 1. Qualquer outro
//! programa, script, `-m` ou o REPL falha com uma mensagem honesta de que não há interpretador
//! (saída 1): não existe um Python parcial que fingisse rodar o que não roda.
//!
//! Ordem de saída: como o Python, o stdout vai a um buffer de 8192 bytes e o traceback sai antes
//! do descarregamento final.
//!
//! Limites conhecidos: nomes de arquivo viram texto com perda (UTF-8 inválido no argumento);
//! `str.isprintable` é aproximado fora do Latin-1 (ver `repr`); erro de escrita no stdout
//! (`BrokenPipeError`) é ignorado em silêncio.

pub mod csv;
pub mod json;
pub mod repr;
pub mod text;
pub mod trace;

use std::ffi::OsString;
use std::io::{Read, Write};

use sysabi::Ctx;

use crate::py::{JsonError, Py, json_error_text, to_json};

use self::csv::{CsvError, Reader, format_row};
use self::json::{LoadError, loads};
use self::repr::{repr as py_repr, to_str};
use self::text::{
    DecodeError, LineReader, decode_surrogateescape, encode_surrogateescape, translate_newlines,
};
use self::trace::{JSON_DECODER, JSON_INIT, Traceback};

/// A versão que o oráculo reporta.
const VERSION_LINE: &str = "Python 3.13.5\n";

const USAGE_TAIL: &str = "usage: python3 [option] ... [-c cmd | -m mod | file | -] [arg] ...\nTry `python -h' for more information.\n";

const READER_SOURCE: &str = "import csv, sys, json\nfor row in csv.reader(open(sys.argv[1], newline='', encoding='utf-8')):\n    print(json.dumps(row, ensure_ascii=False))";
const READER_LINE: &str = "for row in csv.reader(open(sys.argv[1], newline='', encoding='utf-8')):";
const OPEN_CALL: &str = "open(sys.argv[1], newline='', encoding='utf-8')";
const READER_CALL: &str = "csv.reader(open(sys.argv[1], newline='', encoding='utf-8'))";

const WRITER_LINE: &str = "w.writerow(json.loads(line))";

const JSON_ERROR_CLASS: &str = "json.decoder.JSONDecodeError";

/// Tamanho do buffer do `sys.stdout` (`io.DEFAULT_BUFFER_SIZE`).
const STDOUT_BUFFER: usize = 8192;

/// Os programas reconhecidos.
#[derive(Debug, PartialEq, Eq)]
enum Program {
    Reader,
    /// `lineterminator` do `csv.writer` (`\r\n` quando o programa não o diz).
    Writer { terminator: String },
}

/// O `str` que um literal Python simples (aspas simples ou duplas, escapes `\n \r \t \\ \' \"`)
/// escreve; `None` pra qualquer outra forma.
fn parse_string_literal(lit: &str) -> Option<String> {
    let chars: Vec<char> = lit.chars().collect();
    let quote = *chars.first()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    if chars.len() < 2 || chars[chars.len() - 1] != quote {
        return None;
    }
    let body = &chars[1..chars.len() - 1];
    let mut out = String::new();
    let mut i = 0;
    while i < body.len() {
        let c = body[i];
        if c == quote {
            return None;
        }
        if c == '\\' {
            i += 1;
            let escaped = match *body.get(i)? {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '\\' => '\\',
                '\'' => '\'',
                '"' => '"',
                _ => return None,
            };
            out.push(escaped);
        } else {
            out.push(c);
        }
        i += 1;
    }
    Some(out)
}

/// O `lineterminator` da segunda linha do programa de escrita.
fn parse_writer_line(line: &str) -> Option<String> {
    let rest = line.strip_prefix("w = csv.writer(sys.stdout")?;
    if rest == ")" {
        return Some("\r\n".to_string());
    }
    let literal = rest.strip_prefix(", lineterminator=")?.strip_suffix(')')?;
    parse_string_literal(literal)
}

/// Reconhece o programa pelo texto (as quebras de linha do fim não contam, como no Python).
fn recognize(source: &str) -> Option<Program> {
    let src = source.trim_end_matches('\n');
    if src == READER_SOURCE {
        return Some(Program::Reader);
    }
    let lines: Vec<&str> = src.split('\n').collect();
    if lines.len() != 4
        || lines[0] != "import csv, sys, json"
        || lines[2] != "for line in sys.stdin:"
        || lines[3] != "    w.writerow(json.loads(line))"
    {
        return None;
    }
    Some(Program::Writer { terminator: parse_writer_line(lines[1])? })
}

/// O stdout do Python: buffer de 8192 bytes, descarregado quando o próximo bloco não cabe.
struct Buffered {
    buf: Vec<u8>,
}

impl Buffered {
    fn new() -> Buffered {
        Buffered { buf: Vec::new() }
    }

    fn write(&mut self, data: &[u8]) {
        if self.buf.len() + data.len() > STDOUT_BUFFER {
            self.flush();
        }
        if data.len() >= STDOUT_BUFFER {
            emit(data);
        } else {
            self.buf.extend_from_slice(data);
        }
    }

    fn flush(&mut self) {
        if !self.buf.is_empty() {
            emit(&self.buf);
            self.buf.clear();
        }
    }
}

fn emit(data: &[u8]) {
    let mut out = sysio::io::stdout();
    let _ = out.write_all(data);
    let _ = sysio::io::flush_stdout();
}

/// Exceção sem tratamento: o traceback vai ao stderr antes do descarregamento do stdout, e a saída
/// é 1.
fn fail(out: &mut Buffered, traceback: &str) -> i32 {
    sysio::io::eprint_fmt(format_args!("{traceback}"));
    out.flush();
    1
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    sysio::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv: Vec<String> = args.iter().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let mut i = 0;
    while i < argv.len() {
        let arg = argv[i].as_str();
        match arg {
            "-V" | "--version" => {
                emit(VERSION_LINE.as_bytes());
                return 0;
            }
            "-c" => {
                let Some(source) = argv.get(i + 1) else {
                    sysio::io::eprint_fmt(format_args!("Argument expected for the -c option\n{USAGE_TAIL}"));
                    return 2;
                };
                return run_command(source, &argv[i + 2..]);
            }
            // Opções que não mudam o que estes programas escrevem.
            "-u" | "-B" | "-E" | "-s" | "-S" | "-I" | "-O" | "-OO" | "-q" => i += 1,
            joined if joined.starts_with("-c") => {
                return run_command(&joined[2..], &argv[i + 1..]);
            }
            _ => break,
        }
    }
    unsupported()
}

fn unsupported() -> i32 {
    sysio::io::eprint_fmt(format_args!(
        "python3: this system has no Python interpreter; only the csv.reader and csv.writer programs of the test bench run under `python3 -c`\n"
    ));
    1
}

fn run_command(source: &str, extra: &[String]) -> i32 {
    match recognize(source) {
        Some(Program::Reader) => run_reader(extra),
        Some(Program::Writer { terminator }) => run_writer(&terminator),
        None => unsupported(),
    }
}

// ---- leitura ----

/// O frame do módulo na linha 2 do programa de leitura, marcado sob `expr`.
fn reader_frame(expr: &str, split: usize) -> Traceback {
    Traceback::new().marked("<string>", 2, "<module>", READER_LINE, expr, split)
}

/// Classe e mensagem do `OSError` que o `open()` levantaria.
fn os_error(e: &std::io::Error, name: &str) -> (&'static str, String) {
    let errno = e.raw_os_error().unwrap_or(5);
    let class = match errno {
        1 | 13 => "PermissionError",
        2 => "FileNotFoundError",
        20 => "NotADirectoryError",
        21 => "IsADirectoryError",
        _ => "OSError",
    };
    let message = format!("[Errno {errno}] {}: {}", sysio::errno::strerror(e), py_repr(&Py::Str(name.to_string())));
    (class, message)
}

fn decode_failure(e: &DecodeError) -> String {
    reader_frame(READER_CALL, 10).bare("<frozen codecs>", 325, "decode").finish("UnicodeDecodeError", &e.message)
}

fn csv_failure(e: &CsvError) -> String {
    reader_frame(READER_CALL, 10).finish("_csv.Error", &e.0)
}

/// `print(json.dumps(row, ensure_ascii=False))`.
fn print_row(out: &mut Buffered, row: Vec<String>) {
    let list = Py::List(row.into_iter().map(Py::Str).collect());
    let mut line = String::new();
    // Uma lista de textos sempre serializa.
    let _ = to_json(&list, &mut line);
    line.push('\n');
    out.write(line.as_bytes());
}

fn run_reader(extra: &[String]) -> i32 {
    let mut out = Buffered::new();
    let Some(name) = extra.first() else {
        return fail(&mut out, &reader_frame("sys.argv[1]", 8).finish("IndexError", "list index out of range"));
    };
    let data = match sysio::fs::read(name) {
        Ok(d) => d,
        Err(e) => {
            let (class, message) = os_error(&e, name);
            return fail(&mut out, &reader_frame(OPEN_CALL, 4).finish(class, &message));
        }
    };
    let mut lines = LineReader::new(&data);
    let mut reader = Reader::new();
    loop {
        match lines.next_line() {
            Err(e) => return fail(&mut out, &decode_failure(&e)),
            Ok(Some(line)) => match reader.feed_line(&line) {
                Err(e) => return fail(&mut out, &csv_failure(&e)),
                Ok(Some(row)) => print_row(&mut out, row),
                Ok(None) => {}
            },
            Ok(None) => {
                if let Some(row) = reader.finish() {
                    print_row(&mut out, row);
                }
                break;
            }
        }
    }
    out.flush();
    0
}

// ---- escrita ----

/// O frame do módulo na linha 4 do programa de escrita, marcado sob `expr`.
fn writer_frame(expr: &str) -> Traceback {
    Traceback::new().marked("<string>", 4, "<module>", WRITER_LINE, expr, 10)
}

fn loads_frame(tb: Traceback) -> Traceback {
    tb.marked(JSON_INIT, 346, "loads", "return _default_decoder.decode(s)", "_default_decoder.decode(s)", 23)
}

fn decode_frame(tb: Traceback) -> Traceback {
    tb.marked(
        JSON_DECODER,
        345,
        "decode",
        "obj, end = self.raw_decode(s, idx=_w(s, 0).end())",
        "self.raw_decode(s, idx=_w(s, 0).end())",
        15,
    )
}

fn scan_frame(tb: Traceback) -> Traceback {
    tb.marked(JSON_DECODER, 361, "raw_decode", "obj, end = self.scan_once(s, idx)", "self.scan_once(s, idx)", 14)
}

/// O traceback de um `json.loads(line)` que falhou.
fn json_failure(e: &LoadError, line: &[char]) -> String {
    let user = writer_frame("json.loads(line)");
    let message = |msg: &str, pos: usize| json_error_text(line, &JsonError { msg: msg.to_string(), pos });
    match e {
        LoadError::Bom => {
            let text = message("Unexpected UTF-8 BOM (decode using utf-8-sig)", 0);
            user.plain(
                JSON_INIT,
                335,
                "loads",
                "    raise JSONDecodeError(\"Unexpected UTF-8 BOM (decode using utf-8-sig)\",\n                          s, 0)\n",
            )
            .finish(JSON_ERROR_CLASS, &text)
        }
        LoadError::Extra { pos } => loads_frame(user)
            .plain(JSON_DECODER, 348, "decode", "    raise JSONDecodeError(\"Extra data\", s, end)\n")
            .finish(JSON_ERROR_CLASS, &message("Extra data", *pos)),
        LoadError::Stop { pos } => decode_frame(loads_frame(user))
            .plain(
                JSON_DECODER,
                363,
                "raw_decode",
                "    raise JSONDecodeError(\"Expecting value\", s, err.value) from None\n",
            )
            .finish(JSON_ERROR_CLASS, &message("Expecting value", *pos)),
        LoadError::Scan { msg, pos } => {
            scan_frame(decode_frame(loads_frame(user))).finish(JSON_ERROR_CLASS, &message(msg, *pos))
        }
        LoadError::Value(text) => scan_frame(decode_frame(loads_frame(user))).finish("ValueError", text),
        LoadError::Recursion(kind) => scan_frame(decode_frame(loads_frame(user))).finish(
            "RecursionError",
            &format!("maximum recursion depth exceeded while decoding a JSON {kind} from a unicode string"),
        ),
    }
}

/// O campo como o `csv.writer` o converte: `None` é vazio, o resto passa por `str()`.
fn field_text(v: &Py) -> String {
    match v {
        Py::None => String::new(),
        other => to_str(other),
    }
}

/// Os campos de `writerow(obj)`: itera `obj`. O que não é iterável vira `_csv.Error`.
fn row_fields(row: &Py) -> Result<Vec<String>, String> {
    match row {
        Py::List(items) => Ok(items.iter().map(field_text).collect()),
        Py::Str(s) => Ok(s.chars().map(String::from).collect()),
        Py::Dict(pairs) => Ok(pairs.iter().map(|(k, _)| field_text(k)).collect()),
        other => Err(format!("iterable expected, not {}", other.type_name())),
    }
}

fn run_writer(terminator: &str) -> i32 {
    let mut out = Buffered::new();
    let mut data: Vec<u8> = Vec::new();
    let _ = sysio::io::stdin().read_to_end(&mut data);
    let input = translate_newlines(decode_surrogateescape(&data));
    for line in input.split_inclusive(|c| *c == '\n') {
        let row = match loads(line) {
            Ok(v) => v,
            Err(e) => return fail(&mut out, &json_failure(&e, line)),
        };
        let fields = match row_fields(&row) {
            Ok(f) => f,
            Err(message) => return fail(&mut out, &writer_frame(WRITER_LINE).finish("_csv.Error", &message)),
        };
        let record = format_row(&fields, terminator);
        match encode_surrogateescape(&record) {
            Ok(bytes) => out.write(&bytes),
            Err(e) => {
                return fail(&mut out, &writer_frame(WRITER_LINE).finish("UnicodeEncodeError", &e.message()));
            }
        }
    }
    out.flush();
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_bench_programs() {
        let reader = "import csv, sys, json\nfor row in csv.reader(open(sys.argv[1], newline='', encoding='utf-8')):\n    print(json.dumps(row, ensure_ascii=False))\n";
        assert_eq!(recognize(reader), Some(Program::Reader));
        let writer = "import csv, sys, json\nw = csv.writer(sys.stdout, lineterminator='\\n')\nfor line in sys.stdin:\n    w.writerow(json.loads(line))\n";
        assert_eq!(recognize(writer), Some(Program::Writer { terminator: "\n".to_string() }));
        let default = "import csv, sys, json\nw = csv.writer(sys.stdout)\nfor line in sys.stdin:\n    w.writerow(json.loads(line))\n";
        assert_eq!(recognize(default), Some(Program::Writer { terminator: "\r\n".to_string() }));
        assert_eq!(recognize("print(1)"), None);
    }

    #[test]
    fn string_literals() {
        assert_eq!(parse_string_literal("'\\r\\n'"), Some("\r\n".to_string()));
        assert_eq!(parse_string_literal("\"x\""), Some("x".to_string()));
        assert_eq!(parse_string_literal("'a'b'"), None);
        assert_eq!(parse_string_literal("'\\q'"), None);
    }

    #[test]
    fn row_fields_follow_iteration() {
        assert_eq!(row_fields(&Py::Str("abc".to_string())).unwrap(), vec!["a", "b", "c"]);
        assert_eq!(row_fields(&Py::None).unwrap_err(), "iterable expected, not NoneType");
    }
}
