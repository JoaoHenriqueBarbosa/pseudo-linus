//! `shell_exec` e a impressão das linhas (`shell_callback`, `exec_prepared_stmt_columnar`, EXPLAIN e
//! EXPLAIN QUERY PLAN), com as mensagens de erro do shell.c (`save_err_msg`,
//! `shell_error_context`).

use rusqlite::types::ValueRef;
use rusqlite::{Connection, Statement};
use sysabi::{Clock, RusageWho};

use super::scan::{self, is_space};
use super::text::{self, cstr};
use super::{Exit, Mode, Shell, flag};
use crate::{funcs, unwind};

/// Um valor de coluna, com o tipo de armazenamento do SQLite.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Null,
    Int(i64),
    Real(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

impl Cell {
    fn from_ref(v: ValueRef<'_>) -> Cell {
        match v {
            ValueRef::Null => Cell::Null,
            ValueRef::Integer(i) => Cell::Int(i),
            ValueRef::Real(f) => Cell::Real(f),
            ValueRef::Text(t) => Cell::Text(t.to_vec()),
            ValueRef::Blob(b) => Cell::Blob(b.to_vec()),
        }
    }

    /// `sqlite3_column_text` (None = NULL).
    pub fn text(&self) -> Option<Vec<u8>> {
        match self {
            Cell::Null => None,
            Cell::Int(i) => Some(i.to_string().into_bytes()),
            Cell::Real(f) => Some(funcs::fmt_real(*f, 15)),
            Cell::Text(t) | Cell::Blob(t) => Some(t.clone()),
        }
    }
}

/// Erro de um `shell_exec`: a mensagem já com a fase ("in prepare, ..." ou "stepping, ...") e o
/// código primário do SQLite.
#[derive(Clone, Debug)]
pub struct ExecError {
    pub message: String,
    pub rc: i32,
}

/// Código primário e mensagem de um erro do rusqlite (o `sqlite3_errmsg`).
pub fn error_parts(e: &rusqlite::Error) -> (i32, String) {
    match e {
        rusqlite::Error::SqliteFailure(err, msg) => {
            (err.extended_code & 0xff, msg.clone().unwrap_or_else(|| funcs::errstr(err.extended_code)))
        }
        rusqlite::Error::SqlInputError { error, msg, .. } => (error.extended_code & 0xff, msg.clone()),
        rusqlite::Error::ExecuteReturnedResults => (1, "not an error".into()),
        other => (1, other.to_string()),
    }
}

fn error_offset(e: &rusqlite::Error) -> Option<usize> {
    match e {
        rusqlite::Error::SqlInputError { offset, .. } => usize::try_from(*offset).ok(),
        _ => None,
    }
}

/// `shell_error_context`.
pub fn error_context(sql: &[u8], offset: Option<usize>) -> String {
    let sql = cstr(sql);
    let Some(mut off) = offset else { return String::new() };
    if off >= sql.len() {
        return String::new();
    }
    let mut z = sql;
    while off > 50 {
        off -= 1;
        z = &z[1..];
        while !z.is_empty() && (z[0] & 0xc0) == 0x80 {
            z = &z[1..];
            off = off.saturating_sub(1);
        }
    }
    let mut len = z.len();
    if len > 78 {
        len = 78;
        while len > 0 && (z[len] & 0xc0) == 0x80 {
            len -= 1;
        }
    }
    let code: Vec<u8> = z[..len].iter().map(|&c| if is_space(c) { b' ' } else { c }).collect();
    let code = String::from_utf8_lossy(&code);
    if off < 25 {
        format!("\n  {code}\n  {}^--- error here", " ".repeat(off))
    } else {
        format!("\n  {code}\n  {}error here ---^", " ".repeat(off - 14))
    }
}

/// `save_err_msg`.
fn save_err_msg(phase: &str, e: &rusqlite::Error, sql: Option<&[u8]>) -> ExecError {
    let (rc, msg) = error_parts(e);
    let mut m = format!("{phase}, {msg}");
    if rc > 1 {
        m.push_str(&format!(" ({rc})"));
    }
    if let Some(sql) = sql {
        m.push_str(&error_context(sql, error_offset(e)));
    }
    ExecError { message: m, rc }
}

/// Instante pro `.timer` (relógio de parede e CPU do processo).
#[derive(Clone, Copy)]
pub struct Times {
    wall: f64,
    user: f64,
    sys: f64,
}

pub fn times(sh: &Shell) -> Times {
    let wall = sh.sys.clock_gettime(Clock::Monotonic).map(|t| t.sec as f64 + f64::from(t.nsec) * 1e-9).unwrap_or(0.0);
    let ru = sh.sys.getrusage(RusageWho::SelfProcess).unwrap_or_default();
    Times { wall, user: ru.utime.as_secs_f64(), sys: ru.stime.as_secs_f64() }
}

/// `END_TIMER`.
pub fn end_timer(sh: &mut Shell, t0: Times) {
    let t1 = times(sh);
    let line = format!(
        "Run Time: real {:.3} user {:.6} sys {:.6}\n",
        t1.wall - t0.wall,
        t1.user - t0.user,
        t1.sys - t0.sys
    );
    sh.oputs(&line);
}

/// `doAutoDetectRestore`: um `.dump` sendo recarregado num banco vazio liga o modo que deixa o
/// esquema ser recriado sem o modo defensivo. Devolve `true` em erro.
pub fn auto_detect_restore(sh: &mut Shell, sql: &[u8]) -> bool {
    use rusqlite::config::DbConfig;
    if sh.restore_state >= 7 {
        return false;
    }
    match sh.restore_state {
        0 => {
            sh.restore_state = if !sh.safe_mode && cstr(sql) == b"PRAGMA foreign_keys=OFF;" { 1 } else { 7 };
        }
        1 => {
            let mut is_dump = false;
            if cstr(sql) == b"BEGIN TRANSACTION;" {
                let conn = sh.conn();
                let empty = conn.query_row("SELECT 1 FROM sqlite_schema LIMIT 1", [], |_| Ok(())).is_err();
                unwind::reraise();
                is_dump = empty;
            }
            if is_dump {
                let conn = sh.conn();
                let def = conn.db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE).unwrap_or(false);
                let dqs = conn.db_config(DbConfig::SQLITE_DBCONFIG_DQS_DDL).unwrap_or(false);
                let _ = conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, false);
                let _ = conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DQS_DDL, true);
                sh.restore_state = (if def { 2 } else { 0 }) + (if dqs { 4 } else { 0 });
            } else {
                sh.restore_state = 7;
            }
        }
        st => {
            let conn = sh.conn();
            if conn.is_autocommit() {
                if st & 2 != 0 {
                    let _ = conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true);
                }
                if st & 4 != 0 {
                    let _ = conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DQS_DDL, false);
                }
                sh.restore_state = 7;
            }
        }
    }
    false
}

