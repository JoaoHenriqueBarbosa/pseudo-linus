//! Camada mínima do CLI `sqlite3` (modo list), igual pros três motores.
//!
//! O que é do CLI e não do motor: leitura de stdin linha a linha até o comando ficar completo
//! (`sqlite3_complete`), separação dos comandos de um buffer, a moldura das mensagens de erro
//! ("Parse error near line N:", "Error: in prepare, ..."), o trecho com `^--- error here`, o código de
//! saída, a conversão de valores em texto no modo list, os comandos de ponto (`.tables`, `.schema`,
//! `.headers`) e o desenho do `EXPLAIN QUERY PLAN`. Os modos csv/json/column ficam de fora de propósito:
//! o caso que os usa é contado como não suportado.

use harness::MemTree;

use crate::shell::{Ctx, rel};

/// Um valor de coluna, já convertido pelo motor pro tipo de armazenamento do SQLite.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Null,
    Integer(i64),
    Real(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Prepare,
    Step,
}

/// Erro devolvido pelo motor. `code` é o código primário do SQLite (1 = SQLITE_ERROR, 19 = CONSTRAINT...).
#[derive(Clone, Debug)]
pub struct EngineError {
    pub phase: Phase,
    pub message: String,
    /// Deslocamento em bytes do token com erro dentro do texto passado ao motor, quando ele informa.
    pub offset: Option<usize>,
    pub code: i32,
}

impl EngineError {
    pub fn prepare(message: impl Into<String>, offset: Option<usize>) -> EngineError {
        EngineError { phase: Phase::Prepare, message: message.into(), offset, code: 1 }
    }

    pub fn step(message: impl Into<String>, code: i32) -> EngineError {
        EngineError { phase: Phase::Step, message: message.into(), offset: None, code }
    }
}

/// Uma conexão aberta, do ponto de vista do CLI.
pub trait Session {
    /// Executa um comando SQL completo (o texto começa no primeiro caractere não branco e pode ter
    /// comentários antes). `on_row` recebe os nomes das colunas e os valores de cada linha.
    fn execute(&mut self, sql: &str, on_row: &mut dyn FnMut(&[String], &[Cell])) -> Result<(), EngineError>;

    /// Fecha a conexão e persiste o banco no FS do caso. `abrupt` imita o processo saindo sem
    /// `sqlite3_close` (o que o sqlite3 faz quando um comando da linha de comando falha): o que estiver
    /// no FS naquele instante (inclusive um journal quente) é o que fica.
    fn close(self: Box<Self>, fs: &mut MemTree, abrupt: bool) -> Result<(), String>;
}

/// Um motor que sabe abrir um banco do FS do caso.
pub trait Backend {
    fn name(&self) -> String;

    /// `path` relativo ao caso, ou `None` pra banco em memória. O arquivo já existe quando `path` é `Some`.
    fn open(&mut self, fs: &mut MemTree, path: Option<&str>, readonly: bool) -> Result<Box<dyn Session>, String>;
}

/// Modos de saída que a camada mínima não implementa.
pub const UNSUPPORTED_MODES: &[&str] =
    &["-csv", "-json", "-column", "-line", "-box", "-table", "-markdown", "-html", "-quote", "-tabs", "-ascii", "-list"];

// ---------------------------------------------------------------------------------------------
// sqlite3_complete e separação de comandos
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Tk {
    Semi,
    Ws,
    Other,
    Explain,
    Create,
    Temp,
    Trigger,
    End,
}

/// Tabela de transição de `sqlite3_complete` (complete.c).
const TRANS: [[u8; 8]; 8] = [
    [1, 0, 2, 3, 4, 2, 2, 2],
    [1, 1, 2, 3, 4, 2, 2, 2],
    [1, 2, 2, 2, 2, 2, 2, 2],
    [1, 3, 3, 2, 4, 2, 2, 2],
    [1, 4, 2, 2, 2, 4, 5, 2],
    [6, 5, 5, 5, 5, 5, 5, 5],
    [6, 6, 5, 5, 5, 5, 5, 7],
    [1, 7, 5, 5, 5, 5, 5, 5],
];

