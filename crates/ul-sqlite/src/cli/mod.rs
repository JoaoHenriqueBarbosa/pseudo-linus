//! O programa `sqlite3`: porte do shell.c do SQLite 3.46.1 (CLI, modos de saída, comandos de ponto,
//! mensagens e códigos de saída) sobre o rusqlite, com o banco no FS do sandbox.

pub mod exec;
pub mod help;
pub mod meta;
pub mod out;
pub mod scan;
pub mod text;

use std::ffi::OsString;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;

use rusqlite::{Connection, OpenFlags};
use sysabi::{Ctx, Errno, Fd, OFlags, SigDisposition, Signal, Syscalls, sys};

use crate::{funcs, unwind, vfs};
use out::{Sink, Stream};
use scan::{Qss, is_space};

/// Os modos de saída (`MODE_*`), na ordem do `modeDescr`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Line,
    Column,
    List,
    Semi,
    Html,
    Insert,
    Quote,
    Tcl,
    Csv,
    Explain,
    Ascii,
    Pretty,
    Eqp,
    Json,
    Markdown,
    Table,
    Box,
    Count,
    Off,
}

impl Mode {
    pub fn descr(self) -> &'static str {
        match self {
            Mode::Line => "line",
            Mode::Column => "column",
            Mode::List => "list",
            Mode::Semi => "semi",
            Mode::Html => "html",
            Mode::Insert => "insert",
            Mode::Quote => "quote",
            Mode::Tcl => "tcl",
            Mode::Csv => "csv",
            Mode::Explain => "explain",
            Mode::Ascii => "ascii",
            Mode::Pretty => "prettyprint",
            Mode::Eqp => "eqp",
            Mode::Json => "json",
            Mode::Markdown => "markdown",
            Mode::Table => "table",
            Mode::Box => "box",
            Mode::Count => "count",
            Mode::Off => "off",
        }
    }

    pub fn is_columnar(self) -> bool {
        matches!(self, Mode::Column | Mode::Table | Mode::Box | Mode::Markdown)
    }
}

pub const SEP_COLUMN: &[u8] = b"|";
pub const SEP_ROW: &[u8] = b"\n";
pub const SEP_TAB: &[u8] = b"\t";
pub const SEP_SPACE: &[u8] = b" ";
pub const SEP_COMMA: &[u8] = b",";
pub const SEP_CRLF: &[u8] = b"\r\n";
pub const SEP_UNIT: &[u8] = b"\x1f";
pub const SEP_RECORD: &[u8] = b"\x1e";

/// `SHFLG_*`.
pub mod flag {
    pub const BACKSLASH: u32 = 0x04;
    pub const PRESERVE_ROWID: u32 = 0x08;
    pub const NEWLINES: u32 = 0x10;
    pub const COUNT_CHANGES: u32 = 0x20;
    pub const ECHO: u32 = 0x40;
    pub const HEADER_SET: u32 = 0x80;
    pub const DUMP_DATA_ONLY: u32 = 0x100;
    pub const DUMP_NO_SYS: u32 = 0x200;
    pub const TESTING_MODE: u32 = 0x400;
}

/// `SHELL_OPEN_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMode {
    Unspec,
    Normal,
    Readonly,
    Deserialize,
}

/// `ColModeOpts`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColModeOpts {
    pub wrap: i32,
    pub quote: bool,
    pub word_wrap: bool,
}

impl ColModeOpts {
    pub const DEFAULT: ColModeOpts = ColModeOpts { wrap: 60, quote: false, word_wrap: false };
    pub const QBOX: ColModeOpts = ColModeOpts { wrap: 60, quote: true, word_wrap: false };
    pub const ZERO: ColModeOpts = ColModeOpts { wrap: 0, quote: false, word_wrap: false };
}

/// A conexão, com o fechamento do jeito que o processo real faria: `sqlite3_close` na saída normal,
/// e nada (journal quente fica) quando o sqlite3 sai por `exit()` sem fechar ou morre.
pub struct Db {
    conn: Option<Connection>,
    pub abandon: bool,
}

impl Db {
    pub fn conn(&self) -> &Connection {
        self.conn.as_ref().expect("conexão aberta")
    }