/// `bind_prepared_stmt`: parâmetros vêm de `temp.sqlite_parameters` (`.parameter set`).
fn bind_params(conn: &Connection, stmt: &mut Statement<'_>) {
    let n = stmt.parameter_count();
    if n == 0 {
        return;
    }
    let mut q = conn.prepare("SELECT value FROM temp.sqlite_parameters WHERE key=?1").ok();
    unwind::reraise();
    for i in 1..=n {
        let name = stmt.parameter_name(i).map(str::to_string).unwrap_or_else(|| format!("?{i}"));
        let mut found: Option<rusqlite::types::Value> = None;
        if let Some(q) = q.as_mut() {
            found = q.query_row([&name], |r| r.get::<_, rusqlite::types::Value>(0)).ok();
            unwind::reraise();
        }
        let v = match found {
            Some(v) => v,
            None if name.eq_ignore_ascii_case("_NAN") => rusqlite::types::Value::Real(f64::NAN),
            None if name.eq_ignore_ascii_case("_INF") => rusqlite::types::Value::Real(f64::INFINITY),
            None => rusqlite::types::Value::Null,
        };
        let _ = stmt.raw_bind_parameter(i, v);
    }
}

/// `explain_data_prepare`: recuo das linhas do EXPLAIN.
fn explain_indent(rows: &[Vec<Cell>]) -> Vec<i32> {
    let next_ops = ["Next", "Prev", "VPrev", "VNext", "SorterNext", "Return"];
    let yield_ops = ["Yield", "SeekLT", "SeekGT", "RowSetRead", "Rewind"];
    let mut indent = vec![0i32; rows.len()];
    let mut is_yield = vec![false; rows.len()];
    let int = |c: &Cell| match c {
        Cell::Int(i) => *i,
        Cell::Text(t) => String::from_utf8_lossy(t).parse().unwrap_or(0),
        _ => 0,
    };
    for (op_i, row) in rows.iter().enumerate() {
        let addr = row.first().map(int).unwrap_or(0);
        let op = row.get(1).and_then(Cell::text).unwrap_or_default();
        let op = String::from_utf8_lossy(&op).into_owned();
        let p1 = row.get(2).map(int).unwrap_or(0);
        let p2 = row.get(3).map(int).unwrap_or(0);
        let p2op = p2 + (op_i as i64 - addr);
        is_yield[op_i] = yield_ops.contains(&op.as_str());
        if next_ops.contains(&op.as_str()) && p2op > 0 {
            for x in (p2op as usize)..op_i {
                if let Some(v) = indent.get_mut(x) {
                    *v += 2;
                }
            }
        }
        if op == "Goto" && p2op >= 0 && (p2op as usize) < op_i && (is_yield[p2op as usize] || p1 != 0) {
            for v in indent.iter_mut().take(op_i).skip(p2op as usize) {
                *v += 2;
            }
        }
    }
    indent
}

/// Executa um comando (ou vários) e imprime o resultado no modo corrente.
pub fn shell_exec(sh: &mut Shell, sql: &[u8]) -> Result<Option<ExecError>, Exit> {
    let Some(db) = sh.db.take() else { return Ok(None) };
    let r = exec_with(sh, db.conn(), sql);
    sh.db = Some(db);
    r
}