fn id_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80
}

/// Resultado da varredura: fronteiras (índice logo depois de cada `;` que fecha um comando) e se o
/// texto termina completo.
pub struct Scan {
    pub boundaries: Vec<usize>,
    pub complete: bool,
}

/// Porte de `sqlite3_complete`, devolvendo também onde cada comando termina.
pub fn scan(sql: &str) -> Scan {
    let z = sql.as_bytes();
    let mut state: u8 = 0;
    let mut i = 0;
    let mut boundaries = Vec::new();
    while i < z.len() {
        let c = z[i];
        let token = match c {
            b';' => {
                i += 1;
                Tk::Semi
            }
            b' ' | b'\r' | b'\t' | b'\n' | 0x0c => {
                i += 1;
                Tk::Ws
            }
            b'/' => {
                if z.get(i + 1) != Some(&b'*') {
                    i += 1;
                    Tk::Other
                } else {
                    let mut j = i + 2;
                    while j < z.len() && !(z[j] == b'*' && z.get(j + 1) == Some(&b'/')) {
                        j += 1;
                    }
                    if j >= z.len() {
                        return Scan { boundaries, complete: false };
                    }
                    i = j + 2;
                    Tk::Ws
                }
            }
            b'-' => {
                if z.get(i + 1) != Some(&b'-') {
                    i += 1;
                    Tk::Other
                } else {
                    while i < z.len() && z[i] != b'\n' {
                        i += 1;
                    }
                    if i >= z.len() {
                        return Scan { boundaries, complete: state == 1 };
                    }
                    Tk::Ws
                }
            }
            b'[' | b'`' | b'"' | b'\'' => {
                let close = if c == b'[' { b']' } else { c };
                let mut j = i + 1;
                while j < z.len() && z[j] != close {
                    j += 1;
                }
                if j >= z.len() {
                    return Scan { boundaries, complete: false };
                }
                i = j + 1;
                Tk::Other
            }
            _ if id_char(c) => {
                let start = i;
                while i < z.len() && id_char(z[i]) {
                    i += 1;
                }
                let word = sql[start..i].to_ascii_lowercase();
                match word.as_str() {
                    "create" => Tk::Create,
                    "trigger" => Tk::Trigger,
                    "temp" | "temporary" => Tk::Temp,
                    "end" => Tk::End,
                    "explain" => Tk::Explain,
                    _ => Tk::Other,
                }
            }
            _ => {
                i += 1;
                Tk::Other
            }
        };
        let next = TRANS[state as usize][token as usize];
        if token == Tk::Semi && next == 1 {
            boundaries.push(i);
        }
        state = next;
    }
    Scan { boundaries, complete: state == 1 }
}