    /// `close_db`: fecha e devolve a mensagem de erro do `sqlite3_close`, se houver.
    pub fn close(mut self) -> Option<String> {
        let conn = self.conn.take()?;
        let r = conn.close();
        unwind::reraise();
        match r {
            Ok(()) => None,
            Err((_, e)) => {
                let (code, msg) = exec::error_parts(&e);
                Some(format!("Error: sqlite3_close() returns {code}: {msg}\n"))
            }
        }
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let abrupt = self.abandon || std::thread::panicking();
            if abrupt {
                vfs::set_abandoned(true);
            }
            let _ = conn.close();
            if abrupt {
                vfs::set_abandoned(false);
            }
        }
    }
}

/// Linha do grafo do EXPLAIN QUERY PLAN.
#[derive(Clone, Debug)]
pub struct EqpRow {
    pub id: i64,
    pub parent: i64,
    pub text: Vec<u8>,
}

/// Leitor de linhas sobre um fd (o `fgets` do stdio).
pub struct LineReader {
    fd: Fd,
    buf: Vec<u8>,
    pos: usize,
    eof: bool,
    owned: bool,
}

impl LineReader {
    pub fn new(fd: Fd, owned: bool) -> LineReader {
        LineReader { fd, buf: Vec::new(), pos: 0, eof: false, owned }
    }

    fn fill(&mut self) -> bool {
        if self.eof {
            return false;
        }
        let mut chunk = vec![0u8; out::BUFSIZ];
        let mut r = sysabi::FdReader(self.fd);
        match r.read(&mut chunk) {
            Ok(0) | Err(_) => {
                self.eof = true;
                false
            }
            Ok(n) => {
                self.buf.drain(..self.pos);
                self.pos = 0;
                self.buf.extend_from_slice(&chunk[..n]);
                true
            }
        }
    }

    /// `local_getline`: a linha sem o `\n` (e sem o `\r` antes dele), cortada no primeiro NUL;
    /// `None` no fim.
    pub fn read_line(&mut self) -> Option<Vec<u8>> {
        loop {
            if let Some(p) = self.buf[self.pos..].iter().position(|&b| b == b'\n') {
                let mut line = self.buf[self.pos..self.pos + p].to_vec();
                self.pos += p + 1;
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Some(text::cstr(&line).to_vec());
            }
            if !self.fill() {
                if self.pos < self.buf.len() {
                    let line = self.buf[self.pos..].to_vec();
                    self.pos = self.buf.len();
                    return Some(text::cstr(&line).to_vec());
                }
                return None;
            }
        }
    }

    pub fn close(&mut self) {
        if self.owned {
            let _ = sys::close(self.fd);
            self.owned = false;
        }
    }
}

/// O `ShellState`.
pub struct Shell {
    pub db: Option<Db>,
    pub db_filename: Vec<u8>,
    pub open_mode: OpenMode,
    pub nofollow: bool,
    pub mode: Mode,
    pub c_mode: Mode,
    pub normal_mode: Mode,
    pub mode_prior: Mode,
    pub show_header: bool,
    pub flags: u32,
    pub prior_flags: u32,
    pub col_sep: Vec<u8>,
    pub row_sep: Vec<u8>,
    pub col_sep_prior: Vec<u8>,
    pub row_sep_prior: Vec<u8>,
    pub null_value: Vec<u8>,
    pub dest_table: Option<Vec<u8>>,
    pub col_width: Vec<i32>,
    pub actual_width: Vec<i32>,
    pub cm_opts: ColModeOpts,
    pub cnt: usize,
    pub stdout: Stream,
    pub redirect: Option<Stream>,
    pub stderr: Stream,
    pub out_count: i32,
    pub bail: bool,
    pub interactive: bool,
    pub auto_explain: bool,
    pub auto_eqp: u8,
    pub eqp: Vec<EqpRow>,
    pub indent: Vec<i32>,
    pub i_indent: usize,
    pub lineno: usize,
    pub input_nesting: usize,
    pub restore_state: u8,
    pub timer: bool,
    pub stats_on: u32,
    pub prompt_main: Vec<u8>,
    pub prompt_cont: Vec<u8>,
    pub safe_mode: bool,
    pub safe_mode_persist: bool,
    pub seen_interrupt: u32,
    pub n_err: usize,
    pub argv0: String,
    pub sys: std::sync::Arc<dyn Syscalls>,
    /// O `main` chegou ao fim normal (fecha o banco). Todo `return` antecipado do C sai sem fechar.
    pub clean_exit: bool,
}

/// Fim do processo pedido no meio do caminho (`exit(n)` do C).
pub struct Exit(pub i32);