fn exec_with(sh: &mut Shell, conn: &Connection, sql: &[u8]) -> Result<Option<ExecError>, Exit> {
    let sql = cstr(sql);
    for (b, e) in scan::split_statements(sql) {
        let piece = &sql[b..e];
        if scan::is_blank_sql(piece) {
            continue;
        }
        let piece_str = String::from_utf8_lossy(piece).into_owned();
        funcs::before_prepare();
        let prepared = conn.prepare(&piece_str);
        unwind::reraise();
        let mut stmt = match prepared {
            Ok(s) => s,
            Err(rusqlite::Error::MultipleStatement) => {
                // A varredura do complete.c e o parser discordaram: o resto vai inteiro pro parser.
                return Ok(Some(ExecError { message: "in prepare, multiple statements".into(), rc: 1 }));
            }
            Err(e) => return Ok(Some(save_err_msg("in prepare", &e, Some(&sql[b..])))),
        };
        sh.cnt = 0;
        if sh.auto_eqp > 0 && stmt.is_explain() == 0 {
            run_auto_eqp(sh, conn, &piece_str)?;
        }
        let is_explain = stmt.is_explain();
        sh.c_mode = sh.mode;
        if sh.auto_explain {
            if is_explain == 1 {
                sh.c_mode = Mode::Explain;
            }
            if is_explain == 2 {
                sh.c_mode = Mode::Eqp;
            }
        }
        bind_params(conn, &mut stmt);
        funcs::before_step(conn);
        let step_err = exec_prepared(sh, conn, &mut stmt)?;
        sh.indent.clear();
        eqp_render(sh);
        drop(stmt);
        funcs::after_statement(conn);
        unwind::reraise();
        if sh.poll_interrupt()? {
            return Ok(Some(ExecError { message: "stepping, interrupted (9)".into(), rc: 9 }));
        }
        if let Some(e) = step_err {
            return Ok(Some(save_err_msg("stepping", &e, None)));
        }
    }
    Ok(None)
}

/// `.eqp on|full`: o plano antes de cada comando.
fn run_auto_eqp(sh: &mut Shell, conn: &Connection, sql: &str) -> Result<(), Exit> {
    let q = format!("EXPLAIN QUERY PLAN {sql}");
    if let Ok(mut st) = conn.prepare(&q) {
        let mut rows = st.raw_query();
        while let Ok(Some(row)) = rows.next() {
            let id = row.get::<_, i64>(0).unwrap_or(0);
            let parent = row.get::<_, i64>(1).unwrap_or(0);
            let text = row.get_ref(3).ok().and_then(|v| Cell::from_ref(v).text()).unwrap_or_default();
            if text.first() == Some(&b'-') {
                eqp_render(sh);
            }
            sh.eqp.push(super::EqpRow { id, parent, text });
        }
        drop(rows);
        unwind::reraise();
        eqp_render(sh);
    }
    unwind::reraise();
    if sh.auto_eqp >= 3 {
        let q = format!("EXPLAIN {sql}");
        if let Ok(mut st) = conn.prepare(&q) {
            sh.c_mode = Mode::Explain;
            let _ = exec_prepared(sh, conn, &mut st)?;
            sh.indent.clear();
        }
        unwind::reraise();
    }
    Ok(())
}

/// Lê todas as linhas de um comando.
fn collect_rows(stmt: &mut Statement<'_>) -> (Vec<Vec<Cell>>, Option<rusqlite::Error>) {
    let n = stmt.column_count();
    let mut out = Vec::new();
    let mut rows = stmt.raw_query();
    loop {
        match rows.next() {
            Ok(Some(row)) => {
                let cells = (0..n).map(|i| row.get_ref(i).map(Cell::from_ref).unwrap_or(Cell::Null)).collect();
                out.push(cells);
            }
            Ok(None) => return (out, None),
            Err(e) => return (out, Some(e)),
        }
    }
}

fn column_names(stmt: &Statement<'_>) -> Vec<Vec<u8>> {
    (0..stmt.column_count())
        .map(|i| {
            unwind::guard(|| stmt.column_name(i).map(|s| s.as_bytes().to_vec()).unwrap_or_default()).unwrap_or_default()
        })
        .collect()
}

