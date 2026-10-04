//! `.import` (leitor CSV/ASCII e criação automática da tabela, como o shell.c) e as extensões que o
//! shell.c embute e que não dependem de tabela virtual.

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};

use super::ImportArgs;
use crate::cli::exec::error_parts;
use crate::cli::text::{self, dquote};
use crate::cli::{Exit, Shell};
use crate::unwind;

/// `strtod` da libc (prefixo numérico, o resto é ignorado).
pub fn c_strtod(z: &[u8]) -> f64 {
    let z = text::cstr(z);
    let mut i = 0;
    while i < z.len() && crate::cli::scan::is_space(z[i]) {
        i += 1;
    }
    let start = i;
    if i < z.len() && (z[i] == b'+' || z[i] == b'-') {
        i += 1;
    }
    let lower: Vec<u8> = z[i..].iter().take(8).map(|c| c.to_ascii_lowercase()).collect();
    if lower.starts_with(b"inf") {
        let neg = z.get(start) == Some(&b'-');
        return if neg { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    if lower.starts_with(b"nan") {
        return f64::NAN;
    }
    let mut digits = 0;
    while i < z.len() && z[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < z.len() && z[i] == b'.' {
        i += 1;
        while i < z.len() && z[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return 0.0;
    }
    let mut end = i;
    if i < z.len() && (z[i] == b'e' || z[i] == b'E') {
        let mut k = i + 1;
        if k < z.len() && (z[k] == b'+' || z[k] == b'-') {
            k += 1;
        }
        if k < z.len() && z[k].is_ascii_digit() {
            while k < z.len() && z[k].is_ascii_digit() {
                k += 1;
            }
            end = k;
        }
    }
    let mut s = String::from_utf8_lossy(&z[start..end]).into_owned();
    if s.ends_with('.') {
        s.push('0');
    }
    if let Some(p) = s.find('.')
        && (p == 0 || !s.as_bytes()[p - 1].is_ascii_digit())
    {
        s.insert(p, '0');
    }
    s.parse().unwrap_or(0.0)
}

/// Leitor de campos do `.import` (`csv_read_one_field`/`ascii_read_one_field`).
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    file: String,
    line: usize,
    rows: usize,
    errors: usize,
    not_first: bool,
    /// Caractere que terminou o último campo; `None` = EOF.
    term: Option<u8>,
    col: u8,
    row: u8,
    ascii: bool,
}

impl Reader<'_> {
    fn getc(&mut self) -> Option<u8> {
        let c = self.data.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn read(&mut self, sh: &mut Shell) -> Option<Vec<u8>> {
        if self.ascii { self.read_ascii() } else { self.read_csv(sh) }
    }

    fn read_ascii(&mut self) -> Option<Vec<u8>> {
        let mut z = Vec::new();
        let Some(mut c) = self.getc() else {
            self.term = None;
            return None;
        };
        loop {
            if c == self.col || c == self.row {
                break;
            }
            z.push(c);
            match self.getc() {
                Some(n) => c = n,
                None => {
                    self.term = None;
                    return Some(z);
                }
            }
        }
        if c == self.row {
            self.line += 1;
        }
        self.term = Some(c);
        Some(z)
    }

    fn read_csv(&mut self, sh: &mut Shell) -> Option<Vec<u8>> {
        let mut z: Vec<u8> = Vec::new();
        let (sep, rsep) = (self.col, self.row);
        let Some(c) = self.getc() else {
            self.term = None;
            return None;
        };
        if sh.seen_interrupt > 0 {
            self.term = None;
            return None;
        }
        if c == b'"' {
            let start_line = self.line;
            let (mut pc, mut ppc): (Option<u8>, Option<u8>) = (None, None);
            loop {
                let c = self.getc();
                if c == Some(rsep) {
                    self.line += 1;
                }
                if c == Some(b'"') && pc == Some(b'"') {
                    pc = None;
                    continue;
                }
                if (c == Some(sep) && pc == Some(b'"'))
                    || (c == Some(rsep) && pc == Some(b'"'))
                    || (c == Some(rsep) && pc == Some(b'\r') && ppc == Some(b'"'))
                    || (c.is_none() && pc == Some(b'"'))
                {
                    while let Some(last) = z.pop() {
                        if last == b'"' {
                            break;
                        }
                    }
                    self.term = c;
                    break;
                }
                if pc == Some(b'"') && c != Some(b'\r') {
                    sh.eputs(&format!("{}:{}: unescaped \" character\n", self.file, self.line));
                }
                let Some(ch) = c else {
                    sh.eputs(&format!("{}:{}: unterminated \"-quoted field\n", self.file, start_line));
                    self.term = None;
                    break;
                };
                z.push(ch);
                ppc = pc;
                pc = Some(ch);
            }
        } else {
            let mut c = Some(c);
            if c == Some(0xef) && !self.not_first {
                z.push(0xef);
                c = self.getc();
                if c == Some(0xbb) {
                    z.push(0xbb);
                    c = self.getc();
                    if c == Some(0xbf) {
                        self.not_first = true;
                        return self.read_csv(sh);
                    }
                }
            }
            while let Some(ch) = c {
                if ch == sep || ch == rsep {
                    break;
                }
                z.push(ch);
                c = self.getc();
            }
            if c == Some(rsep) {
                self.line += 1;
                if z.last() == Some(&b'\r') {
                    z.pop();
                }
            }
            self.term = c;
        }
        self.not_first = true;
        Some(z)
    }
}

/// `zAutoColumn`: as colunas da tabela nova (todas TEXT), renomeando duplicatas como o shell.c faz,
/// com as mesmas consultas num banco em memória.
fn auto_columns(names: &[Vec<u8>]) -> Option<(String, Option<String>)> {
    if names.is_empty() {
        return None;
    }
    let db = Connection::open_with_flags(":memory:", OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    db.execute_batch("CREATE TABLE ColNames( cpos INTEGER PRIMARY KEY, name TEXT, nlen INT, chop INT, reps INT, suff TEXT);CREATE VIEW RepeatedNames AS SELECT DISTINCT t.name FROM ColNames t WHERE t.name COLLATE NOCASE IN ( SELECT o.name FROM ColNames o WHERE o.cpos<>t.cpos);").ok()?;
    for n in names {
        let v = match String::from_utf8(n.clone()) {
            Ok(s) => Value::Text(s),
            Err(e) => Value::Blob(e.into_bytes()),
        };
        db.execute("INSERT INTO ColNames(name,nlen,chop,reps,suff) VALUES(iif(length(?1)>0,?1,'?'),max(length(?1),1),0,0,'')", [v]).ok()?;
    }
    let has_dupes: i64 = db
        .query_row("SELECT count(DISTINCT (substring(name,1,nlen-chop)||suff) COLLATE NOCASE) <count(name) FROM ColNames", [], |r| r.get(0))
        .ok()?;
    if has_dupes != 0 {
        let digits: i64 = db.query_row("SELECT CAST(ceil(log(count(*)+0.5)) AS INT) FROM ColNames ", [], |r| r.get(0)).ok()?;
        db.execute_batch("UPDATE ColNames AS t SET reps=(SELECT count(*) FROM ColNames d  WHERE substring(t.name,1,t.nlen-t.chop)=substring(d.name,1,d.nlen-d.chop) COLLATE NOCASE)").ok()?;
        let rank = "WITH Lzn(nlz) AS (  SELECT 0 AS nlz  UNION  SELECT nlz+1 AS nlz FROM Lzn  WHERE EXISTS(   SELECT 1   FROM ColNames t, ColNames o   WHERE    iif(t.name IN (SELECT * FROM RepeatedNames),     printf('%s_%s',      t.name, substring(printf('%.*c%0.*d',nlz+1,'0',$1,t.cpos),2)),     t.name    )    =    iif(o.name IN (SELECT * FROM RepeatedNames),     printf('%s_%s',      o.name, substring(printf('%.*c%0.*d',nlz+1,'0',$1,o.cpos),2)),     o.name    )    COLLATE NOCASE    AND o.cpos<>t.cpos   GROUP BY t.cpos  )) UPDATE Colnames AS t SET chop = 0, suff = iif(name IN (SELECT * FROM RepeatedNames),  printf('_%s', substring(   printf('%.*c%0.*d',(SELECT max(nlz) FROM Lzn)+1,'0',1,t.cpos),2)),  '' )";
        db.execute(rank, [digits]).ok()?;
    }
    let spec: String = db
        .query_row(
            "SELECT '('||x'0a' || group_concat(  cname||' TEXT',  ','||iif((cpos-1)%4>0, ' ', x'0a'||' ')) ||')' AS ColsSpec FROM ( SELECT cpos, printf('\"%w\"',printf('%!.*s%s', nlen-chop,name,suff)) AS cname  FROM ColNames ORDER BY cpos)",
            [],
            |r| r.get(0),
        )
        .ok()?;
    let renamed = if has_dupes != 0 {
        db.query_row(
            "SELECT group_concat( printf('\"%w\" to \"%w\"',name,printf('%!.*s%s', nlen-chop, name, suff)), ','||x'0a')FROM ColNames WHERE suff<>'' OR chop!=0",
            [],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
    } else {
        None
    };
    Some((spec, renamed))
}

fn table_exists(conn: &Connection, schema: Option<&[u8]>, table: &[u8]) -> bool {
    let sql = match schema {
        Some(s) => format!("SELECT 1 FROM {}.sqlite_schema WHERE type IN ('table','view') AND name={} COLLATE NOCASE", String::from_utf8_lossy(&dquote(s)), String::from_utf8_lossy(&text::squote(table))),
        None => format!(
            "SELECT 1 FROM sqlite_temp_schema WHERE type IN ('table','view') AND name={0} COLLATE NOCASE UNION ALL SELECT 1 FROM sqlite_schema WHERE type IN ('table','view') AND name={0} COLLATE NOCASE",
            String::from_utf8_lossy(&text::squote(table))
        ),
    };
    let r = conn.prepare(&sql).and_then(|mut s| s.exists([]));
    unwind::reraise();
    r.unwrap_or(false)
}

/// `.import` com o arquivo já lido.
pub fn import(sh: &mut Shell, a: ImportArgs<'_>) -> Result<i32, Exit> {
    let mut r = Reader {
        data: a.data,
        pos: 0,
        file: String::from_utf8_lossy(a.file_name).into_owned(),
        line: 1,
        rows: 0,
        errors: 0,
        not_first: false,
        term: None,
        col: a.col_sep,
        row: a.row_sep,
        ascii: a.ascii,
    };
    let mut skip = a.skip;
    while skip > 0 {
        skip -= 1;
        while r.read(sh).is_some() && r.term == Some(r.col) {}
    }
    let schema_name = a.schema.map(|s| s.to_vec()).unwrap_or_else(|| b"main".to_vec());
    if !table_exists(sh.conn(), a.schema, a.table) {
        let mut names = Vec::new();
        while let Some(f) = r.read(sh) {
            names.push(f);
            if r.term != Some(r.col) {
                break;
            }
        }
        let Some((spec, renamed)) = auto_columns(&names) else {
            sh.eputs(&format!("{}: empty file\n", r.file));
            return Ok(1);
        };
        if let Some(ren) = renamed {
            sh.eputs(&format!("Columns renamed during .import {} due to duplicates:\n{}\n", r.file, ren));
        }
        let create = format!(
            "CREATE TABLE {}.{}{}\n",
            String::from_utf8_lossy(&dquote(&schema_name)),
            String::from_utf8_lossy(&dquote(a.table)),
            spec
        );
        if a.verbose >= 1 {
            sh.oputs(&format!("{create}\n"));
        }
        let res = sh.conn().execute_batch(&create);
        unwind::reraise();
        if let Err(e) = res {
            sh.eputs(&format!("(null) failed:\n{}\n", error_parts(&e).1));
            return Ok(1);
        }
    }
    let count_sql = format!(
        "SELECT count(*) FROM pragma_table_info({},{});",
        String::from_utf8_lossy(&text::squote(a.table)),
        match a.schema {
            Some(s) => String::from_utf8_lossy(&text::squote(s)).into_owned(),
            None => "NULL".into(),
        }
    );
    let n_col: i64 = match sh.conn().query_row(&count_sql, [], |row| row.get(0)) {
        Ok(n) => n,
        Err(e) => {
            unwind::reraise();
            sh.eputs(&format!("Error: {}\n", error_parts(&e).1));
            return Ok(1);
        }
    };
    unwind::reraise();
    if n_col == 0 {
        return Ok(0);
    }
    let n_col = n_col as usize;
    let mut sql = match a.schema {
        Some(s) => format!("INSERT INTO {}.{} VALUES(?", String::from_utf8_lossy(&dquote(s)), String::from_utf8_lossy(&dquote(a.table))),
        None => format!("INSERT INTO {} VALUES(?", String::from_utf8_lossy(&dquote(a.table))),
    };
    for _ in 1..n_col {
        sql.push_str(",?");
    }
    sql.push(')');
    if a.verbose >= 2 {
        sh.oputs(&format!("Insert using: {sql}\n"));
    }
    let Some(db) = sh.db.take() else { return Ok(1) };
    let out = import_rows(sh, db.conn(), &sql, n_col, &mut r);
    sh.db = Some(db);
    let rc = out?;
    if a.verbose > 0 {
        sh.oputs(&format!("Added {} rows with {} errors using {} lines of input\n", r.rows, r.errors, r.line - 1));
    }
    Ok(rc)
}

fn import_rows(sh: &mut Shell, conn: &Connection, sql: &str, n_col: usize, r: &mut Reader<'_>) -> Result<i32, Exit> {
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(e) => {
            unwind::reraise();
            sh.eputs(&format!("Error: {}\n", error_parts(&e).1));
            return Ok(1);
        }
    };
    let need_commit = conn.is_autocommit();
    if need_commit {
        let _ = conn.execute_batch("BEGIN");
    }
    let mut rc = 0;
    loop {
        let start_line = r.line;
        let mut i = 0;
        let mut stop = false;
        while i < n_col {
            let z = r.read(sh);
            if z.is_none() && i == 0 {
                stop = true;
                break;
            }
            if r.ascii && z.as_ref().is_none_or(|z| z.is_empty()) && i == 0 {
                stop = true;
                break;
            }
            let z = match z {
                None if !r.ascii && i == n_col - 1 && i > 0 => Some(Vec::new()),
                other => other,
            };
            let v = match z {
                Some(b) => match String::from_utf8(text::cstr(&b).to_vec()) {
                    Ok(s) => Value::Text(s),
                    Err(e) => Value::Blob(e.into_bytes()),
                },
                None => Value::Null,
            };
            let _ = stmt.raw_bind_parameter(i + 1, v);
            if i < n_col - 1 && r.term != Some(r.col) {
                sh.eputs(&format!(
                    "{}:{}: expected {} columns but found {} - filling the rest with NULL\n",
                    r.file,
                    start_line,
                    n_col,
                    i + 1
                ));
                i += 2;
                while i <= n_col {
                    let _ = stmt.raw_bind_parameter(i, Value::Null);
                    i += 1;
                }
            }
            i += 1;
        }
        if !stop && r.term == Some(r.col) {
            loop {
                r.read(sh);
                i += 1;
                if r.term != Some(r.col) {
                    break;
                }
            }
            sh.eputs(&format!("{}:{}: expected {} columns but found {} - extras ignored\n", r.file, start_line, n_col, i));
        }
        if i >= n_col {
            let res = stmt.raw_execute();
            unwind::reraise();
            match res {
                Ok(_) => {
                    r.rows += 1;
                    rc = 0;
                }
                Err(e) => {
                    let (code, msg) = error_parts(&e);
                    sh.eputs(&format!("{}:{}: INSERT failed: {}\n", r.file, start_line, msg));
                    r.errors += 1;
                    rc = code;
                }
            }
            stmt.clear_bindings();
        }
        if r.term.is_none() {
            break;
        }
    }
    drop(stmt);
    if need_commit {
        let _ = conn.execute_batch("COMMIT");
    }
    unwind::reraise();
    Ok(rc)
}

/// Extensões do shell.c que não dependem de tabela virtual.
pub fn register(conn: &Connection) {
    let _ = conn.create_collation("uint", uint_collation);
}

/// Colação `uint` (ext/misc/uint.c): sequências de dígitos comparadas como inteiros.
fn uint_collation(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            while i < a.len() && a[i] == b'0' {
                i += 1;
            }
            while j < b.len() && b[j] == b'0' {
                j += 1;
            }
            let mut k = 0;
            while i + k < a.len() && j + k < b.len() && a[i + k].is_ascii_digit() && b[j + k].is_ascii_digit() {
                k += 1;
            }
            let da = i + k < a.len() && a[i + k].is_ascii_digit();
            let db = j + k < b.len() && b[j + k].is_ascii_digit();
            if da {
                return Ordering::Greater;
            }
            if db {
                return Ordering::Less;
            }
            match a[i..i + k].cmp(&b[j..j + k]) {
                Ordering::Equal => {}
                o => return o,
            }
            i += k;
            j += k;
        } else if a[i] != b[j] {
            return a[i].cmp(&b[j]);
        } else {
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}