impl Shell {
    pub fn new(argv0: String) -> Shell {
        Shell {
            db: None,
            db_filename: Vec::new(),
            open_mode: OpenMode::Unspec,
            nofollow: false,
            mode: Mode::List,
            c_mode: Mode::List,
            normal_mode: Mode::List,
            mode_prior: Mode::List,
            show_header: false,
            flags: 0,
            prior_flags: 0,
            col_sep: SEP_COLUMN.to_vec(),
            row_sep: SEP_ROW.to_vec(),
            col_sep_prior: Vec::new(),
            row_sep_prior: Vec::new(),
            null_value: Vec::new(),
            dest_table: None,
            col_width: Vec::new(),
            actual_width: Vec::new(),
            cm_opts: ColModeOpts::ZERO,
            cnt: 0,
            stdout: Stream::new(Sink::Inherited(Fd::STDOUT)),
            redirect: None,
            stderr: Stream::new(Sink::Inherited(Fd::STDERR)),
            out_count: 0,
            bail: false,
            interactive: false,
            auto_explain: true,
            auto_eqp: 0,
            eqp: Vec::new(),
            indent: Vec::new(),
            i_indent: 0,
            lineno: 0,
            input_nesting: 0,
            restore_state: 0,
            timer: false,
            stats_on: 0,
            prompt_main: b"sqlite> ".to_vec(),
            prompt_cont: b"   ...> ".to_vec(),
            safe_mode: false,
            safe_mode_persist: false,
            seen_interrupt: 0,
            n_err: 0,
            argv0,
            sys: sys::current(),
            clean_exit: false,
        }
    }

    pub fn has_flag(&self, f: u32) -> bool {
        self.flags & f != 0
    }

    pub fn set_flag(&mut self, f: u32, on: bool) {
        if on {
            self.flags |= f;
        } else {
            self.flags &= !f;
        }
    }

    /// Saída corrente (`p->out`).
    pub fn out(&mut self) -> &mut Stream {
        match self.redirect.as_mut() {
            Some(r) => r,
            None => &mut self.stdout,
        }
    }

    pub fn oput(&mut self, data: &[u8]) {
        self.out().put(data);
    }

    pub fn oputs(&mut self, s: &str) {
        self.out().put(s.as_bytes());
    }

    pub fn eput(&mut self, data: &[u8]) {
        self.stderr.put(data);
    }

    pub fn eputs(&mut self, s: &str) {
        self.stderr.put(s.as_bytes());
    }

    pub fn flush_out(&mut self) {
        self.out().flush();
    }

    /// `exit(code)` do C: esvazia os buffers do stdio e termina o processo sem fechar o banco.
    pub fn exit(&mut self, code: i32) -> Exit {
        if let Some(db) = self.db.as_mut() {
            db.abandon = true;
        }
        Exit(code)
    }

    pub fn conn(&self) -> &Connection {
        self.db.as_ref().expect("banco aberto").conn()
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        self.db.as_mut().expect("banco aberto").conn.as_mut().expect("conexão aberta")
    }