/// `exec_prepared_stmt`: devolve o erro do passo, se houve.
fn exec_prepared(sh: &mut Shell, conn: &Connection, stmt: &mut Statement<'_>) -> Result<Option<rusqlite::Error>, Exit> {
    let names = column_names(stmt);
    if sh.c_mode.is_columnar() {
        let (rows, err) = collect_rows(stmt);
        unwind::reraise();
        funcs::drain_puts(sh);
        if !rows.is_empty() && !names.is_empty() {
            columnar(sh, &names, &rows);
        }
        return Ok(err);
    }
    if sh.c_mode == Mode::Explain {
        let (rows, err) = collect_rows(stmt);
        unwind::reraise();
        sh.indent = explain_indent(&rows);
        sh.i_indent = 0;
        for r in &rows {
            output_row(sh, &names, r);
        }
        return Ok(err);
    }
    let n = names.len();
    let mut n_row: u64 = 0;
    let mut first = true;
    let mut err = None;
    {
        let mut rows = stmt.raw_query();
        loop {
            let next = rows.next();
            unwind::reraise();
            match next {
                Ok(Some(row)) => {
                    let cells: Vec<Cell> = (0..n).map(|i| row.get_ref(i).map(Cell::from_ref).unwrap_or(Cell::Null)).collect();
                    funcs::drain_puts(sh);
                    first = false;
                    n_row += 1;
                    output_row(sh, &names, &cells);
                    if sh.seen_interrupt > 0 || sh.poll_interrupt()? {
                        conn.get_interrupt_handle().interrupt();
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
    }
    funcs::drain_puts(sh);
    if !first {
        if sh.c_mode == Mode::Json {
            sh.oput(b"]\n");
        } else if sh.c_mode == Mode::Count {
            let line = format!("{} row{}\n", n_row, if n_row != 1 { "s" } else { "" });
            sh.stdout.put(line.as_bytes());
        }
    }
    Ok(err)
}

fn print_dashes(n: i32) -> Vec<u8> {
    vec![b'-'; n.max(0) as usize]
}

const BOX_24: &str = "\u{2500}";
const BOX_13: &str = "\u{2502}";
const BOX_23: &str = "\u{250c}";
const BOX_34: &str = "\u{2510}";
const BOX_12: &str = "\u{2514}";
const BOX_14: &str = "\u{2518}";
const BOX_123: &str = "\u{251c}";
const BOX_134: &str = "\u{2524}";
const BOX_234: &str = "\u{252c}";
const BOX_124: &str = "\u{2534}";
const BOX_1234: &str = "\u{253c}";

fn row_separator(sh: &mut Shell, n: usize, sep: &str) {
    let mut line = Vec::new();
    if n > 0 {
        line.extend_from_slice(sep.as_bytes());
        line.extend(print_dashes(sh.actual_width[0] + 2));
        for i in 1..n {
            line.extend_from_slice(sep.as_bytes());
            line.extend(print_dashes(sh.actual_width[i] + 2));
        }
        line.extend_from_slice(sep.as_bytes());
    }
    line.push(b'\n');
    sh.oput(&line);
}

fn box_separator(sh: &mut Shell, n: usize, s1: &str, s2: &str, s3: &str) {
    let mut line = String::new();
    if n > 0 {
        line.push_str(s1);
        line.push_str(&BOX_24.repeat((sh.actual_width[0] + 2).max(0) as usize));
        for i in 1..n {
            line.push_str(s2);
            line.push_str(&BOX_24.repeat((sh.actual_width[i] + 2).max(0) as usize));
        }
        line.push_str(s3);
    }
    line.push('\n');
    sh.oputs(&line);
}

/// `translateForDisplayAndDup`: a primeira linha do texto pra exibição em tabela, e o resto.
fn translate_for_display(z: &[u8], mx_width: i32, word_wrap: bool) -> (Vec<u8>, Option<Vec<u8>>) {
    let z = cstr(z);
    let mx = if mx_width < 0 { -mx_width } else { mx_width };
    let mx = if mx == 0 { 1_000_000 } else { mx } as usize;
    let at = |i: usize| -> u8 { z.get(i).copied().unwrap_or(0) };
    let (mut i, mut n) = (0usize, 0usize);
    while n < mx {
        let c = at(i);
        if c >= b' ' {
            n += 1;
            i += 1;
            while at(i) & 0xc0 == 0x80 {
                i += 1;
            }
            continue;
        }
        if c == b'\t' {
            loop {
                n += 1;
                if n & 7 == 0 || n >= mx {
                    break;
                }
            }
            i += 1;
            continue;
        }
        break;
    }
    let k;
    if n >= mx && word_wrap {
        let mut kk = i;
        while kk > i / 2 {
            if text_is_space(at(kk - 1)) {
                break;
            }
            kk -= 1;
        }
        if kk <= i / 2 {
            kk = i;
            while kk > i / 2 {
                if at(kk - 1).is_ascii_alphanumeric() != at(kk).is_ascii_alphanumeric() && at(kk) & 0xc0 != 0x80 {
                    break;
                }
                kk -= 1;
            }
        }
        if kk <= i / 2 {
            k = i;
        } else {
            i = kk;
            k = kk;
            while at(i) == b' ' {
                i += 1;
            }
        }
    } else {
        k = i;
    }
    let tail = if n >= mx && at(i) >= b' ' {
        Some(z[i..].to_vec())
    } else if at(i) == b'\r' && at(i + 1) == b'\n' {
        if at(i + 2) != 0 { Some(z[i + 2..].to_vec()) } else { None }
    } else if at(i) == 0 || at(i + 1) == 0 {
        None
    } else {
        Some(z[i + 1..].to_vec())
    };
    let mut out = Vec::new();
    let (mut j, mut n) = (0usize, 0usize);
    while j < k {
        let c = at(j);
        if c >= b' ' {
            n += 1;
            out.push(c);
            j += 1;
            while at(j) & 0xc0 == 0x80 {
                out.push(at(j));
                j += 1;
            }
            continue;
        }
        if c == b'\t' {
            loop {
                n += 1;
                out.push(b' ');
                if n & 7 == 0 || n >= mx {
                    break;
                }
            }
            j += 1;
            continue;
        }
        break;
    }
    (out, tail)
}

fn text_is_space(c: u8) -> bool {
    is_space(c)
}

/// `quoted_column`.
fn quoted_cell(c: &Cell) -> Vec<u8> {
    match c {
        Cell::Null => b"NULL".to_vec(),
        Cell::Int(_) | Cell::Real(_) => c.text().unwrap_or_default(),
        Cell::Text(t) => text::squote(cstr(t)),
        Cell::Blob(b) => {
            let mut out = b"x'".to_vec();
            for x in b {
                out.extend_from_slice(format!("{x:02x}").as_bytes());
            }
            out.push(b'\'');
            out
        }
    }
}

/// `exec_prepared_stmt_columnar`.
fn columnar(sh: &mut Shell, names: &[Vec<u8>], rows: &[Vec<Cell>]) {
    let ncol = names.len();
    let bw = sh.cm_opts.word_wrap;
    if ncol > sh.col_width.len() {
        sh.col_width.resize(ncol, 0);
    }
    sh.actual_width = sh.col_width.iter().take(ncol).map(|w| w.abs()).collect();
    let wrap_of = |sh: &Shell, i: usize| -> i32 {
        let mut wx = sh.col_width[i];
        if wx == 0 {
            wx = sh.cm_opts.wrap;
        }
        wx.abs()
    };
    let mut data: Vec<Vec<u8>> = Vec::new();
    for (i, name) in names.iter().enumerate() {
        data.push(translate_for_display(name, wrap_of(sh, i), bw).0);
    }
    let mut row_div: Vec<bool> = Vec::new();
    let mut multi_line = false;
    for row in rows {
        let mut next: Vec<Option<Vec<u8>>> = vec![None; ncol];
        let mut first = true;
        loop {
            let mut any = false;
            row_div.push(true);
            let r = row_div.len() - 1;
            for i in 0..ncol {
                let wx = wrap_of(sh, i);
                let src: Vec<u8> = if !first {
                    next[i].take().unwrap_or_default()
                } else if sh.cm_opts.quote {
                    quoted_cell(&row[i])
                } else {
                    row[i].text().unwrap_or_else(|| sh.null_value.clone())
                };
                let (shown, tail) = translate_for_display(&src, wx, bw);
                data.push(shown);
                if tail.is_some() {
                    any = true;
                    row_div[r] = false;
                    multi_line = true;
                }
                next[i] = tail;
            }
            first = false;
            if !any {
                break;
            }
        }
    }
    let total = data.len();
    for (i, z) in data.iter().enumerate() {
        let n = text::strlen_char(z) as i32;
        let j = i % ncol;
        if n > sh.actual_width[j] {
            sh.actual_width[j] = n;
        }
    }
    let (col_sep, row_sep): (&str, &str) = match sh.c_mode {
        Mode::Column => {
            if sh.show_header {
                let mut line = Vec::new();
                for (i, d) in data.iter().enumerate().take(ncol) {
                    let mut w = sh.actual_width[i];
                    if sh.col_width[i] < 0 {
                        w = -w;
                    }
                    line.extend(text::width_print(w, d));
                    line.extend_from_slice(if i == ncol - 1 { b"\n" } else { b"  " });
                }
                for i in 0..ncol {
                    line.extend(print_dashes(sh.actual_width[i]));
                    line.extend_from_slice(if i == ncol - 1 { b"\n" } else { b"  " });
                }
                sh.oput(&line);
            }
            ("  ", "\n")
        }
        Mode::Table | Mode::Markdown => {
            if sh.c_mode == Mode::Table {
                row_separator(sh, ncol, "+");
            }
            let mut line = b"| ".to_vec();
            for (i, d) in data.iter().enumerate().take(ncol) {
                let w = sh.actual_width[i];
                let n = text::strlen_char(d) as i32;
                line.extend(std::iter::repeat_n(b' ', ((w - n) / 2).max(0) as usize));
                line.extend_from_slice(d);
                line.extend(std::iter::repeat_n(b' ', ((w - n + 1) / 2).max(0) as usize));
                line.extend_from_slice(if i == ncol - 1 { b" |\n" } else { b" | " });
            }
            sh.oput(&line);
            row_separator(sh, ncol, if sh.c_mode == Mode::Table { "+" } else { "|" });
            (" | ", " |\n")
        }
        Mode::Box => {
            box_separator(sh, ncol, BOX_23, BOX_234, BOX_34);
            let mut line = format!("{BOX_13} ").into_bytes();
            for (i, d) in data.iter().enumerate().take(ncol) {
                let w = sh.actual_width[i];
                let n = text::strlen_char(d) as i32;
                line.extend(std::iter::repeat_n(b' ', ((w - n) / 2).max(0) as usize));
                line.extend_from_slice(d);
                line.extend(std::iter::repeat_n(b' ', ((w - n + 1) / 2).max(0) as usize));
                line.extend_from_slice(if i == ncol - 1 { format!(" {BOX_13}\n") } else { format!(" {BOX_13} ") }.as_bytes());
            }
            sh.oput(&line);
            box_separator(sh, ncol, BOX_123, BOX_1234, BOX_134);
            (" \u{2502} ", " \u{2502}\n")
        }
        _ => ("  ", "\n"),
    };
    let mut j = 0usize;
    let mut line = Vec::new();
    for i in ncol..total {
        if j == 0 && sh.c_mode != Mode::Column {
            line.extend_from_slice(if sh.c_mode == Mode::Box { "\u{2502} ".as_bytes() } else { b"| " });
        }
        let mut w = sh.actual_width[j];
        if sh.col_width[j] < 0 {
            w = -w;
        }
        line.extend(text::width_print(w, &data[i]));
        if j == ncol - 1 {
            line.extend_from_slice(row_sep.as_bytes());
            sh.oput(&line);
            line.clear();
            if multi_line && row_div[i / ncol - 1] && i + 1 < total {
                match sh.c_mode {
                    Mode::Table => row_separator(sh, ncol, "+"),
                    Mode::Box => box_separator(sh, ncol, BOX_123, BOX_1234, BOX_134),
                    Mode::Column => sh.oput(b"\n"),
                    _ => {}
                }
            }
            j = 0;
        } else {
            line.extend_from_slice(col_sep.as_bytes());
            j += 1;
        }
    }
    if sh.c_mode == Mode::Table {
        row_separator(sh, ncol, "+");
    } else if sh.c_mode == Mode::Box {
        box_separator(sh, ncol, BOX_12, BOX_124, BOX_14);
    }
}

/// Número REAL como o shell.c escreve nos modos insert e json (`%!.20g`, com ±9.0e+999 pros
/// infinitos).
fn real_literal(f: f64, insert: bool) -> Vec<u8> {
    if f == f64::INFINITY {
        return b"9.0e+999".to_vec();
    }
    if f == f64::NEG_INFINITY {
        return b"-9.0e+999".to_vec();
    }
    if insert {
        let ir = f as i64;
        if f == ir as f64 {
            return format!("{ir}.0").into_bytes();
        }
    }
    funcs::fmt_real(f, 20)
}

fn hex_blob(b: &[u8]) -> Vec<u8> {
    let mut out = b"X'".to_vec();
    for x in b {
        out.extend_from_slice(format!("{x:02x}").as_bytes());
    }
    out.push(b'\'');
    out
}

/// `shell_callback` pra uma linha.
pub fn output_row(sh: &mut Shell, names: &[Vec<u8>], cells: &[Cell]) {
    let n = cells.len();
    let mut o: Vec<u8> = Vec::new();
    let null = sh.null_value.clone();
    let colsep = sh.col_sep.clone();
    let rowsep = sh.row_sep.clone();
    match sh.c_mode {
        Mode::Count | Mode::Off => {}
        Mode::Line => {
            let w = names.iter().map(|c| cstr(c).len()).max().unwrap_or(0).max(5);
            if sh.cnt > 0 {
                o.extend_from_slice(&rowsep);
            }
            sh.cnt += 1;
            for (i, c) in cells.iter().enumerate() {
                let name = cstr(&names[i]);
                o.extend(std::iter::repeat_n(b' ', w.saturating_sub(name.len())));
                o.extend_from_slice(name);
                o.extend_from_slice(b" = ");
                o.extend_from_slice(cstr(&c.text().unwrap_or_else(|| null.clone())));
                o.extend_from_slice(&rowsep);
            }
        }
        Mode::Explain => {
            const WIDTHS: [i32; 8] = [4, 13, 4, 4, 4, 13, 2, 13];
            let nargs = n.min(WIDTHS.len());
            if sh.cnt == 0 {
                for i in 0..nargs {
                    o.extend(text::width_print(WIDTHS[i], &names[i]));
                    o.extend_from_slice(if i == nargs - 1 { b"\n" } else { b"  " });
                }
                for (i, &wd) in WIDTHS.iter().enumerate().take(nargs) {
                    o.extend(print_dashes(wd));
                    o.extend_from_slice(if i == nargs - 1 { b"\n" } else { b"  " });
                }
            }
            sh.cnt += 1;
            for i in 0..nargs {
                let mut sep: &[u8] = b"  ";
                let mut w = WIDTHS[i];
                let val = cells[i].text();
                if i == nargs - 1 {
                    w = 0;
                }
                if let Some(v) = &val
                    && text::strlen_char(v) as i32 > w
                {
                    w = text::strlen_char(v) as i32;
                    sep = b" ";
                }
                if i == 1 && !sh.indent.is_empty() {
                    if let Some(&ind) = sh.indent.get(sh.i_indent) {
                        o.extend(std::iter::repeat_n(b' ', ind.max(0) as usize));
                    }
                    sh.i_indent += 1;
                }
                o.extend(text::width_print(w, &val.unwrap_or_else(|| null.clone())));
                o.extend_from_slice(if i == nargs - 1 { b"\n" } else { sep });
            }
        }
        Mode::Semi => {
            if let Some(t) = cells.first().and_then(Cell::text) {
                o.extend(schema_line(&t, b";\n"));
            }
        }
        Mode::Pretty => {
            if let Some(t) = cells.first().and_then(Cell::text) {
                o.extend(pretty_schema(&t));
            }
        }
        Mode::List => {
            if sh.cnt == 0 && sh.show_header {
                for (i, name) in names.iter().enumerate() {
                    o.extend_from_slice(cstr(name));
                    o.extend_from_slice(if i == n - 1 { &rowsep } else { &colsep });
                }
            }
            sh.cnt += 1;
            for (i, c) in cells.iter().enumerate() {
                let z = c.text().unwrap_or_else(|| null.clone());
                o.extend_from_slice(cstr(&z));
                o.extend_from_slice(if i < n - 1 { &colsep } else { &rowsep });
            }
        }
        Mode::Html => {
            if sh.cnt == 0 && sh.show_header {
                o.extend_from_slice(b"<TR>");
                for name in names {
                    o.extend_from_slice(b"<TH>");
                    o.extend(text::html_string(name));
                    o.extend_from_slice(b"</TH>\n");
                }
                o.extend_from_slice(b"</TR>\n");
            }
            sh.cnt += 1;
            o.extend_from_slice(b"<TR>");
            for c in cells {
                o.extend_from_slice(b"<TD>");
                o.extend(text::html_string(&c.text().unwrap_or_else(|| null.clone())));
                o.extend_from_slice(b"</TD>\n");
            }
            o.extend_from_slice(b"</TR>\n");
        }
        Mode::Tcl => {
            if sh.cnt == 0 && sh.show_header {
                for (i, name) in names.iter().enumerate() {
                    o.extend(text::c_string(name));
                    if i < n - 1 {
                        o.extend_from_slice(&colsep);
                    }
                }
                o.extend_from_slice(&rowsep);
            }
            sh.cnt += 1;
            for (i, c) in cells.iter().enumerate() {
                o.extend(text::c_string(&c.text().unwrap_or_else(|| null.clone())));
                if i < n - 1 {
                    o.extend_from_slice(&colsep);
                }
            }
            o.extend_from_slice(&rowsep);
        }
        Mode::Csv => {
            if sh.cnt == 0 && sh.show_header {
                for (i, name) in names.iter().enumerate() {
                    o.extend(text::csv_field(Some(name), &null, &colsep));
                    if i < n - 1 {
                        o.extend_from_slice(&colsep);
                    }
                }
                o.extend_from_slice(&rowsep);
            }
            sh.cnt += 1;
            if n > 0 {
                for (i, c) in cells.iter().enumerate() {
                    o.extend(text::csv_field(c.text().as_deref(), &null, &colsep));
                    if i < n - 1 {
                        o.extend_from_slice(&colsep);
                    }
                }
                o.extend_from_slice(&rowsep);
            }
        }
        Mode::Insert => {
            o.extend_from_slice(b"INSERT INTO ");
            match &sh.dest_table {
                Some(t) => o.extend_from_slice(t),
                None => o.extend_from_slice(b"(null)"),
            }
            if sh.show_header {
                o.push(b'(');
                for (i, name) in names.iter().enumerate() {
                    if i > 0 {
                        o.push(b',');
                    }
                    o.extend(text::quote_ident_if_needed(cstr(name)));
                }
                o.push(b')');
            }
            sh.cnt += 1;
            for (i, c) in cells.iter().enumerate() {
                o.extend_from_slice(if i > 0 { b"," } else { b" VALUES(" });
                match c {
                    Cell::Null => o.extend_from_slice(b"NULL"),
                    Cell::Text(t) => {
                        if sh.has_flag(flag::NEWLINES) {
                            o.extend(text::quoted_string(t));
                        } else {
                            o.extend(text::quoted_escaped_string(t));
                        }
                    }
                    Cell::Int(v) => o.extend_from_slice(v.to_string().as_bytes()),
                    Cell::Real(f) => o.extend(real_literal(*f, true)),
                    Cell::Blob(b) => o.extend(hex_blob(b)),
                }
            }
            o.extend_from_slice(b");\n");
        }
        Mode::Json => {
            if sh.cnt == 0 {
                o.extend_from_slice(b"[{");
            } else {
                o.extend_from_slice(b",\n{");
            }
            sh.cnt += 1;
            for (i, c) in cells.iter().enumerate() {
                o.extend(text::json_string(&names[i], false));
                o.push(b':');
                match c {
                    Cell::Null => o.extend_from_slice(b"null"),
                    Cell::Real(f) => o.extend(real_literal(*f, false)),
                    Cell::Blob(b) => o.extend(text::json_string(b, true)),
                    Cell::Text(t) => o.extend(text::json_string(t, false)),
                    Cell::Int(v) => o.extend_from_slice(v.to_string().as_bytes()),
                }
                if i < n - 1 {
                    o.push(b',');
                }
            }
            o.push(b'}');
        }
        Mode::Quote => {
            if sh.cnt == 0 && sh.show_header {
                for (i, name) in names.iter().enumerate() {
                    if i > 0 {
                        o.extend_from_slice(&colsep);
                    }
                    o.extend(text::quoted_string(name));
                }
                o.extend_from_slice(&rowsep);
            }
            sh.cnt += 1;
            for (i, c) in cells.iter().enumerate() {
                if i > 0 {
                    o.extend_from_slice(&colsep);
                }
                match c {
                    Cell::Null => o.extend_from_slice(b"NULL"),
                    Cell::Text(t) => o.extend(text::quoted_string(t)),
                    Cell::Int(v) => o.extend_from_slice(v.to_string().as_bytes()),
                    Cell::Real(f) => o.extend(funcs::fmt_real(*f, 20)),
                    Cell::Blob(b) => o.extend(hex_blob(b)),
                }
            }
            o.extend_from_slice(&rowsep);
        }
        Mode::Ascii => {
            if sh.cnt == 0 && sh.show_header {
                for (i, name) in names.iter().enumerate() {
                    if i > 0 {
                        o.extend_from_slice(&colsep);
                    }
                    o.extend_from_slice(cstr(name));
                }
                o.extend_from_slice(&rowsep);
            }
            sh.cnt += 1;
            for (i, c) in cells.iter().enumerate() {
                if i > 0 {
                    o.extend_from_slice(&colsep);
                }
                o.extend_from_slice(cstr(&c.text().unwrap_or_else(|| null.clone())));
            }
            o.extend_from_slice(&rowsep);
        }
        Mode::Eqp => {
            let int = |c: Option<&Cell>| match c {
                Some(Cell::Int(i)) => *i,
                Some(c) => c.text().map(|t| String::from_utf8_lossy(&t).parse().unwrap_or(0)).unwrap_or(0),
                None => 0,
            };
            let id = int(cells.first());
            let parent = int(cells.get(1));
            if let Some(t) = cells.get(3).and_then(Cell::text) {
                sh.eqp.push(super::EqpRow { id, parent, text: t });
            }
        }
        Mode::Column | Mode::Table | Mode::Box | Mode::Markdown => {}
    }
    if !o.is_empty() {
        sh.oput(&o);
    }
}

/// `printSchemaLine`.
pub fn schema_line(z: &[u8], tail: &[u8]) -> Vec<u8> {
    let mut z = cstr(z).to_vec();
    if tail.first() == Some(&b';') && (contains(&z, b"/*") || contains(&z, b"--")) {
        for term in [&b""[..], b"*/", b"\n"] {
            let mut cand = z.clone();
            cand.extend_from_slice(term);
            cand.push(b';');
            if scan::complete(&cand) {
                cand.pop();
                z = cand;
                break;
            }
        }
    }
    let mut out = Vec::new();
    if strglob_create_table_quoted(&z) {
        out.extend_from_slice(b"CREATE TABLE IF NOT EXISTS ");
        out.extend_from_slice(&z[13..]);
    } else {
        out.extend_from_slice(&z);
    }
    out.extend_from_slice(tail);
    out
}

fn contains(z: &[u8], needle: &[u8]) -> bool {
    z.windows(needle.len()).any(|w| w == needle)
}

/// `sqlite3_strglob("CREATE TABLE ['\"]*", z)`.
fn strglob_create_table_quoted(z: &[u8]) -> bool {
    z.len() >= 14 && &z[..13] == b"CREATE TABLE " && (z[13] == b'\'' || z[13] == b'"')
}

/// `MODE_Pretty` (`.schema --indent`).
pub fn pretty_schema(src: &[u8]) -> Vec<u8> {
    let src = cstr(src);
    let lower = String::from_utf8_lossy(src).to_ascii_lowercase();
    if lower.starts_with("create view") || lower.starts_with("create trig") {
        let mut o = src.to_vec();
        o.extend_from_slice(b";\n");
        return o;
    }
    let mut z: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < src.len() && is_space(src[i]) {
        i += 1;
    }
    while i < src.len() {
        let c = src[i];
        if is_space(c) {
            if z.last() == Some(&b'\r') {
                let l = z.len();
                z[l - 1] = b'\n';
            }
            if z.last().is_some_and(|&p| is_space(p) || p == b'(') {
                i += 1;
                continue;
            }
        } else if (c == b'(' || c == b')') && z.last().is_some_and(|&p| is_space(p)) {
            z.pop();
        }
        z.push(c);
        i += 1;
    }
    while z.last().is_some_and(|&p| is_space(p)) {
        z.pop();
    }
    let mut out = Vec::new();
    if z.len() >= 79 {
        let mut cur: Vec<u8> = Vec::new();
        let mut c_end = 0u8;
        let mut n_paren = 0i32;
        let mut n_line = 0;
        let mut i = 0;
        while i < z.len() {
            let c = z[i];
            if c == c_end && c_end != 0 {
                c_end = 0;
            } else if c_end == 0 && (c == b'"' || c == b'\'' || c == b'`') {
                c_end = c;
            } else if c_end == 0 && c == b'[' {
                c_end = b']';
            } else if c_end == 0 && c == b'-' && z.get(i + 1) == Some(&b'-') {
                c_end = b'\n';
            } else if c_end == 0 && c == b'(' {
                n_paren += 1;
            } else if c_end == 0 && c == b')' {
                n_paren -= 1;
                if n_line > 0 && n_paren == 0 && !cur.is_empty() {
                    out.extend(schema_line(&cur, b"\n"));
                    cur.clear();
                }
            }
            cur.push(c);
            if n_paren == 1 && c_end == 0 && (c == b'(' || c == b'\n' || (c == b',' && !ws_to_eol(&z[i + 1..]))) {
                if c == b'\n' {
                    cur.pop();
                }
                out.extend(schema_line(&cur, b"\n  "));
                cur.clear();
                n_line += 1;
                while z.get(i + 1).is_some_and(|&p| is_space(p)) {
                    i += 1;
                }
            }
            i += 1;
        }
        z = cur;
    }
    out.extend(schema_line(&z, b";\n"));
    out
}

/// `wsToEol`.
fn ws_to_eol(z: &[u8]) -> bool {
    let mut i = 0;
    while i < z.len() {
        if z[i] == b'\n' {
            return true;
        }
        if is_space(z[i]) {
            i += 1;
            continue;
        }
        if z[i] == b'-' && z.get(i + 1) == Some(&b'-') {
            return true;
        }
        return false;
    }
    true
}

/// `eqp_render`.
pub fn eqp_render(sh: &mut Shell) {
    if sh.eqp.is_empty() {
        return;
    }
    let mut rows = std::mem::take(&mut sh.eqp);
    let mut out = Vec::new();
    if rows[0].text.first() == Some(&b'-') {
        if rows.len() == 1 {
            return;
        }
        out.extend_from_slice(&rows[0].text[3.min(rows[0].text.len())..]);
        out.push(b'\n');
        rows.remove(0);
    } else {
        out.extend_from_slice(b"QUERY PLAN\n");
    }
    fn level(rows: &[super::EqpRow], parent: i64, prefix: &mut Vec<u8>, out: &mut Vec<u8>) {
        let kids: Vec<&super::EqpRow> = rows.iter().filter(|r| r.parent == parent).collect();
        for (k, r) in kids.iter().enumerate() {
            let has_next = k + 1 < kids.len();
            out.extend_from_slice(prefix);
            out.extend_from_slice(if has_next { b"|--" } else { b"`--" });
            out.extend_from_slice(&r.text);
            out.push(b'\n');
            if prefix.len() < 100 - 7 {
                let n = prefix.len();
                prefix.extend_from_slice(if has_next { b"|  " } else { b"   " });
                level(rows, r.id, prefix, out);
                prefix.truncate(n);
            }
        }
    }
    let mut prefix = Vec::new();
    level(&rows, 0, &mut prefix, &mut out);
    sh.oput(&out);
}