/// `true` quando o texto só tem espaço, comentários e `;`.
pub fn is_blank_sql(sql: &str) -> bool {
    let z = sql.as_bytes();
    let mut i = 0;
    while i < z.len() {
        match z[i] {
            b' ' | b'\t' | b'\n' | b'\r' | 0x0c | b';' => i += 1,
            b'-' if z.get(i + 1) == Some(&b'-') => {
                while i < z.len() && z[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if z.get(i + 1) == Some(&b'*') => {
                let mut j = i + 2;
                while j < z.len() && !(z[j] == b'*' && z.get(j + 1) == Some(&b'/')) {
                    j += 1;
                }
                i = j + 2;
            }
            _ => return false,
        }
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Conversão de valores em texto (sqlite3_column_text do 3.46: REAL vira "%!.15g")
// ---------------------------------------------------------------------------------------------

/// Formata um REAL como o SQLite 3.46 faz em `sqlite3_column_text` (`%!.15g`).
pub fn fmt_real(f: f64) -> String {
    if f.is_nan() {
        return String::new();
    }
    if f.is_infinite() {
        return if f > 0.0 { "Inf".into() } else { "-Inf".into() };
    }
    if f == 0.0 {
        return "0.0".into();
    }
    // 15 dígitos significativos, arredondados corretamente.
    let sci = format!("{:.14e}", f);
    let (mant, exp) = sci.split_once('e').expect("formato científico");
    let exp: i32 = exp.parse().expect("expoente");
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let sign = if neg { "-" } else { "" };
    if !(-4..15).contains(&exp) {
        let mut m = digits.trim_end_matches('0').to_string();
        if m.is_empty() {
            m.push('0');
        }
        let (head, tail) = m.split_at(1);
        let tail = if tail.is_empty() { "0" } else { tail };
        let esign = if exp < 0 { '-' } else { '+' };
        format!("{sign}{head}.{tail}e{esign}{:02}", exp.abs())
    } else if exp >= 0 {
        let int_len = (exp + 1) as usize;
        let (int_part, frac) = digits.split_at(int_len.min(digits.len()));
        let frac = frac.trim_end_matches('0');
        let frac = if frac.is_empty() { "0" } else { frac };
        format!("{sign}{int_part}.{frac}")
    } else {
        let zeros = "0".repeat((-exp - 1) as usize);
        let frac = format!("{zeros}{}", digits.trim_end_matches('0'));
        format!("{sign}0.{frac}")
    }
}

fn cell_text(cell: &Cell) -> Vec<u8> {
    let cut = |b: &[u8]| -> Vec<u8> {
        // O CLI imprime com %s: o texto acaba no primeiro NUL.
        match b.iter().position(|&x| x == 0) {
            Some(p) => b[..p].to_vec(),
            None => b.to_vec(),
        }
    };
    match cell {
        Cell::Null => Vec::new(),
        Cell::Integer(i) => i.to_string().into_bytes(),
        Cell::Real(f) => fmt_real(*f).into_bytes(),
        Cell::Text(t) => cut(t),
        Cell::Blob(b) => cut(b),
    }
}

// ---------------------------------------------------------------------------------------------
// O programa
// ---------------------------------------------------------------------------------------------

struct Opts {
    headers: bool,
    separator: Vec<u8>,
    bail: bool,
}

/// Contexto de erro igual ao `shell_error_context` do shell.c.
fn error_context(sql: &str, offset: Option<usize>) -> String {
    let Some(mut off) = offset else { return String::new() };
    let mut z = sql.as_bytes();
    if off >= z.len() {
        return String::new();
    }
    while off > 50 {
        off -= 1;
        z = &z[1..];
        while !z.is_empty() && (z[0] & 0xc0) == 0x80 {
            z = &z[1..];
            off -= 1;
        }
    }
    let mut len = z.len();
    if len > 78 {
        len = 78;
        while len > 0 && (z[len] & 0xc0) == 0x80 {
            len -= 1;
        }
    }
    let code: Vec<u8> = z[..len].iter().map(|&c| if c.is_ascii_whitespace() || c == 0x0b { b' ' } else { c }).collect();
    let code = String::from_utf8_lossy(&code);
    if off < 25 {
        format!("\n  {code}\n  {}^--- error here", " ".repeat(off))
    } else {
        format!("\n  {code}\n  {}error here ---^", " ".repeat(off - 14))
    }
}

struct Runner<'a> {
    session: Box<dyn Session>,
    opts: Opts,
    out: &'a mut Vec<u8>,
}

/// Erro de um buffer: o texto já com moldura de fase e o código.
struct ExecError {
    /// "in prepare, ..." ou "stepping, ..."
    message: String,
    rc: i32,
}

impl Runner<'_> {
    /// `shell_exec`: roda todos os comandos do buffer até o primeiro erro.
    fn shell_exec(&mut self, buf: &str) -> Result<(), ExecError> {
        let scan = scan(buf);
        let mut start = 0;
        let mut ends = scan.boundaries.clone();
        if ends.last().copied() != Some(buf.len()) {
            ends.push(buf.len());
        }
        for end in ends {
            let seg = &buf[start..end];
            start = end;
            if is_blank_sql(seg) {
                continue;
            }
            let lead = seg.len() - seg.trim_start().len();
            let stmt = &seg[lead..];
            let rest = &buf[end - seg.len() + lead..];
            self.run_statement(stmt, rest)?;
        }
        Ok(())
    }

    fn run_statement(&mut self, stmt: &str, rest: &str) -> Result<(), ExecError> {
        let eqp = is_eqp(stmt);
        let mut header_done = false;
        let mut eqp_rows: Vec<(i64, i64, String)> = Vec::new();
        let opts = &self.opts;
        let out = &mut *self.out;
        let result = self.session.execute(stmt, &mut |cols, cells| {
            if eqp {
                let id = match cells.first() {
                    Some(Cell::Integer(i)) => *i,
                    _ => 0,
                };
                let parent = match cells.get(1) {
                    Some(Cell::Integer(i)) => *i,
                    _ => 0,
                };
                let detail = cells.get(3).map(|c| String::from_utf8_lossy(&cell_text(c)).into_owned()).unwrap_or_default();
                eqp_rows.push((id, parent, detail));
                return;
            }
            if opts.headers && !header_done {
                for (i, c) in cols.iter().enumerate() {
                    if i > 0 {
                        out.extend_from_slice(&opts.separator);
                    }
                    out.extend_from_slice(c.as_bytes());
                }
                out.push(b'\n');
                header_done = true;
            }
            for (i, c) in cells.iter().enumerate() {
                if i > 0 {
                    out.extend_from_slice(&opts.separator);
                }
                out.extend_from_slice(&cell_text(c));
            }
            out.push(b'\n');
        });
        if eqp && !eqp_rows.is_empty() {
            render_eqp(&eqp_rows, self.out);
        }
        match result {
            Ok(()) => Ok(()),
            Err(e) => {
                let message = match e.phase {
                    Phase::Prepare => {
                        let mut m = format!("in prepare, {}", e.message);
                        if e.code > 1 {
                            m.push_str(&format!(" ({})", e.code));
                        }
                        // O deslocamento do motor é relativo ao comando; o trecho mostrado vai até o fim do buffer.
                        m.push_str(&error_context(rest, e.offset));
                        m
                    }
                    Phase::Step => {
                        let mut m = format!("stepping, {}", e.message);
                        if e.code > 1 {
                            m.push_str(&format!(" ({})", e.code));
                        }
                        m
                    }
                };
                Err(ExecError { message, rc: e.code.max(1) })
            }
        }
    }

    /// Comando de ponto. Devolve `Err(mensagem)` em erro.
    fn meta(&mut self, line: &str) -> Result<bool, String> {
        let args: Vec<&str> = line.split_whitespace().collect();
        let cmd = args[0].trim_start_matches('.');
        match cmd {
            "headers" | "header" => {
                self.opts.headers = matches!(args.get(1).copied(), Some("on" | "1" | "yes" | "true"));
            }
            "separator" => {
                if let Some(s) = args.get(1) {
                    self.opts.separator = s.as_bytes().to_vec();
                }
            }
            "bail" => self.opts.bail = matches!(args.get(1).copied(), Some("on" | "1" | "yes" | "true")),
            "mode" if matches!(args.get(1).copied(), Some("list") | None) => {}
            "quit" | "exit" => return Ok(true),
            "tables" => self.dot_tables(args.get(1).copied()),
            "schema" => self.dot_schema(args.get(1).copied()),
            _ => {
                return Err(format!(
                    "Error: unknown command or invalid arguments:  \"{cmd}\". Enter \".help\" for help"
                ));
            }
        }
        Ok(false)
    }

    fn query_strings(&mut self, sql: &str) -> Vec<Vec<String>> {
        let mut rows = Vec::new();
        let _ = self.session.execute(sql, &mut |_, cells| {
            rows.push(cells.iter().map(|c| String::from_utf8_lossy(&cell_text(c)).into_owned()).collect());
        });
        rows
    }

    fn dot_tables(&mut self, pattern: Option<&str>) {
        let filter = match pattern {
            Some(p) => format!(" AND name LIKE '{}'", p.replace('\'', "''")),
            None => String::new(),
        };
        let sql = format!(
            "SELECT name FROM sqlite_schema WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%'{filter} ORDER BY 1"
        );
        let names: Vec<String> = self.query_strings(&sql).into_iter().filter_map(|r| r.into_iter().next()).collect();
        if names.is_empty() {
            return;
        }
        let maxlen = names.iter().map(|n| n.chars().count()).max().unwrap_or(0);
        let ncol = (80 / (maxlen + 2)).max(1);
        let nrow = names.len().div_ceil(ncol);
        for i in 0..nrow {
            let mut j = i;
            while j < names.len() {
                let sp = if j < nrow { "" } else { "  " };
                let pad = maxlen.saturating_sub(names[j].chars().count());
                self.out.extend_from_slice(format!("{sp}{}{}", names[j], " ".repeat(pad)).as_bytes());
                j += nrow;
            }
            self.out.push(b'\n');
        }
    }

    fn dot_schema(&mut self, pattern: Option<&str>) {
        let filter = match pattern {
            Some(p) => format!(" AND lower(tbl_name) LIKE '{}'", p.to_lowercase().replace('\'', "''")),
            None => " AND name NOT LIKE 'sqlite_%'".to_string(),
        };
        let sql = format!("SELECT type, name, sql FROM sqlite_schema WHERE sql IS NOT NULL{filter} ORDER BY rowid");
        let rows = self.query_strings(&sql);
        for row in rows {
            let (kind, name, text) = (&row[0], &row[1], &row[2]);
            let mut line = text.clone();
            if kind == "view" {
                let cols: Vec<String> = self
                    .query_strings(&format!("SELECT name FROM pragma_table_info('{}')", name.replace('\'', "''")))
                    .into_iter()
                    .filter_map(|r| r.into_iter().next())
                    .collect();
                if !cols.is_empty() {
                    line.push_str(&format!("\n/* {name}({}) */", cols.join(",")));
                }
            }
            line.push_str(";\n");
            self.out.extend_from_slice(line.as_bytes());
        }
    }

    /// `process_input`: lê stdin linha a linha.
    fn process_input(&mut self, input: &[u8], err: &mut Vec<u8>) -> i32 {
        let text = String::from_utf8_lossy(input);
        let mut lines: Vec<&str> = text.split('\n').collect();
        if text.ends_with('\n') {
            lines.pop();
        }
        let mut buf = String::new();
        let mut start_line = 0;
        let mut errors = 0;
        for (idx, line) in lines.iter().enumerate() {
            let lineno = idx + 1;
            if buf.is_empty() && line.trim().is_empty() {
                continue;
            }
            if buf.is_empty() && (line.starts_with('.') || line.starts_with('#')) {
                if line.starts_with('#') {
                    continue;
                }
                match self.meta(line) {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(msg) => {
                        err.extend_from_slice(msg.as_bytes());
                        err.push(b'\n');
                        errors += 1;
                        if self.opts.bail {
                            break;
                        }
                    }
                }
                continue;
            }
            if buf.is_empty() {
                start_line = lineno;
                buf.push_str(line.trim_start());
            } else {
                buf.push('\n');
                buf.push_str(line);
            }
            if line.contains(';') && scan(&buf).complete {
                if self.run_buffer(&buf, start_line, err) {
                    errors += 1;
                    if self.opts.bail {
                        buf.clear();
                        break;
                    }
                }
                buf.clear();
            } else if is_blank_sql(&buf) && !buf.trim().is_empty() && scan(&buf).complete {
                buf.clear();
            }
        }
        if !buf.is_empty() && !is_blank_sql(&buf) && !(self.opts.bail && errors > 0) && self.run_buffer(&buf, start_line, err) {
            errors += 1;
        }
        if errors > 0 { 1 } else { 0 }
    }

    /// Roda um buffer vindo do stdin; `true` se deu erro (já impresso).
    fn run_buffer(&mut self, buf: &str, start_line: usize, err: &mut Vec<u8>) -> bool {
        match self.shell_exec(buf) {
            Ok(()) => false,
            Err(e) => {
                let (kind, tail) = if let Some(t) = e.message.strip_prefix("in prepare, ") {
                    ("Parse error", t)
                } else if let Some(t) = e.message.strip_prefix("stepping, ") {
                    ("Runtime error", t)
                } else {
                    ("Error", e.message.as_str())
                };
                err.extend_from_slice(format!("{kind} near line {start_line}: {tail}\n").as_bytes());
                true
            }
        }
    }
}

fn is_eqp(stmt: &str) -> bool {
    let mut s = stmt;
    loop {
        let t = s.trim_start();
        if let Some(rest) = t.strip_prefix("--") {
            s = rest.split_once('\n').map(|x| x.1).unwrap_or("");
        } else if let Some(rest) = t.strip_prefix("/*") {
            s = rest.split_once("*/").map(|x| x.1).unwrap_or("");
        } else {
            s = t;
            break;
        }
    }
    let words: Vec<String> = s.split_whitespace().take(3).map(|w| w.to_ascii_lowercase()).collect();
    words.len() == 3 && words[0] == "explain" && words[1] == "query" && words[2].starts_with("plan")
}

fn render_eqp(rows: &[(i64, i64, String)], out: &mut Vec<u8>) {
    out.extend_from_slice(b"QUERY PLAN\n");
    fn level(rows: &[(i64, i64, String)], parent: i64, prefix: &mut String, out: &mut Vec<u8>) {
        let children: Vec<&(i64, i64, String)> = rows.iter().filter(|r| r.1 == parent).collect();
        for (k, row) in children.iter().enumerate() {
            let has_next = k + 1 < children.len();
            out.extend_from_slice(format!("{prefix}{}{}\n", if has_next { "|--" } else { "`--" }, row.2).as_bytes());
            let n = prefix.len();
            prefix.push_str(if has_next { "|  " } else { "   " });
            level(rows, row.0, prefix, out);
            prefix.truncate(n);
        }
    }
    let mut prefix = String::new();
    level(rows, 0, &mut prefix, out);
}

/// Roda o programa `sqlite3` com `argv` sobre o FS do caso, usando `backend` como motor.
pub fn run_sqlite3(backend: &mut dyn Backend, argv: &[String], ctx: &mut Ctx<'_>) -> i32 {
    let mut opts = Opts { headers: false, separator: b"|".to_vec(), bail: false };
    let mut readonly = false;
    let mut positional: Vec<&str> = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let a = argv[i].as_str();
        if positional.is_empty() && a.starts_with('-') && a.len() > 1 {
            let flag = a.trim_start_matches('-');
            match flag {
                "header" | "headers" => opts.headers = true,
                "noheader" => opts.headers = false,
                "bail" => opts.bail = true,
                "readonly" => readonly = true,
                "separator" => {
                    i += 1;
                    if let Some(s) = argv.get(i) {
                        opts.separator = s.as_bytes().to_vec();
                    }
                }
                "batch" | "list" => {}
                _ => {
                    ctx.stderr.extend_from_slice(format!("sqlite3: Error: unknown option: {a}\n").as_bytes());
                    return 1;
                }
            }
        } else {
            positional.push(a);
        }
        i += 1;
    }
    let db = positional.first().copied();
    let sql_args: Vec<&str> = positional.iter().skip(1).copied().collect();
    let path = match db {
        None | Some(":memory:") | Some("") => None,
        Some(p) => Some(rel(p)),
    };
    if let Some(p) = &path
        && ctx.fs.get(p).is_none()
    {
        if readonly {
            ctx.stderr.extend_from_slice(
                format!("Error: unable to open database \"{}\": unable to open database file\n", db.unwrap_or_default())
                    .as_bytes(),
            );
            return 1;
        }
        ctx.fs.insert(p, harness::Entry::file(Vec::new(), 0o644));
    }
    let session = match backend.open(ctx.fs, path.as_deref(), readonly) {
        Ok(s) => s,
        Err(e) => {
            ctx.stderr.extend_from_slice(
                format!("Error: unable to open database \"{}\": {e}\n", db.unwrap_or_default()).as_bytes(),
            );
            return 1;
        }
    };
    let mut out = Vec::new();
    let mut runner = Runner { session, opts, out: &mut out };
    let mut rc = 0;
    let mut abrupt = false;
    if sql_args.is_empty() {
        rc = runner.process_input(ctx.stdin, ctx.stderr);
    } else {
        for arg in sql_args {
            if arg.starts_with('.') {
                match runner.meta(arg) {
                    Ok(_) => {}
                    Err(msg) => {
                        ctx.stderr.extend_from_slice(msg.as_bytes());
                        ctx.stderr.push(b'\n');
                        rc = 1;
                        break;
                    }
                }
                continue;
            }
            if let Err(e) = runner.shell_exec(arg) {
                ctx.stderr.extend_from_slice(format!("Error: {}\n", e.message).as_bytes());
                rc = e.rc;
                abrupt = true;
                break;
            }
        }
    }
    let Runner { session, .. } = runner;
    ctx.stdout.extend_from_slice(&out);
    if let Err(e) = session.close(ctx.fs, abrupt) {
        ctx.stderr.extend_from_slice(format!("Error: closing database: {e}\n").as_bytes());
        if rc == 0 {
            rc = 1;
        }
    }
    rc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_formatting_matches_sqlite_346() {
        let cases: &[(f64, &str)] = &[
            (1.0, "1.0"),
            (0.1 + 0.2, "0.3"),
            (1e20, "1.0e+20"),
            (1.5e-7, "1.5e-07"),
            (100.0 / 3.0, "33.3333333333333"),
            (6.0, "6.0"),
            (9007199254740993.0, "9.00719925474099e+15"),
            (123456789012345678.0, "1.23456789012346e+17"),
            (0.5, "0.5"),
            (1e15, "1.0e+15"),
            (1e14, "100000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1.0e-05"),
            (-2.5, "-2.5"),
            (f64::INFINITY, "Inf"),
            (11.6666666666667, "11.6666666666667"),
            (std::f64::consts::PI, "3.14159265358979"),
            (2461055.5, "2461055.5"),
        ];
        for (v, want) in cases {
            assert_eq!(fmt_real(*v), *want, "{v}");
        }
    }

    #[test]
    fn complete_and_boundaries() {
        assert!(scan("SELECT 1;").complete);
        assert!(!scan("SELECT 1").complete);
        assert!(!scan("SELECT 'a;").complete);
        assert!(scan("SELECT 1; -- x").complete);
        let trig = "CREATE TRIGGER t AFTER INSERT ON a BEGIN INSERT INTO b VALUES(1); END;";
        let s = scan(trig);
        assert!(s.complete);
        assert_eq!(s.boundaries, vec![trig.len()]);
        let s = scan("SELECT 1; SELECT 2;");
        assert_eq!(s.boundaries, vec![9, 19]);
        assert!(is_blank_sql("  ;; -- c\n /* x */ ;"));
        assert!(!is_blank_sql("-- c\nSELECT 1"));
    }

    #[test]
    fn error_context_like_shell() {
        assert_eq!(error_context("SELEC 1;", Some(0)), "\n  SELEC 1;\n  ^--- error here");
        let sql = "SELECT count(*) FROM a WHERE count(*) > 1;";
        assert_eq!(error_context(sql, Some(29)), format!("\n  {sql}\n                 error here ---^"));
        assert_eq!(error_context("x", None), "");
    }
}