    /// `open_db`. `keepalive` é o `OPEN_DB_KEEPALIVE` (`.open`): em erro, cai num banco em memória
    /// em vez de sair.
    pub fn open_db(&mut self, keepalive: bool) -> Result<(), Exit> {
        if self.db.is_some() {
            return Ok(());
        }
        if self.open_mode == OpenMode::Unspec {
            self.open_mode = OpenMode::Normal;
        }
        let name = self.db_filename.clone();
        let name_str = String::from_utf8_lossy(&name).into_owned();
        let mut flags = match self.open_mode {
            OpenMode::Readonly => OpenFlags::SQLITE_OPEN_READ_ONLY,
            _ => OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        };
        if self.nofollow {
            flags |= OpenFlags::SQLITE_OPEN_NOFOLLOW;
        }
        // O shell.c liga URI com sqlite3_config(SQLITE_CONFIG_URI); o build do Debian já tem USE_URI.
        flags |= OpenFlags::SQLITE_OPEN_URI;
        let opened = if self.open_mode == OpenMode::Deserialize {
            open_conn(":memory:", OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE)
        } else if let Some(msg) = funcs::reject_foreign_vfs_uri(&name) {
            Err(msg)
        } else {
            open_conn(&name_str, flags)
        };
        let mut conn = match opened {
            Ok(c) => c,
            Err(msg) => {
                self.eputs(&format!("Error: unable to open database \"{name_str}\": {msg}\n"));
                if !keepalive {
                    return Err(self.exit(1));
                }
                match open_conn(":memory:", OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE) {
                    Ok(c) => {
                        self.eputs(&format!("Notice: using substitute in-memory database instead of \"{name_str}\"\n"));
                        c
                    }
                    Err(_) => {
                        self.eputs("Also: unable to open substitute in-memory database.\n");
                        return Err(self.exit(1));
                    }
                }
            }
        };
        let testing = self.has_flag(flag::TESTING_MODE);
        use rusqlite::config::DbConfig;
        let _ = conn.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, testing);
        let _ = conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, !testing);
        funcs::register_all(&conn);
        if self.open_mode == OpenMode::Deserialize {
            match sys::read_file(&name) {
                Ok(data) if !data.is_empty() => {
                    if let Err(e) = funcs::deserialize_main(&mut conn, &data) {
                        self.eputs(&format!("Error: sqlite3_deserialize() returns {e}\n"));
                    }
                }
                _ => {}
            }
        }
        self.db = Some(Db { conn: Some(conn), abandon: false });
        Ok(())
    }

    /// `close_db` + esquecer a conexão.
    pub fn close_db(&mut self) {
        if let Some(db) = self.db.take()
            && let Some(msg) = db.close()
        {
            self.eputs(&msg);
        }
    }

    /// `output_reset`: volta a saída pro stdout.
    pub fn output_reset(&mut self) {
        if let Some(mut r) = self.redirect.take() {
            r.close();
        }
    }

    /// `outputModePush`.
    pub fn mode_push(&mut self) {
        self.mode_prior = self.mode;
        self.prior_flags = self.flags;
        self.col_sep_prior = self.col_sep.clone();
        self.row_sep_prior = self.row_sep.clone();
    }

    /// `outputModePop`.
    pub fn mode_pop(&mut self) {
        self.mode = self.mode_prior;
        self.flags = self.prior_flags;
        self.col_sep = self.col_sep_prior.clone();
        self.row_sep = self.row_sep_prior.clone();
    }

    /// `echo_group_input`.
    fn echo(&mut self, z: &[u8]) {
        if self.has_flag(flag::ECHO) {
            self.oput(z);
            self.oput(b"\n");
        }
    }

    /// Sinal de interrupção (SIGINT) chegou desde a última olhada.
    pub fn poll_interrupt(&mut self) -> Result<bool, Exit> {
        let got = self.sys.take_caught_signals();
        let mut hit = false;
        let seen = funcs::take_interrupts();
        if seen > 0 {
            self.seen_interrupt += seen;
            hit = true;
            if self.seen_interrupt > 1 {
                return Err(self.exit(1));
            }
        }
        for s in got {
            if s == Signal::SIGINT {
                self.seen_interrupt += 1;
                hit = true;
                if self.seen_interrupt > 1 {
                    return Err(self.exit(1));
                }
            }
        }
        Ok(hit)
    }

    /// `runOneSqlLine`: devolve o número de erros (0 ou 1).
    pub fn run_one_sql_line(&mut self, sql: &[u8], from_file: bool, startline: usize) -> Result<usize, Exit> {
        self.open_db(false)?;
        let sql = if self.has_flag(flag::BACKSLASH) { text::resolve_backslashes(sql) } else { sql.to_vec() };
        let t0 = self.timer.then(|| exec::times(self));
        let r = exec::shell_exec(self, &sql)?;
        if let Some(t0) = t0 {
            exec::end_timer(self, t0);
        }
        if let Some(err) = r {
            let (kind, tail) = if let Some(t) = err.message.strip_prefix("in prepare, ") {
                ("Parse error", t.to_string())
            } else if let Some(t) = err.message.strip_prefix("stepping, ") {
                ("Runtime error", t.to_string())
            } else {
                ("Error", err.message.clone())
            };
            let prefix = if from_file || !self.interactive {
                format!("{kind} near line {startline}:")
            } else {
                format!("{kind}:")
            };
            self.eputs(&format!("{prefix} {tail}\n"));
            return Ok(1);
        }
        if self.has_flag(flag::COUNT_CHANGES) {
            let c = self.conn();
            let line = format!("changes: {}   total_changes: {}\n", c.changes(), c.total_changes());
            self.oputs(&line);
        }
        if exec::auto_detect_restore(self, &sql) {
            return Ok(1);
        }
        Ok(0)
    }

    /// `process_input`: lê comandos de `input` (stdin quando `stdin_tty`). Devolve se houve erro.
    pub fn process_input(&mut self, input: &mut LineReader, is_stdin: bool) -> Result<bool, Exit> {
        if self.input_nesting == 25 {
            self.eputs(&format!("Input nesting limit (25) reached at line {}. Check recursion.\n", self.lineno));
            return Ok(true);
        }
        self.input_nesting += 1;
        self.lineno = 0;
        let interactive_input = is_stdin && self.interactive;
        let from_file = !is_stdin;
        let mut sql: Vec<u8> = Vec::new();
        let mut startline = 0;
        let mut errcnt = 0usize;
        let mut qss = Qss::START;
        while errcnt == 0 || !self.bail || interactive_input {
            self.flush_out();
            if interactive_input {
                let prompt = if sql.is_empty() { self.prompt_main.clone() } else { self.prompt_cont.clone() };
                self.stdout.put(&prompt);
                self.stdout.flush();
            }
            let Some(mut line) = input.read_line() else {
                if interactive_input {
                    self.oput(b"\n");
                }
                break;
            };
            if self.poll_interrupt()? || self.seen_interrupt > 0 {
                if !interactive_input {
                    break;
                }
                self.seen_interrupt = 0;
            }
            self.lineno += 1;
            if qss.in_plain() && scan::line_is_command_terminator(&line) && scan::line_is_complete(&sql) {
                line = b";".to_vec();
            }
            qss = scan::quickscan(&line, qss);
            if qss.plain_white() && sql.is_empty() {
                self.echo(&line);
                qss = Qss::START;
                continue;
            }
            if sql.is_empty() && matches!(line.first(), Some(b'.' | b'#')) {
                self.echo(&line);
                if line[0] == b'.' {
                    match meta::do_meta_command(self, &line)? {
                        2 => break,
                        0 => {}
                        _ => errcnt += 1,
                    }
                }
                qss = Qss::START;
                continue;
            }
            if sql.is_empty() {
                let skip = line.iter().take_while(|&&c| is_space(c)).count();
                sql.extend_from_slice(&line[skip..]);
                startline = self.lineno;
            } else {
                sql.push(b'\n');
                sql.extend_from_slice(&line);
            }
            if !sql.is_empty() && qss.semi_term() && scan::complete(&sql) {
                let s = std::mem::take(&mut sql);
                self.echo(&s);
                errcnt += self.run_one_sql_line(&s, from_file, startline)?;
                if self.out_count > 0 {
                    self.output_reset();
                    self.out_count = 0;
                }
                self.safe_mode = self.safe_mode_persist;
                qss = Qss::START;
            } else if !sql.is_empty() && qss.plain_white() {
                let s = std::mem::take(&mut sql);
                self.echo(&s);
                qss = Qss::START;
            }
        }
        if !sql.is_empty() {
            let s = std::mem::take(&mut sql);
            self.echo(&s);
            errcnt += self.run_one_sql_line(&s, from_file, startline)?;
        }
        self.input_nesting -= 1;
        Ok(errcnt > 0)
    }

    /// `process_sqliterc`.
    fn process_sqliterc(&mut self, override_file: Option<&[u8]>) -> Result<(), Exit> {
        let path: Vec<u8> = match override_file {
            Some(f) => f.to_vec(),
            None => {
                let xdg = self.sys.getenv(b"XDG_CONFIG_HOME").map(|x| {
                    let mut p = x;
                    p.extend_from_slice(b"/sqlite3/sqliterc");
                    p
                });
                match xdg.filter(|p| sys::stat(p).is_ok()) {
                    Some(p) => p,
                    None => match self.home_dir() {
                        Some(mut h) => {
                            h.extend_from_slice(b"/.sqliterc");
                            h
                        }
                        None => {
                            self.eputs("-- warning: cannot find home directory; cannot read ~/.sqliterc\n");
                            return Ok(());
                        }
                    },
                }
            }
        };
        match sys::open(&path, OFlags::RDONLY | OFlags::CLOEXEC, 0) {
            Ok(fd) => {
                if self.interactive {
                    self.eputs(&format!("-- Loading resources from {}\n", String::from_utf8_lossy(&path)));
                }
                let saved = self.lineno;
                let mut r = LineReader::new(fd, true);
                let failed = self.process_input(&mut r, false)?;
                r.close();
                self.lineno = saved;
                if failed && self.bail {
                    return Err(self.exit(1));
                }
            }
            Err(_) => {
                if override_file.is_some() {
                    self.eputs(&format!("cannot open: \"{}\"\n", String::from_utf8_lossy(&path)));
                    if self.bail {
                        return Err(self.exit(1));
                    }
                }
            }
        }
        Ok(())
    }

    /// `find_home_dir`: o diretório do usuário no /etc/passwd do sandbox, senão `$HOME`.
    pub fn home_dir(&self) -> Option<Vec<u8>> {
        let uid = self.sys.getuid();
        if let Ok(pw) = sys::read_file(b"/etc/passwd") {
            for line in pw.split(|&b| b == b'\n') {
                let f: Vec<&[u8]> = line.split(|&b| b == b':').collect();
                if f.len() >= 6 && f[2] == uid.to_string().as_bytes() {
                    return Some(f[5].to_vec());
                }
            }
        }
        self.sys.getenv(b"HOME")
    }

    /// `cmdline_option_value`.
    fn option_value<'a>(&mut self, argv: &'a [Vec<u8>], i: usize) -> Result<&'a [u8], Exit> {
        match argv.get(i) {
            Some(v) => Ok(v),
            None => {
                let last = String::from_utf8_lossy(&argv[argv.len() - 1]).into_owned();
                self.eputs(&format!("{}: Error: missing argument to {}\n", String::from_utf8_lossy(&argv[0]), last));
                Err(self.exit(1))
            }
        }
    }

    fn usage(&mut self, detail: bool) -> Exit {
        self.eputs(&format!(
            "Usage: {} [OPTIONS] [FILENAME [SQL]]\nFILENAME is the name of an SQLite database. A new database is created\nif the file does not previously exist. Defaults to :memory:.\n",
            self.argv0
        ));
        if detail {
            self.eputs("OPTIONS include:\n");
            self.eputs(meta::OPTIONS_HELP);
        } else {
            self.eputs("Use the -help option for additional information\n");
        }
        self.exit(0)
    }

    /// O `main` do shell.c.
    pub fn run(&mut self, argv: &[Vec<u8>]) -> Result<i32, Exit> {
        self.interactive = self.sys.isatty(Fd::STDIN);
        let _ = self.sys.sigaction(Signal::SIGINT, SigDisposition::Catch);
        let mut db_name: Option<Vec<u8>> = None;
        let mut read_stdin = true;
        let mut cmds: Vec<Vec<u8>> = Vec::new();
        let mut opts_end = argv.len();
        let mut init_file: Option<Vec<u8>> = None;
        let mut warn_inmemory = false;
        // Primeira passada: nome do banco, arquivo de init, opções que afetam a abertura.
        let mut i = 1;
        while i < argv.len() {
            let z = &argv[i];
            if z.first() != Some(&b'-') || i > opts_end {
                if db_name.is_none() {
                    db_name = Some(z.clone());
                } else {
                    read_stdin = false;
                    cmds.push(z.clone());
                }
                i += 1;
                continue;
            }
            let z: &[u8] = if z.get(1) == Some(&b'-') { &z[1..] } else { z };
            match z {
                b"-" => opts_end = i,
                b"-separator" | b"-nullvalue" | b"-newline" | b"-cmd" => {
                    i += 1;
                    self.option_value(argv, i)?;
                }
                b"-init" => {
                    i += 1;
                    init_file = Some(self.option_value(argv, i)?.to_vec());
                }
                b"-batch" => self.interactive = false,
                b"-heap" | b"-mmap" | b"-vfs" | b"-sorterref" => {
                    i += 1;
                    let v = self.option_value(argv, i)?.to_vec();
                    if z == b"-vfs" && !vfs::VFS_ALIASES.iter().chain([&vfs::VFS_NAME]).any(|n| n.as_bytes() == v) && v != b"memdb" {
                        self.eputs(&format!("no such VFS: \"{}\"\n", String::from_utf8_lossy(&v)));
                        return Err(self.exit(1));
                    }
                }
                b"-pagecache" | b"-lookaside" => {
                    i += 1;
                    self.option_value(argv, i)?;
                    i += 1;
                    self.option_value(argv, i)?;
                }
                b"-threadsafe" => {
                    i += 1;
                    self.option_value(argv, i)?;
                }
                b"-deserialize" => self.open_mode = OpenMode::Deserialize,
                b"-maxsize" if i + 1 < argv.len() => i += 1,
                b"-readonly" => self.open_mode = OpenMode::Readonly,
                b"-nofollow" => self.nofollow = true,
                b"-bail" => self.bail = true,
                b"-nonce" => {
                    i += 1;
                    self.option_value(argv, i)?;
                }
                b"-unsafe-testing" => self.flags |= flag::TESTING_MODE,
                _ => {}
            }
            i += 1;
        }
        let db_name = match db_name {
            Some(n) => n,
            None => {
                warn_inmemory = argv.len() == 1;
                b":memory:".to_vec()
            }
        };
        self.db_filename = db_name.clone();
        // O banco só é aberto agora se já existe: assim um nome digitado errado não cria arquivo.
        if sys::stat(&db_name).is_ok() {
            self.open_db(false)?;
        }
        self.process_sqliterc(init_file.as_deref())?;
        // Segunda passada: as opções de verdade.
        let mut i = 1;
        while i < argv.len() {
            let z0 = &argv[i];
            if z0.first() != Some(&b'-') || i >= opts_end {
                i += 1;
                continue;
            }
            let z: &[u8] = if z0.get(1) == Some(&b'-') { &z0[1..] } else { z0 };
            match z {
                b"-init" => i += 1,
                b"-html" => self.mode = Mode::Html,
                b"-list" => self.mode = Mode::List,
                b"-quote" => {
                    self.mode = Mode::Quote;
                    self.col_sep = SEP_COMMA.to_vec();
                    self.row_sep = SEP_ROW.to_vec();
                }
                b"-line" => self.mode = Mode::Line,
                b"-column" => self.mode = Mode::Column,
                b"-json" => self.mode = Mode::Json,
                b"-markdown" => self.mode = Mode::Markdown,
                b"-table" => self.mode = Mode::Table,
                b"-box" => self.mode = Mode::Box,
                b"-csv" => {
                    self.mode = Mode::Csv;
                    self.col_sep = SEP_COMMA.to_vec();
                }
                b"-deserialize" => self.open_mode = OpenMode::Deserialize,
                b"-maxsize" if i + 1 < argv.len() => i += 1,
                b"-readonly" => self.open_mode = OpenMode::Readonly,
                b"-nofollow" => self.nofollow = true,
                b"-ascii" => {
                    self.mode = Mode::Ascii;
                    self.col_sep = SEP_UNIT.to_vec();
                    self.row_sep = SEP_RECORD.to_vec();
                }
                b"-tabs" => {
                    self.mode = Mode::List;
                    self.col_sep = SEP_TAB.to_vec();
                    self.row_sep = SEP_ROW.to_vec();
                }
                b"-separator" => {
                    i += 1;
                    self.col_sep = trunc19(self.option_value(argv, i)?);
                }
                b"-newline" => {
                    i += 1;
                    self.row_sep = trunc19(self.option_value(argv, i)?);
                }
                b"-nullvalue" => {
                    i += 1;
                    self.null_value = trunc19(self.option_value(argv, i)?);
                }
                b"-header" => {
                    self.show_header = true;
                    self.flags |= flag::HEADER_SET;
                }
                b"-noheader" => {
                    self.show_header = false;
                    self.flags |= flag::HEADER_SET;
                }
                b"-echo" => self.flags |= flag::ECHO,
                b"-eqp" => self.auto_eqp = 1,
                b"-eqpfull" => self.auto_eqp = 3,
                b"-stats" => self.stats_on = 1,
                b"-scanstats" => {}
                b"-backslash" => self.flags |= flag::BACKSLASH,
                b"-bail" => {}
                b"-version" => {
                    let v = format!("{} {} (64-bit)\n", rusqlite::version(), funcs::source_id());
                    self.oputs(&v);
                    return Ok(0);
                }
                b"-interactive" => self.interactive = true,
                b"-batch" | b"-utf8" | b"-no-utf8" | b"-no-rowid-in-view" | b"-memtrace" | b"-pcachetrace" => {}
                b"-heap" | b"-mmap" | b"-vfs" | b"-sorterref" => i += 1,
                b"-pagecache" | b"-lookaside" | b"-threadsafe" | b"-nonce" => i += 2,
                b"-help" => return Err(self.usage(true)),
                b"-cmd" => {
                    if i == argv.len() - 1 {
                        break;
                    }
                    i += 1;
                    let z = self.option_value(argv, i)?.to_vec();
                    if z.first() == Some(&b'.') {
                        let rc = meta::do_meta_command(self, &z)?;
                        if rc != 0 && self.bail {
                            return Ok(if rc == 2 { 0 } else { rc });
                        }
                    } else {
                        self.open_db(false)?;
                        match exec::shell_exec(self, &z)? {
                            Some(err) => {
                                self.eputs(&format!("Error: {}\n", err.message));
                                if self.bail {
                                    return Ok(if err.rc != 0 { err.rc } else { 1 });
                                }
                            }
                            None => {}
                        }
                    }
                }
                b"-safe" => {
                    self.safe_mode = true;
                    self.safe_mode_persist = true;
                }
                b"-unsafe-testing" => {}
                b"-A" | b"-zip" | b"-append" => {
                    // Arquivos zip e appendvfs dependem de módulos virtuais que o rusqlite só expõe por
                    // trait unsafe (ver STATUS.md).
                    self.eputs(&format!("{}: Error: unknown option: {}\n", self.argv0, String::from_utf8_lossy(z)));
                    self.eputs("Use -help for a list of options.\n");
                    return Ok(1);
                }
                _ => {
                    self.eputs(&format!("{}: Error: unknown option: {}\n", self.argv0, String::from_utf8_lossy(z)));
                    self.eputs("Use -help for a list of options.\n");
                    return Ok(1);
                }
            }
            self.c_mode = self.mode;
            i += 1;
        }
        if !read_stdin {
            for cmd in &cmds {
                if cmd.first() == Some(&b'.') {
                    let rc = meta::do_meta_command(self, cmd)?;
                    if rc != 0 {
                        return Ok(if rc == 2 { 0 } else { rc });
                    }
                } else {
                    self.open_db(false)?;
                    self.echo(cmd);
                    if let Some(err) = exec::shell_exec(self, cmd)? {
                        self.eputs(&format!("Error: {}\n", err.message));
                        return Ok(if err.rc != 0 { err.rc } else { 1 });
                    }
                }
            }
            self.clean_exit = true;
        } else if self.interactive {
            let banner = format!(
                "SQLite version {} {}\nEnter \".help\" for usage hints.\n",
                rusqlite::version(),
                &funcs::source_id()[..19]
            );
            self.oputs(&banner);
            if warn_inmemory {
                self.oputs("Connected to a \x1b[1mtransient in-memory database\x1b[0m.\nUse \".open FILENAME\" to reopen on a persistent database.\n");
            }
            let mut r = LineReader::new(Fd::STDIN, false);
            let failed = self.process_input(&mut r, true)?;
            self.clean_exit = true;
            return Ok(i32::from(failed));
        } else {
            let mut r = LineReader::new(Fd::STDIN, false);
            let failed = self.process_input(&mut r, true)?;
            self.clean_exit = true;
            return Ok(i32::from(failed));
        }
        Ok(0)
    }

    /// O fim do `main`: fecha o banco (se não for saída abrupta) e esvazia as saídas.
    pub fn finish(&mut self, abrupt: bool) {
        if abrupt {
            if let Some(db) = self.db.as_mut() {
                db.abandon = true;
            }
            self.db = None;
        } else {
            self.close_db();
        }
        self.output_reset();
        self.stdout.flush();
        if self.seen_interrupt > 0 {
            self.eputs("Program interrupted.\n");
        }
    }
}

fn trunc19(z: &[u8]) -> Vec<u8> {
    z[..z.len().min(19)].to_vec()
}

/// Abre uma conexão pelo nosso VFS, sem o busy timeout de 5 s que o rusqlite liga sozinho (o
/// sqlite3 real não tem nenhum: trava ocupada é "database is locked" na hora).
pub fn open_conn(name: &str, flags: OpenFlags) -> Result<Connection, String> {
    if let Err(code) = vfs::register() {
        return Err(format!("could not register VFS ({code})"));
    }
    let r = Connection::open_with_flags_and_vfs(name, flags | OpenFlags::SQLITE_OPEN_NO_MUTEX, vfs::VFS_NAME);
    unwind::reraise();
    match r {
        Ok(c) => {
            let _ = c.busy_timeout(std::time::Duration::ZERO);
            Ok(c)
        }
        Err(e) => {
            // O rusqlite acrescenta ": <caminho>" ao sqlite3_errmsg da abertura; o CLI mostra só o
            // errmsg.
            let msg = exec::error_parts(&e).1;
            let suffix = format!(": {name}");
            Err(msg.strip_suffix(&suffix).map(str::to_string).unwrap_or(msg))
        }
    }
}

/// O `main` do programa.
pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    vfs::set_abandoned(false);
    let argv: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
    let argv0 = argv.first().map(|a| String::from_utf8_lossy(a).into_owned()).unwrap_or_else(|| "sqlite3".into());
    let mut sh = Shell::new(argv0);
    match sh.run(&argv) {
        Ok(code) => {
            let abrupt = !sh.clean_exit || sh.db.as_ref().is_some_and(|d| d.abandon);
            sh.finish(abrupt);
            code
        }
        Err(Exit(code)) => {
            sh.finish(true);
            code
        }
    }
}

/// Códigos de erro do kernel que viram mensagem de arquivo (usado por `.read`, `.import`...).
pub fn errno_msg(e: Errno) -> String {
    e.message()
}
