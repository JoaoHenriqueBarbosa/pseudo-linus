//! Comandos de ponto (`do_meta_command` do shell.c 3.46.1), na mesma ordem de despacho e com as
//! mesmas abreviações aceitas.

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};
use sysabi::{Fd, FdAction, OFlags, Pid, ProcAttrs, SpawnSpec, WaitOptions, WaitStatus, WaitTarget, sys};

use super::exec::{self, Cell};
use super::help::{AZ_HELP, OPTIONS};
use super::out::{Sink, Stream};
use super::text::{self, boolean_value, cstr, integer_value};
use super::{ColModeOpts, Exit, LineReader, Mode, OpenMode, Shell, flag};
use super::{SEP_COLUMN, SEP_COMMA, SEP_CRLF, SEP_RECORD, SEP_ROW, SEP_SPACE, SEP_TAB, SEP_UNIT};
use crate::{funcs, unwind};

/// `zOptions` do `sqlite3 -help`.
pub const OPTIONS_HELP: &str = OPTIONS;

/// Divide a linha de um comando de ponto em argumentos, como o laço do começo do
/// `do_meta_command` (aspas simples literais, aspas duplas com escapes de barra).
pub fn tokenize(line: &[u8]) -> Vec<Vec<u8>> {
    let z = cstr(line);
    let mut args = Vec::new();
    let mut h = 1;
    while h < z.len() && args.len() < 51 {
        while h < z.len() && super::scan::is_space(z[h]) {
            h += 1;
        }
        if h >= z.len() {
            break;
        }
        if z[h] == b'\'' || z[h] == b'"' {
            let delim = z[h];
            h += 1;
            let start = h;
            while h < z.len() && z[h] != delim {
                if z[h] == b'\\' && delim == b'"' && h + 1 < z.len() {
                    h += 1;
                }
                h += 1;
            }
            let mut arg = z[start..h.min(z.len())].to_vec();
            if h < z.len() && z[h] == delim {
                h += 1;
            }
            if delim == b'"' {
                arg = text::resolve_backslashes(&arg);
            }
            args.push(arg);
        } else {
            let start = h;
            while h < z.len() && !super::scan::is_space(z[h]) {
                h += 1;
            }
            args.push(z[start..h].to_vec());
            if h < z.len() {
                h += 1;
            }
        }
    }
    args
}

/// `cli_strncmp(azArg[0], name, n)==0`: o comando é prefixo de `name`.
fn is(cmd: &[u8], name: &str) -> bool {
    !cmd.is_empty() && name.as_bytes().starts_with(cmd)
}

fn lossy(z: &[u8]) -> String {
    String::from_utf8_lossy(z).into_owned()
}

/// `optionMatch`.
fn option_match(z: &[u8], opt: &str) -> bool {
    let Some(rest) = z.strip_prefix(b"-") else { return false };
    let rest = rest.strip_prefix(b"-").unwrap_or(rest);
    rest == opt.as_bytes()
}

/// `showHelp`: devolve o número de comandos que casaram.
pub fn show_help(sh: &mut Shell, pattern: Option<&[u8]>) -> usize {
    let mut out = Vec::new();
    let mut n = 0;
    let all = match pattern {
        None => true,
        Some(p) => p.first() == Some(&b'0') || p == b"-a" || p == b"-all" || p == b"--all",
    };
    if all {
        let hw: u8 = match pattern {
            None => 1,
            Some(p) if p.first() == Some(&b'0') => 2,
            Some(_) => 0,
        };
        let mut hh: u8 = 0;
        for item in AZ_HELP {
            match item.as_bytes()[0] {
                b',' => hh = 3,
                b'.' => hh = 1,
                _ => hh &= !1,
            }
            if ((hw ^ hh) & 2) == 0 {
                if hh & 1 != 0 {
                    out.push(b'.');
                    out.extend_from_slice(&item.as_bytes()[1..]);
                    out.push(b'\n');
                    n += 1;
                } else if hw & 1 == 0 {
                    out.extend_from_slice(item.as_bytes());
                    out.push(b'\n');
                }
            }
        }
        sh.oput(&out);
        return n;
    }
    let pat = pattern.unwrap_or_default();
    let mut j = 0;
    for (i, item) in AZ_HELP.iter().enumerate() {
        let b = item.as_bytes();
        if b.first() == Some(&b'.') && b[1..].starts_with(pat) {
            out.extend_from_slice(b);
            out.push(b'\n');
            j = i + 1;
            n += 1;
        }
    }
    if n > 0 {
        if n == 1 {
            while j < AZ_HELP.len() - 1 && AZ_HELP[j].as_bytes()[0] == b' ' {
                out.extend_from_slice(AZ_HELP[j].as_bytes());
                out.push(b'\n');
                j += 1;
            }
        }
        sh.oput(&out);
        return n;
    }
    let needle = String::from_utf8_lossy(pat).to_ascii_lowercase();
    let mut i = 0;
    let mut j = 0;
    while i < AZ_HELP.len() {
        let item = AZ_HELP[i];
        if item.starts_with(',') {
            while i < AZ_HELP.len() - 1 && AZ_HELP[i + 1].starts_with(' ') {
                i += 1;
            }
            i += 1;
            continue;
        }
        if item.starts_with('.') {
            j = i;
        }
        if item.to_ascii_lowercase().contains(&needle) {
            out.extend_from_slice(AZ_HELP[j].as_bytes());
            out.push(b'\n');
            while j < AZ_HELP.len() - 1 && AZ_HELP[j + 1].starts_with(' ') {
                j += 1;
                out.extend_from_slice(AZ_HELP[j].as_bytes());
                out.push(b'\n');
            }
            i = j;
            n += 1;
        }
        i += 1;
    }
    sh.oput(&out);
    n
}

/// `failIfSafeMode`.
fn fail_if_safe(sh: &mut Shell, msg: &str) -> Result<(), Exit> {
    if sh.safe_mode {
        sh.eputs(&format!("line {}: {msg}\n", sh.lineno));
        return Err(sh.exit(1));
    }
    Ok(())
}

/// Linhas de uma consulta interna, como texto (`sqlite3_column_text`).
pub fn query_text(conn: &Connection, sql: &str) -> rusqlite::Result<Vec<Vec<Option<Vec<u8>>>>> {
    let r = (|| {
        let mut st = conn.prepare(sql)?;
        let n = st.column_count();
        let mut rows = st.raw_query();
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let mut cells = Vec::with_capacity(n);
            for i in 0..n {
                let c = row.get_ref(i).map(|v| match v {
                    rusqlite::types::ValueRef::Null => Cell::Null,
                    rusqlite::types::ValueRef::Integer(i) => Cell::Int(i),
                    rusqlite::types::ValueRef::Real(f) => Cell::Real(f),
                    rusqlite::types::ValueRef::Text(t) => Cell::Text(t.to_vec()),
                    rusqlite::types::ValueRef::Blob(b) => Cell::Blob(b.to_vec()),
                });
                cells.push(c.ok().and_then(|c| c.text()));
            }
            out.push(cells);
        }
        Ok(out)
    })();
    unwind::reraise();
    r
}

/// `sqlite3_exec` sem callback: roda e ignora o resultado.
fn exec_quiet(conn: &Connection, sql: &str) -> rusqlite::Result<()> {
    let r = conn.execute_batch(sql);
    unwind::reraise();
    r
}

/// `popen(cmd, mode)` sobre o `/bin/sh` do sandbox: devolve o fd do nosso lado e o pid.
pub fn popen(cmd: &[u8], write: bool) -> Option<(Fd, Pid)> {
    let s = sys::current();
    let (r, w) = s.pipe2(OFlags::CLOEXEC).ok()?;
    let (ours, theirs, target) = if write { (w, r, Fd::STDIN) } else { (r, w, Fd::STDOUT) };
    let spec = SpawnSpec {
        path: b"/bin/sh".to_vec(),
        argv: vec![b"sh".to_vec(), b"-c".to_vec(), cmd.to_vec()],
        attrs: ProcAttrs { fd_actions: vec![FdAction::Dup2 { from: theirs, to: target }], ..ProcAttrs::default() },
    };
    let pid = s.spawn(spec);
    let _ = s.close(theirs);
    match pid {
        Ok(pid) => Some((ours, pid)),
        Err(_) => {
            let _ = s.close(ours);
            None
        }
    }
}

/// Espera um filho e devolve o status no formato do `system()`/`pclose()` (código << 8).
pub fn wait_status(pid: Pid) -> i32 {
    let s = sys::current();
    loop {
        match s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()) {
            Err(sysabi::Errno::EINTR) => continue,
            Ok(Some((_, WaitStatus::Exited(c)))) => return (c & 0xff) << 8,
            Ok(Some((_, WaitStatus::Signaled { signal, .. }))) => return signal.0,
            _ => return -1,
        }
    }
}

/// Executa o comando de ponto. 0 = ok, 1 = erro, 2 = sair.
pub fn do_meta_command(sh: &mut Shell, line: &[u8]) -> Result<i32, Exit> {
    let args = tokenize(line);
    if args.is_empty() {
        return Ok(0);
    }
    let rc = dispatch(sh, &args)?;
    if sh.out_count > 0 {
        sh.out_count -= 1;
        if sh.out_count == 0 {
            sh.output_reset();
        }
    }
    sh.safe_mode = sh.safe_mode_persist;
    Ok(rc)
}

fn dispatch(sh: &mut Shell, args: &[Vec<u8>]) -> Result<i32, Exit> {
    let cmd = args[0].as_slice();
    let n = cmd.len();
    let c = cmd[0];
    let nargs = args.len();
    let mut rc = 0;
    if c == b'a' && is(cmd, "auth") {
        if nargs != 2 {
            sh.eputs("Usage: .auth ON|OFF\n");
            return Ok(1);
        }
        sh.open_db(false)?;
        let (on, warn) = boolean_value(&args[1]);
        if let Some(w) = warn {
            sh.eputs(&w);
        }
        funcs::set_auth_trace(on != 0);
    } else if c == b'a' && is(cmd, "archive") {
        sh.open_db(false)?;
        fail_if_safe(sh, "cannot run .archive in safe mode")?;
        sh.eputs("Error: .archive needs the zipfile virtual table, which this build does not provide\n");
        rc = 1;
    } else if (c == b'b' && n >= 3 && is(cmd, "backup")) || (c == b's' && n >= 3 && is(cmd, "save")) {
        fail_if_safe(sh, &format!("cannot run .{} in safe mode", lossy(cmd)))?;
        let mut dest: Option<&[u8]> = None;
        let mut db: Option<&[u8]> = None;
        let mut async_ = false;
        for a in &args[1..] {
            if a.first() == Some(&b'-') {
                let z = if a.get(1) == Some(&b'-') { &a[1..] } else { &a[..] };
                if z == b"-async" {
                    async_ = true;
                } else {
                    sh.eputs(&format!("unknown option: {}\n", lossy(a)));
                    return Ok(1);
                }
            } else if dest.is_none() {
                dest = Some(a);
            } else if db.is_none() {
                db = dest;
                dest = Some(a);
            } else {
                sh.eputs("Usage: .backup ?DB? ?OPTIONS? FILENAME\n");
                return Ok(1);
            }
        }
        let Some(dest) = dest else {
            sh.eputs("missing FILENAME argument on .backup\n");
            return Ok(1);
        };
        let db = lossy(db.unwrap_or(b"main"));
        let dest_name = lossy(dest);
        let mut pdest = match super::open_conn(&dest_name, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE) {
            Ok(c) => c,
            Err(_) => {
                sh.eputs(&format!("Error: cannot open \"{dest_name}\"\n"));
                return Ok(1);
            }
        };
        if async_ {
            let _ = exec_quiet(&pdest, "PRAGMA synchronous=OFF; PRAGMA journal_mode=OFF;");
        }
        sh.open_db(false)?;
        let r = funcs::backup(sh.conn(), &db, &mut pdest, "main", false);
        if let Err(msg) = r {
            sh.eputs(&format!("Error: {msg}\n"));
            rc = 1;
        }
        let _ = pdest.close();
        unwind::reraise();
    } else if c == b'b' && n >= 3 && is(cmd, "bail") {
        if nargs == 2 {
            let (v, w) = boolean_value(&args[1]);
            if let Some(w) = w {
                sh.eputs(&w);
            }
            sh.bail = v != 0;
        } else {
            sh.eputs("Usage: .bail on|off\n");
            rc = 1;
        }
    } else if c == b'b' && n >= 3 && is(cmd, "binary") {
        if nargs == 2 {
            let (_, w) = boolean_value(&args[1]);
            if let Some(w) = w {
                sh.eputs(&w);
            }
        } else {
            sh.eputs("The \".binary\" command is deprecated. Use \".crnl\" instead.\nUsage: .binary on|off\n");
            rc = 1;
        }
    } else if c == b'b' && n >= 3 && is(cmd, "breakpoint") {
    } else if c == b'c' && cmd == b"cd" {
        fail_if_safe(sh, "cannot run .cd in safe mode")?;
        if nargs == 2 {
            if sh.sys.chdir(&args[1]).is_err() {
                sh.eputs(&format!("Cannot change to directory \"{}\"\n", lossy(&args[1])));
                rc = 1;
            }
        } else {
            sh.eputs("Usage: .cd DIRECTORY\n");
            rc = 1;
        }
    } else if c == b'c' && n >= 3 && is(cmd, "changes") {
        if nargs == 2 {
            let (v, w) = boolean_value(&args[1]);
            if let Some(w) = w {
                sh.eputs(&w);
            }
            sh.set_flag(flag::COUNT_CHANGES, v != 0);
        } else {
            sh.eputs("Usage: .changes on|off\n");
            rc = 1;
        }
    } else if c == b'c' && n >= 3 && is(cmd, "check") {
        sh.output_reset();
        if nargs != 2 {
            sh.eputs("Usage: .check GLOB-PATTERN\n");
        }
        rc = 2;
    } else if c == b'c' && is(cmd, "clone") {
        fail_if_safe(sh, "cannot run .clone in safe mode")?;
        if nargs == 2 {
            sh.eputs("Error: .clone is not supported by this build\n");
        } else {
            sh.eputs("Usage: .clone FILENAME\n");
            rc = 1;
        }
    } else if c == b'c' && is(cmd, "connection") {
        if nargs == 1 {
            let name = if sh.db_filename.is_empty() { "(temporary-file)".to_string() } else { lossy(&sh.db_filename) };
            sh.stdout.put(format!("ACTIVE 0: {name}\n").as_bytes());
        } else {
            sh.eputs("Usage: .connection [close] [CONNECTION-NUMBER]\n");
            rc = 1;
        }
    } else if c == b'c' && n == 4 && is(cmd, "crnl") {
        if nargs == 2 {
            let (_, w) = boolean_value(&args[1]);
            if let Some(w) = w {
                sh.eputs(&w);
            }
        } else {
            sh.eputs("The \".crnl\" is a no-op on non-Windows machines.\nUsage: .crnl on|off\n");
            rc = 1;
        }
    } else if c == b'd' && n > 1 && is(cmd, "databases") {
        sh.open_db(false)?;
        rc = dot_databases(sh);
    } else if c == b'd' && n >= 3 && is(cmd, "dbconfig") {
        sh.open_db(false)?;
        dot_dbconfig(sh, args);
    } else if c == b'd' && n >= 3 && is(cmd, "dbinfo") {
        sh.open_db(false)?;
        rc = funcs::dbinfo(sh, args);
    } else if c == b'r' && is(cmd, "recover") {
        sh.open_db(false)?;
        sh.eputs("Error: .recover is not supported by this build\n");
        rc = 1;
    } else if c == b'd' && is(cmd, "dump") {
        rc = dot_dump(sh, args)?;
    } else if c == b'e' && is(cmd, "echo") {
        if nargs == 2 {
            let (v, w) = boolean_value(&args[1]);
            if let Some(w) = w {
                sh.eputs(&w);
            }
            sh.set_flag(flag::ECHO, v != 0);
        } else {
            sh.eputs("Usage: .echo on|off\n");
            rc = 1;
        }
    } else if c == b'e' && is(cmd, "eqp") {
        if nargs == 2 {
            sh.auto_eqp = match args[1].as_slice() {
                b"full" => 3,
                b"trigger" => 2,
                other => {
                    let (v, w) = boolean_value(other);
                    if let Some(w) = w {
                        sh.eputs(&w);
                    }
                    v as u8
                }
            };
        } else {
            sh.eputs("Usage: .eqp off|on|trace|trigger|full\n");
            rc = 1;
        }
    } else if c == b'e' && is(cmd, "exit") {
        if nargs > 1 {
            let code = integer_value(&args[1]) as i32;
            if code != 0 {
                return Err(sh.exit(code));
            }
        }
        rc = 2;
    } else if c == b'e' && is(cmd, "explain") {
        let mut val = 1;
        if nargs >= 2 {
            if args[1] == b"auto" {
                val = 99;
            } else {
                let (v, w) = boolean_value(&args[1]);
                if let Some(w) = w {
                    sh.eputs(&w);
                }
                val = v;
            }
        }
        if val == 1 && sh.mode != Mode::Explain {
            sh.normal_mode = sh.mode;
            sh.mode = Mode::Explain;
            sh.auto_explain = false;
        } else if val == 0 {
            if sh.mode == Mode::Explain {
                sh.mode = sh.normal_mode;
            }
            sh.auto_explain = false;
        } else if val == 99 {
            if sh.mode == Mode::Explain {
                sh.mode = sh.normal_mode;
            }
            sh.auto_explain = true;
        }
    } else if c == b'e' && is(cmd, "expert") {
        if sh.safe_mode {
            sh.eputs(&format!("Cannot run experimental commands such as \"{}\" in safe mode\n", lossy(cmd)));
            rc = 1;
        } else {
            sh.open_db(false)?;
            sh.eputs("sqlite3_expert_new: not supported by this build\n");
        }
    } else if c == b'f' && is(cmd, "filectrl") {
        sh.open_db(false)?;
        sh.oputs("Available file-controls:\n");
        for (name, usage) in [
            ("chunk_size", "SIZE"),
            ("data_version", ""),
            ("has_moved", ""),
            ("lock_timeout", "MILLISEC"),
            ("persist_wal", "[BOOLEAN]"),
            ("psow", "[BOOLEAN]"),
            ("reserve_bytes", "[N]"),
            ("size_limit", "[LIMIT]"),
            ("tempfilename", ""),
        ] {
            sh.oputs(&format!("  .filectrl {name} {usage}\n"));
        }
        rc = 1;
    } else if c == b'f' && is(cmd, "fullschema") {
        rc = dot_fullschema(sh, args)?;
    } else if c == b'h' && is(cmd, "headers") {
        if nargs == 2 {
            let (v, w) = boolean_value(&args[1]);
            if let Some(w) = w {
                sh.eputs(&w);
            }
            sh.show_header = v != 0;
            sh.flags |= flag::HEADER_SET;
        } else {
            sh.eputs("Usage: .headers on|off\n");
            rc = 1;
        }
    } else if c == b'h' && is(cmd, "help") {
        if nargs >= 2 {
            let n = show_help(sh, Some(&args[1]));
            if n == 0 {
                sh.oputs(&format!("Nothing matches '{}'\n", lossy(&args[1])));
            }
        } else {
            show_help(sh, None);
        }
    } else if c == b'i' && is(cmd, "import") {
        rc = dot_import(sh, args)?;
    } else if c == b'i' && is(cmd, "imposter") {
        sh.eputs("Usage: .imposter INDEX IMPOSTER\n");
        rc = 1;
    } else if c == b'i' && is(cmd, "intck") {
        sh.eputs("Error: .intck is not supported by this build\n");
        rc = 1;
    } else if c == b'i' && is(cmd, "iotrace") {
        sh.eputs("Error: unknown command or invalid arguments:  \"iotrace\". Enter \".help\" for help\n");
        rc = 1;
    } else if c == b'l' && n >= 5 && is(cmd, "limits") {
        sh.open_db(false)?;
        rc = dot_limits(sh, args);
    } else if c == b'l' && n > 2 && is(cmd, "lint") {
        sh.open_db(false)?;
        sh.eputs(&format!("Usage {} sub-command ?switches...?\nWhere sub-commands are:\n    fkey-indexes\n", lossy(cmd)));
        rc = 1;
    } else if c == b'l' && is(cmd, "load") {
        fail_if_safe(sh, "cannot run .load in safe mode")?;
        if nargs < 2 || args[1].is_empty() {
            sh.eputs("Usage: .load FILE ?ENTRYPOINT?\n");
            return Ok(1);
        }
        sh.open_db(false)?;
        sh.eputs(&format!("Error: {}\n", funcs::load_extension_error(&args[1])));
        rc = 1;
    } else if c == b'l' && is(cmd, "log") {
        if nargs != 2 {
            sh.eputs("Usage: .log FILENAME\n");
            rc = 1;
        }
    } else if c == b'm' && is(cmd, "mode") {
        rc = dot_mode(sh, args);
    } else if c == b'n' && cmd == b"nonce" {
        if nargs != 2 {
            sh.eputs("Usage: .nonce NONCE\n");
            rc = 1;
        } else {
            sh.eputs(&format!("line {}: incorrect nonce: \"{}\"\n", sh.lineno, lossy(&args[1])));
            return Err(sh.exit(1));
        }
    } else if c == b'n' && is(cmd, "nullvalue") {
        if nargs == 2 {
            sh.null_value = args[1][..args[1].len().min(19)].to_vec();
        } else {
            sh.eputs("Usage: .nullvalue STRING\n");
            rc = 1;
        }
    } else if c == b'o' && is(cmd, "open") && n >= 2 {
        rc = dot_open(sh, args)?;
    } else if (c == b'o' && (is(cmd, "output") || is(cmd, "once"))) || (c == b'e' && n == 5 && cmd == b"excel") {
        rc = dot_output(sh, args)?;
    } else if c == b'p' && n >= 3 && is(cmd, "parameter") {
        rc = dot_parameter(sh, args)?;
    } else if c == b'p' && n >= 3 && is(cmd, "print") {
        let mut o = Vec::new();
        for (i, a) in args[1..].iter().enumerate() {
            if i > 0 {
                o.push(b' ');
            }
            o.extend_from_slice(a);
        }
        o.push(b'\n');
        sh.oput(&o);
    } else if c == b'p' && n >= 3 && is(cmd, "progress") {
        rc = dot_progress(sh, args)?;
    } else if c == b'p' && is(cmd, "prompt") {
        if nargs >= 2 {
            sh.prompt_main = args[1][..args[1].len().min(19)].to_vec();
        }
        if nargs >= 3 {
            sh.prompt_cont = args[2][..args[2].len().min(19)].to_vec();
        }
    } else if c == b'q' && is(cmd, "quit") {
        rc = 2;
    } else if c == b'r' && n >= 3 && is(cmd, "read") {
        rc = dot_read(sh, args)?;
    } else if c == b'r' && n >= 3 && is(cmd, "restore") {
        fail_if_safe(sh, "cannot run .restore in safe mode")?;
        let (src, db) = match nargs {
            2 => (lossy(&args[1]), "main".to_string()),
            3 => (lossy(&args[2]), lossy(&args[1])),
            _ => {
                sh.eputs("Usage: .restore ?DB? FILE\n");
                return Ok(1);
            }
        };
        let psrc = match super::open_conn(&src, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE) {
            Ok(c) => c,
            Err(_) => {
                sh.eputs(&format!("Error: cannot open \"{src}\"\n"));
                return Ok(1);
            }
        };
        sh.open_db(false)?;
        if let Err(msg) = funcs::backup(&psrc, "main", sh.conn_mut(), &db, true) {
            sh.eputs(&format!("Error: {msg}\n"));
            rc = 1;
        }
        let _ = psrc.close();
        unwind::reraise();
    } else if c == b's' && is(cmd, "scanstats") {
        if nargs == 2 {
            sh.open_db(false)?;
        } else {
            sh.eputs("Usage: .scanstats on|off|est\n");
            rc = 1;
        }
    } else if c == b's' && is(cmd, "schema") {
        rc = dot_schema(sh, args)?;
    } else if (c == b's' && n == 11 && cmd == b"selecttrace") || (c == b't' && n == 9 && cmd == b"treetrace") {
    } else if c == b's' && is(cmd, "session") && n >= 3 {
        sh.eputs("Usage: .session ?NAME? CMD ...\n");
        rc = 1;
    } else if c == b's' && n >= 4 && is(cmd, "selftest") {
        sh.eputs("Error: .selftest is not supported by this build\n");
        rc = 1;
    } else if c == b's' && is(cmd, "separator") {
        if !(2..=3).contains(&nargs) {
            sh.eputs("Usage: .separator COL ?ROW?\n");
            rc = 1;
        }
        if nargs >= 2 {
            sh.col_sep = args[1][..args[1].len().min(19)].to_vec();
        }
        if nargs >= 3 {
            sh.row_sep = args[2][..args[2].len().min(19)].to_vec();
        }
    } else if c == b's' && n >= 4 && is(cmd, "sha3sum") {
        sh.open_db(false)?;
        rc = funcs::sha3sum_command(sh, args);
    } else if c == b's' && (is(cmd, "shell") || is(cmd, "system")) {
        fail_if_safe(sh, &format!("cannot run .{} in safe mode", lossy(cmd)))?;
        if nargs < 2 {
            sh.eputs("Usage: .system COMMAND\n");
            return Ok(1);
        }
        let mut z = Vec::new();
        for (i, a) in args[1..].iter().enumerate() {
            if i > 0 {
                z.push(b' ');
            }
            if a.contains(&b' ') {
                z.push(b'"');
                z.extend_from_slice(a);
                z.push(b'"');
            } else {
                z.extend_from_slice(a);
            }
        }
        let x = popen_system(&z).unwrap_or(127 << 8);
        if x != 0 {
            sh.eputs(&format!("System command returns {x}\n"));
        }
    } else if c == b's' && is(cmd, "show") {
        if nargs != 1 {
            sh.eputs("Usage: .show\n");
            return Ok(1);
        }
        dot_show(sh);
    } else if c == b's' && is(cmd, "stats") {
        if nargs == 2 {
            sh.stats_on = match args[1].as_slice() {
                b"stmt" => 2,
                b"vmstep" => 3,
                other => boolean_value(other).0 as u32,
            };
        } else if nargs != 1 {
            sh.eputs("Usage: .stats ?on|off|stmt|vmstep?\n");
            rc = 1;
        }
    } else if (c == b't' && n > 1 && is(cmd, "tables")) || (c == b'i' && (is(cmd, "indices") || is(cmd, "indexes"))) {
        rc = dot_tables(sh, args, c == b'i')?;
    } else if c == b't' && cmd == b"testcase" {
        sh.eputs("Usage: .testcase NAME\n");
        rc = 1;
    } else if c == b't' && n >= 8 && is(cmd, "testctrl") {
        sh.eputs("Error: .testctrl is not supported by this build\n");
        rc = 1;
    } else if c == b't' && n > 4 && is(cmd, "timeout") {
        sh.open_db(false)?;
        let ms = if nargs >= 2 { integer_value(&args[1]) as i32 } else { 0 };
        funcs::set_busy_timeout(sh.conn(), ms);
    } else if c == b't' && n >= 5 && is(cmd, "timer") {
        if nargs == 2 {
            let (v, w) = boolean_value(&args[1]);
            if let Some(w) = w {
                sh.eputs(&w);
            }
            sh.timer = v != 0;
        } else {
            sh.eputs("Usage: .timer on|off\n");
            rc = 1;
        }
    } else if c == b't' && is(cmd, "trace") {
        sh.open_db(false)?;
    } else if c == b'u' && is(cmd, "unmodule") {
        sh.eputs("Error: unknown command or invalid arguments:  \"unmodule\". Enter \".help\" for help\n");
        rc = 1;
    } else if c == b'u' && is(cmd, "user") {
        sh.eputs("Usage: .user login|add|edit|delete ...\n");
        rc = 1;
    } else if c == b'v' && is(cmd, "version") {
        let v = format!(
            "SQLite {} {}\nzlib version 1.3.1\ngcc-14.2.0 (64-bit)\n",
            rusqlite::version(),
            funcs::source_id()
        );
        sh.oputs(&v);
    } else if c == b'v' && is(cmd, "vfsinfo") {
        if sh.db.is_some() {
            sh.oputs("vfs.zName      = \"unix\"\nvfs.iVersion   = 3\nvfs.szOsFile   = 120\nvfs.mxPathname = 512\n");
        }
    } else if c == b'v' && is(cmd, "vfslist") {
        let cur = if sh.db.is_some() { "  <--- CURRENT" } else { "" };
        let mut o = format!("vfs.zName      = \"unix\"{cur}\nvfs.iVersion   = 3\nvfs.szOsFile   = 120\nvfs.mxPathname = 512\n");
        for name in super::super::vfs::VFS_ALIASES.iter().rev() {
            o.push_str(&format!(
                "-----------------------------------\nvfs.zName      = \"{name}\"\nvfs.iVersion   = 3\nvfs.szOsFile   = 120\nvfs.mxPathname = 512\n"
            ));
        }
        o.push_str("-----------------------------------\nvfs.zName      = \"memdb\"\nvfs.iVersion   = 2\nvfs.szOsFile   = 32\nvfs.mxPathname = 1024\n");
        sh.oputs(&o);
    } else if c == b'v' && is(cmd, "vfsname") {
        if sh.db.is_some() {
            sh.oputs("unix\n");
        }
    } else if c == b'w' && is(cmd, "wheretrace") {
    } else if c == b'w' && is(cmd, "width") {
        sh.col_width = args[1..].iter().map(|a| integer_value(a) as i32).collect();
    } else {
        sh.eputs(&format!("Error: unknown command or invalid arguments:  \"{}\". Enter \".help\" for help\n", lossy(cmd)));
        rc = 1;
    }
    Ok(rc)
}

/// `system()` de uma linha de comando pelo `/bin/sh` do sandbox.
fn popen_system(cmd: &[u8]) -> Option<i32> {
    let s = sys::current();
    let spec = SpawnSpec {
        path: b"/bin/sh".to_vec(),
        argv: vec![b"sh".to_vec(), b"-c".to_vec(), cmd.to_vec()],
        attrs: ProcAttrs::default(),
    };
    let pid = s.spawn(spec).ok()?;
    Some(wait_status(pid))
}

fn dot_databases(sh: &mut Shell) -> i32 {
    let rows = match query_text(sh.conn(), "PRAGMA database_list") {
        Ok(r) => r,
        Err(e) => {
            sh.eputs(&format!("Error: {}\n", exec::error_parts(&e).1));
            return 1;
        }
    };
    let mut out = Vec::new();
    for r in rows {
        let (Some(Some(schema)), Some(Some(file))) = (r.get(1), r.get(2)) else { continue };
        let name = lossy(schema);
        let (ro, txn) = funcs::db_state(sh.conn(), &name);
        out.extend_from_slice(schema);
        out.extend_from_slice(b": ");
        if file.is_empty() {
            out.extend_from_slice(b"\"\"");
        } else {
            out.extend_from_slice(file);
        }
        out.extend_from_slice(if ro { b" r/o" } else { b" r/w" });
        out.extend_from_slice(match txn {
            0 => b"".as_slice(),
            1 => b" read-txn",
            _ => b" write-txn",
        });
        out.push(b'\n');
    }
    sh.oput(&out);
    0
}

fn dot_dbconfig(sh: &mut Shell, args: &[Vec<u8>]) {
    use rusqlite::config::DbConfig;
    let choices: &[(&str, Option<DbConfig>)] = &[
        ("defensive", Some(DbConfig::SQLITE_DBCONFIG_DEFENSIVE)),
        ("dqs_ddl", Some(DbConfig::SQLITE_DBCONFIG_DQS_DDL)),
        ("dqs_dml", Some(DbConfig::SQLITE_DBCONFIG_DQS_DML)),
        ("enable_fkey", Some(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY)),
        ("enable_qpsg", Some(DbConfig::SQLITE_DBCONFIG_ENABLE_QPSG)),
        ("enable_trigger", Some(DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER)),
        ("enable_view", Some(DbConfig::SQLITE_DBCONFIG_ENABLE_VIEW)),
        ("fts3_tokenizer", Some(DbConfig::SQLITE_DBCONFIG_ENABLE_FTS3_TOKENIZER)),
        ("legacy_alter_table", Some(DbConfig::SQLITE_DBCONFIG_LEGACY_ALTER_TABLE)),
        ("legacy_file_format", Some(DbConfig::SQLITE_DBCONFIG_LEGACY_FILE_FORMAT)),
        ("load_extension", None),
        ("no_ckpt_on_close", Some(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE)),
        ("reset_database", Some(DbConfig::SQLITE_DBCONFIG_RESET_DATABASE)),
        ("reverse_scanorder", Some(DbConfig::SQLITE_DBCONFIG_REVERSE_SCANORDER)),
        ("stmt_scanstatus", Some(DbConfig::SQLITE_DBCONFIG_STMT_SCANSTATUS)),
        ("trigger_eqp", Some(DbConfig::SQLITE_DBCONFIG_TRIGGER_EQP)),
        ("trusted_schema", Some(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA)),
        ("writable_schema", Some(DbConfig::SQLITE_DBCONFIG_WRITABLE_SCHEMA)),
    ];
    let mut found = false;
    for (name, op) in choices {
        if args.len() > 1 && args[1] != name.as_bytes() {
            continue;
        }
        found = true;
        let conn = sh.conn();
        let v = match op {
            Some(op) => {
                if args.len() >= 3 {
                    let _ = conn.set_db_config(*op, boolean_value(&args[2]).0 != 0);
                }
                conn.db_config(*op).unwrap_or(false)
            }
            // A carga de extensão nunca liga no sandbox (abriria biblioteca do host).
            None => true,
        };
        sh.oputs(&format!("{name:>19} {}\n", if v { "on" } else { "off" }));
        if args.len() > 1 {
            break;
        }
    }
    if args.len() > 1 && !found {
        sh.eputs(&format!("Error: unknown dbconfig \"{}\"\nEnter \".dbconfig\" with no arguments for a list\n", lossy(&args[1])));
    }
}

fn dot_limits(sh: &mut Shell, args: &[Vec<u8>]) -> i32 {
    use rusqlite::limits::Limit;
    let limits: &[(&str, Limit)] = &[
        ("length", Limit::SQLITE_LIMIT_LENGTH),
        ("sql_length", Limit::SQLITE_LIMIT_SQL_LENGTH),
        ("column", Limit::SQLITE_LIMIT_COLUMN),
        ("expr_depth", Limit::SQLITE_LIMIT_EXPR_DEPTH),
        ("compound_select", Limit::SQLITE_LIMIT_COMPOUND_SELECT),
        ("vdbe_op", Limit::SQLITE_LIMIT_VDBE_OP),
        ("function_arg", Limit::SQLITE_LIMIT_FUNCTION_ARG),
        ("attached", Limit::SQLITE_LIMIT_ATTACHED),
        ("like_pattern_length", Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH),
        ("variable_number", Limit::SQLITE_LIMIT_VARIABLE_NUMBER),
        ("trigger_depth", Limit::SQLITE_LIMIT_TRIGGER_DEPTH),
        ("worker_threads", Limit::SQLITE_LIMIT_WORKER_THREADS),
    ];
    let conn = sh.conn();
    if args.len() == 1 {
        let mut o = String::new();
        for (name, l) in limits {
            o.push_str(&format!("{name:>20} {}\n", conn.limit(*l).unwrap_or(-1)));
        }
        sh.stdout.put(o.as_bytes());
        return 0;
    }
    if args.len() > 3 {
        sh.eputs("Usage: .limit NAME ?NEW-VALUE?\n");
        return 1;
    }
    let pat = String::from_utf8_lossy(&args[1]).to_ascii_lowercase();
    let mut hit: Option<usize> = None;
    for (i, (name, _)) in limits.iter().enumerate() {
        if name.starts_with(&pat) {
            if hit.is_some() {
                sh.eputs(&format!("ambiguous limit: \"{}\"\n", lossy(&args[1])));
                return 1;
            }
            hit = Some(i);
        }
    }
    let Some(i) = hit else {
        sh.eputs(&format!("unknown limit: \"{}\"\nenter \".limits\" with no arguments for a list.\n", lossy(&args[1])));
        return 1;
    };
    if args.len() == 3 {
        let _ = conn.set_limit(limits[i].1, integer_value(&args[2]) as i32);
    }
    let v = conn.limit(limits[i].1).unwrap_or(-1);
    sh.stdout.put(format!("{:>20} {v}\n", limits[i].0).as_bytes());
    0
}

fn dot_mode(sh: &mut Shell, args: &[Vec<u8>]) -> i32 {
    let mut mode: Option<Vec<u8>> = None;
    let mut tabname: Option<Vec<u8>> = None;
    let mut opts = ColModeOpts::DEFAULT;
    let mut i = 1;
    while i < args.len() {
        let z = &args[i];
        if option_match(z, "wrap") && i + 1 < args.len() {
            i += 1;
            opts.wrap = integer_value(&args[i]) as i32;
        } else if option_match(z, "ww") {
            opts.word_wrap = true;
        } else if option_match(z, "wordwrap") && i + 1 < args.len() {
            i += 1;
            opts.word_wrap = boolean_value(&args[i]).0 != 0;
        } else if option_match(z, "quote") {
            opts.quote = true;
        } else if option_match(z, "noquote") {
            opts.quote = false;
        } else if mode.is_none() {
            if z == b"qbox" {
                mode = Some(b"box".to_vec());
                opts = ColModeOpts::QBOX;
            } else {
                mode = Some(z.clone());
            }
        } else if tabname.is_none() {
            tabname = Some(z.clone());
        } else if z.first() == Some(&b'-') {
            sh.eputs(&format!(
                "unknown option: {}\noptions:\n  --noquote\n  --quote\n  --wordwrap on/off\n  --wrap N\n  --ww\n",
                lossy(z)
            ));
            return 1;
        } else {
            sh.eputs(&format!("extra argument: \"{}\"\n", lossy(z)));
            return 1;
        }
        i += 1;
    }
    let mode = match mode {
        Some(m) => m,
        None => {
            let line = if sh.mode.is_columnar() {
                format!(
                    "current output mode: {} --wrap {} --wordwrap {} --{}quote\n",
                    sh.mode.descr(),
                    sh.cm_opts.wrap,
                    if sh.cm_opts.word_wrap { "on" } else { "off" },
                    if sh.cm_opts.quote { "" } else { "no" }
                )
            } else {
                format!("current output mode: {}\n", sh.mode.descr())
            };
            sh.oputs(&line);
            sh.mode.descr().as_bytes().to_vec()
        }
    };
    let pre = |name: &str| !mode.is_empty() && name.as_bytes().starts_with(&mode);
    let mut rc = 0;
    if pre("lines") {
        sh.mode = Mode::Line;
        sh.row_sep = SEP_ROW.to_vec();
    } else if pre("columns") {
        sh.mode = Mode::Column;
        if !sh.has_flag(flag::HEADER_SET) {
            sh.show_header = true;
        }
        sh.row_sep = SEP_ROW.to_vec();
        sh.cm_opts = opts;
    } else if pre("list") {
        sh.mode = Mode::List;
        sh.col_sep = SEP_COLUMN.to_vec();
        sh.row_sep = SEP_ROW.to_vec();
    } else if pre("html") {
        sh.mode = Mode::Html;
    } else if pre("tcl") {
        sh.mode = Mode::Tcl;
        sh.col_sep = SEP_SPACE.to_vec();
        sh.row_sep = SEP_ROW.to_vec();
    } else if pre("csv") {
        sh.mode = Mode::Csv;
        sh.col_sep = SEP_COMMA.to_vec();
        sh.row_sep = SEP_CRLF.to_vec();
    } else if pre("tabs") {
        sh.mode = Mode::List;
        sh.col_sep = SEP_TAB.to_vec();
    } else if pre("insert") {
        sh.mode = Mode::Insert;
        let t = tabname.unwrap_or_else(|| b"table".to_vec());
        sh.dest_table = Some(text::quote_ident_if_needed(&t));
    } else if pre("quote") {
        sh.mode = Mode::Quote;
        sh.col_sep = SEP_COMMA.to_vec();
        sh.row_sep = SEP_ROW.to_vec();
    } else if pre("ascii") {
        sh.mode = Mode::Ascii;
        sh.col_sep = SEP_UNIT.to_vec();
        sh.row_sep = SEP_RECORD.to_vec();
    } else if pre("markdown") {
        sh.mode = Mode::Markdown;
        sh.cm_opts = opts;
    } else if pre("table") {
        sh.mode = Mode::Table;
        sh.cm_opts = opts;
    } else if pre("box") {
        sh.mode = Mode::Box;
        sh.cm_opts = opts;
    } else if pre("count") {
        sh.mode = Mode::Count;
    } else if pre("off") {
        sh.mode = Mode::Off;
    } else if pre("json") {
        sh.mode = Mode::Json;
    } else {
        sh.eputs("Error: mode should be one of: ascii box column csv html insert json line list markdown qbox quote table tabs tcl\n");
        rc = 1;
    }
    sh.c_mode = sh.mode;
    rc
}

fn dot_open(sh: &mut Shell, args: &[Vec<u8>]) -> Result<i32, Exit> {
    let mut file: Option<Vec<u8>> = None;
    let mut new_flag = false;
    let mut open_mode = OpenMode::Unspec;
    let mut nofollow = false;
    let mut i = 1;
    while i < args.len() {
        let z = &args[i];
        if option_match(z, "new") {
            new_flag = true;
        } else if option_match(z, "readonly") {
            open_mode = OpenMode::Readonly;
        } else if option_match(z, "nofollow") {
            nofollow = true;
        } else if option_match(z, "deserialize") {
            open_mode = OpenMode::Deserialize;
        } else if option_match(z, "maxsize") && i + 1 < args.len() {
            i += 1;
        } else if option_match(z, "zip") || option_match(z, "append") || option_match(z, "hexdb") {
            sh.eputs(&format!("unknown option: {}\n", lossy(z)));
            return Ok(1);
        } else if z.first() == Some(&b'-') {
            sh.eputs(&format!("unknown option: {}\n", lossy(z)));
            return Ok(1);
        } else if file.is_some() {
            sh.eputs(&format!("extra argument: \"{}\"\n", lossy(z)));
            return Ok(1);
        } else {
            file = Some(z.clone());
        }
        i += 1;
    }
    sh.close_db();
    sh.open_mode = open_mode;
    sh.nofollow = nofollow;
    if let Some(f) = &file {
        if new_flag && !sh.safe_mode {
            let _ = sys::current().unlinkat(Fd::CWD, f, sysabi::AtFlags::empty());
        }
        if sh.safe_mode && f.as_slice() != b":memory:" {
            fail_if_safe(sh, "cannot open disk-based database files in safe mode")?;
        }
        sh.db_filename = f.clone();
        sh.open_db(true)?;
    }
    if sh.db.is_none() {
        sh.db_filename = Vec::new();
        sh.open_db(false)?;
    }
    Ok(0)
}

fn dot_output(sh: &mut Shell, args: &[Vec<u8>]) -> Result<i32, Exit> {
    let cmd = &args[0];
    fail_if_safe(sh, &format!("cannot run .{} in safe mode", lossy(cmd)))?;
    let excel = cmd.first() == Some(&b'e');
    let once = !excel && b"once".starts_with(cmd.as_slice());
    let mut file: Option<Vec<u8>> = None;
    let mut bom = false;
    let mut emode = if excel { b'x' } else { 0 };
    let mut i = 1;
    while i < args.len() {
        let z = &args[i];
        if z.first() == Some(&b'-') {
            let zz = if z.get(1) == Some(&b'-') { &z[1..] } else { &z[..] };
            if zz == b"-bom" {
                bom = true;
            } else if !excel && zz == b"-x" {
                emode = b'x';
            } else if !excel && zz == b"-e" {
                emode = b'e';
            } else {
                sh.oputs(&format!("ERROR: unknown option: \"{}\".  Usage:\n", lossy(z)));
                show_help(sh, Some(cmd));
                return Ok(1);
            }
        } else if file.is_none() && emode != b'e' && emode != b'x' {
            let mut f = z.clone();
            if f.first() == Some(&b'|') {
                while i + 1 < args.len() {
                    i += 1;
                    f.push(b' ');
                    f.extend_from_slice(&args[i]);
                }
                file = Some(f);
                break;
            }
            file = Some(f);
        } else {
            sh.oputs(&format!("ERROR: extra parameter: \"{}\".  Usage:\n", lossy(z)));
            show_help(sh, Some(cmd));
            return Ok(1);
        }
        i += 1;
    }
    let file = file.unwrap_or_else(|| b"stdout".to_vec());
    sh.out_count = if once || excel { 2 } else { 0 };
    sh.output_reset();
    if emode != 0 {
        // Abrir planilha ou editor (xdg-open) não existe no sandbox.
        sh.eputs("Error: cannot open a spreadsheet or text editor in this environment\n");
        return Ok(1);
    }
    let bom_bytes: &[u8] = b"\xef\xbb\xbf";
    if file.first() == Some(&b'|') {
        match popen(&file[1..], true) {
            Some((fd, child)) => {
                let mut s = Stream::new(Sink::Pipe { fd, child });
                if bom {
                    s.put(bom_bytes);
                }
                sh.redirect = Some(s);
            }
            None => {
                sh.eputs(&format!("Error: cannot open pipe \"{}\"\n", lossy(&file[1..])));
                return Ok(1);
            }
        }
        return Ok(0);
    }
    let stream = match file.as_slice() {
        b"stdout" => None,
        b"stderr" => Some(Stream::new(Sink::Inherited(Fd::STDERR))),
        b"off" => return Ok(1),
        f => match sys::open(f, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC, 0o666) {
            Ok(fd) => Some(Stream::new(Sink::File(fd))),
            Err(_) => {
                sh.eputs(&format!("Error: cannot open \"{}\"\n", lossy(f)));
                sh.eputs(&format!("Error: cannot write to \"{}\"\n", lossy(f)));
                return Ok(1);
            }
        },
    };
    sh.redirect = stream;
    if bom {
        sh.oput(bom_bytes);
    }
    Ok(0)
}

fn dot_parameter(sh: &mut Shell, args: &[Vec<u8>]) -> Result<i32, Exit> {
    sh.open_db(false)?;
    let conn = sh.conn();
    let n = args.len();
    let sub = args.get(1).map(Vec::as_slice);
    let mut rc = 0;
    match (n, sub) {
        (2, Some(b"clear")) => {
            let _ = exec_quiet(conn, "DROP TABLE IF EXISTS temp.sqlite_parameters;");
        }
        (2, Some(b"list")) => {
            let len = query_text(conn, "SELECT max(length(key)) FROM temp.sqlite_parameters;")
                .ok()
                .and_then(|r| r.into_iter().next())
                .and_then(|r| r.into_iter().next().flatten())
                .and_then(|t| String::from_utf8_lossy(&t).parse::<usize>().ok())
                .unwrap_or(0)
                .min(40);
            if len > 0 {
                let rows = query_text(conn, "SELECT key, quote(value) FROM temp.sqlite_parameters;").unwrap_or_default();
                let mut o = Vec::new();
                for r in rows {
                    let k = r.first().cloned().flatten().unwrap_or_default();
                    let v = r.get(1).cloned().flatten().unwrap_or_default();
                    o.extend_from_slice(&k);
                    o.extend(std::iter::repeat_n(b' ', len.saturating_sub(k.len())));
                    o.push(b' ');
                    o.extend_from_slice(&v);
                    o.push(b'\n');
                }
                sh.oput(&o);
            }
        }
        (2, Some(b"init")) => funcs::bind_table_init(conn),
        (4, Some(b"set")) => {
            funcs::bind_table_init(conn);
            let key = text::squote(&args[2]);
            let try1 = format!(
                "REPLACE INTO temp.sqlite_parameters(key,value)VALUES({},{});",
                lossy(&key),
                lossy(&args[3])
            );
            let ok = conn.prepare(&try1).map(|mut s| {
                let _ = s.raw_execute();
            });
            unwind::reraise();
            if ok.is_err() {
                let try2 = format!(
                    "REPLACE INTO temp.sqlite_parameters(key,value)VALUES({},{});",
                    lossy(&key),
                    lossy(&text::squote(&args[3]))
                );
                let r = conn.prepare(&try2).map(|mut s| {
                    let _ = s.raw_execute();
                });
                unwind::reraise();
                if let Err(e) = r {
                    let msg = exec::error_parts(&e).1;
                    sh.oputs(&format!("Error: {msg}\n"));
                    rc = 1;
                }
            }
        }
        (3, Some(b"unset")) => {
            let sql = format!("DELETE FROM temp.sqlite_parameters WHERE key={}", lossy(&text::squote(&args[2])));
            let _ = exec_quiet(conn, &sql);
        }
        _ => {
            show_help(sh, Some(b"parameter"));
        }
    }
    Ok(rc)
}

fn dot_progress(sh: &mut Shell, args: &[Vec<u8>]) -> Result<i32, Exit> {
    let mut nn = 0i64;
    let mut flags = 0u32;
    let mut max = 0i64;
    let mut i = 1;
    while i < args.len() {
        let z = &args[i];
        if z.first() == Some(&b'-') {
            let mut zz = &z[1..];
            if zz.first() == Some(&b'-') {
                zz = &zz[1..];
            }
            match zz {
                b"quiet" | b"q" => flags |= funcs::PROGRESS_QUIET,
                b"reset" => flags |= funcs::PROGRESS_RESET,
                b"once" => flags |= funcs::PROGRESS_ONCE,
                b"limit" => {
                    if i + 1 >= args.len() {
                        sh.eputs("Error: missing argument on --limit\n");
                        return Ok(1);
                    }
                    i += 1;
                    max = integer_value(&args[i]);
                }
                _ => {
                    sh.eputs(&format!("Error: unknown option: \"{}\"\n", lossy(z)));
                    return Ok(1);
                }
            }
        } else {
            nn = integer_value(z);
        }
        i += 1;
    }
    sh.open_db(false)?;
    funcs::set_progress(nn as i32, max as u32, flags);
    Ok(0)
}

fn dot_read(sh: &mut Shell, args: &[Vec<u8>]) -> Result<i32, Exit> {
    fail_if_safe(sh, "cannot run .read in safe mode")?;
    if args.len() != 2 {
        sh.eputs("Usage: .read FILE\n");
        return Ok(1);
    }
    let saved = sh.lineno;
    let arg = &args[1];
    let rc;
    if arg.first() == Some(&b'|') {
        match popen(&arg[1..], false) {
            Some((fd, pid)) => {
                let mut r = LineReader::new(fd, true);
                rc = i32::from(sh.process_input(&mut r, false)?);
                r.close();
                wait_status(pid);
            }
            None => {
                sh.eputs(&format!("Error: cannot open \"{}\"\n", lossy(arg)));
                rc = 1;
            }
        }
    } else {
        let ok = match sys::stat(arg) {
            Ok(st) => matches!(st.file_type(), sysabi::FileType::Regular | sysabi::FileType::Fifo | sysabi::FileType::CharDevice),
            Err(_) => false,
        };
        let fd = if ok { sys::open(arg, OFlags::RDONLY | OFlags::CLOEXEC, 0).ok() } else { None };
        match fd {
            Some(fd) => {
                let mut r = LineReader::new(fd, true);
                rc = i32::from(sh.process_input(&mut r, false)?);
                r.close();
            }
            None => {
                sh.eputs(&format!("Error: cannot open \"{}\"\n", lossy(arg)));
                rc = 1;
            }
        }
    }
    sh.lineno = saved;
    Ok(rc)
}

fn dot_show(sh: &mut Shell) {
    let bools = ["off", "on", "trigger", "full"];
    let mut o = Vec::new();
    let mut line = |k: &str, v: &[u8]| {
        o.extend_from_slice(format!("{:>12.12}: ", k).as_bytes());
        o.extend_from_slice(v);
        o.push(b'\n');
    };
    line("echo", bools[usize::from(sh.has_flag(flag::ECHO))].as_bytes());
    line("eqp", bools[usize::from(sh.auto_eqp & 3)].as_bytes());
    line(
        "explain",
        if sh.mode == Mode::Explain {
            b"on"
        } else if sh.auto_explain {
            b"auto"
        } else {
            b"off"
        },
    );
    line("headers", bools[usize::from(sh.show_header)].as_bytes());
    if sh.mode.is_columnar() {
        let v = format!(
            "{} --wrap {} --wordwrap {} --{}quote",
            sh.mode.descr(),
            sh.cm_opts.wrap,
            if sh.cm_opts.word_wrap { "on" } else { "off" },
            if sh.cm_opts.quote { "" } else { "no" }
        );
        line("mode", v.as_bytes());
    } else {
        line("mode", sh.mode.descr().as_bytes());
    }
    line("nullvalue", &text::c_string(&sh.null_value));
    let outname: &[u8] = match sh.redirect.as_ref().map(|r| &r.sink) {
        None => b"stdout",
        Some(Sink::Inherited(fd)) if *fd == Fd::STDERR => b"stderr",
        Some(_) => b"(file)",
    };
    line("output", outname);
    line("colseparator", &text::c_string(&sh.col_sep));
    line("rowseparator", &text::c_string(&sh.row_sep));
    let stats = match sh.stats_on {
        0 => "off",
        2 => "stmt",
        3 => "vmstep",
        _ => "on",
    };
    line("stats", stats.as_bytes());
    let widths: String = sh.col_width.iter().map(|w| format!("{w} ")).collect();
    line("width", widths.as_bytes());
    line("filename", &sh.db_filename.clone());
    sh.oput(&o);
}

fn dot_tables(sh: &mut Shell, args: &[Vec<u8>], indexes: bool) -> Result<i32, Exit> {
    sh.open_db(false)?;
    let dbs = match query_text(sh.conn(), "PRAGMA database_list") {
        Ok(r) => r,
        Err(e) => {
            sh.eputs(&format!("Error: {}\n", exec::error_parts(&e).1));
            return Ok(1);
        }
    };
    if args.len() > 2 && indexes {
        sh.eputs("Usage: .indexes ?LIKE-PATTERN?\n");
        return Ok(1);
    }
    let mut sql = String::new();
    for r in dbs {
        let Some(Some(name)) = r.get(1) else { continue };
        let name = lossy(name);
        if !sql.is_empty() {
            sql.push_str(" UNION ALL ");
        }
        if name.eq_ignore_ascii_case("main") {
            sql.push_str("SELECT name FROM ");
        } else {
            sql.push_str(&format!("SELECT {}||'.'||name FROM ", lossy(&text::squote(name.as_bytes()))));
        }
        sql.push_str(&lossy(&text::dquote(name.as_bytes())));
        sql.push_str(".sqlite_schema ");
        if indexes {
            sql.push_str(" WHERE type='index'   AND tbl_name LIKE ?1");
        } else {
            sql.push_str(" WHERE type IN ('table','view')   AND name NOT LIKE 'sqlite_%'   AND name LIKE ?1");
        }
    }
    sql.push_str(" ORDER BY 1");
    let pattern: Vec<u8> = if args.len() > 1 { args[1].clone() } else { b"%".to_vec() };
    let conn = sh.conn();
    let res = (|| {
        let mut st = conn.prepare(&sql)?;
        st.raw_bind_parameter(1, Value::Text(lossy(&pattern)))?;
        let mut rows = st.raw_query();
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let v = row.get_ref(0)?;
            let cell = match v {
                rusqlite::types::ValueRef::Null => Cell::Null,
                rusqlite::types::ValueRef::Integer(i) => Cell::Int(i),
                rusqlite::types::ValueRef::Real(f) => Cell::Real(f),
                rusqlite::types::ValueRef::Text(t) => Cell::Text(t.to_vec()),
                rusqlite::types::ValueRef::Blob(b) => Cell::Blob(b.to_vec()),
            };
            out.push(cell.text().unwrap_or_default());
        }
        Ok::<_, rusqlite::Error>(out)
    })();
    unwind::reraise();
    let names = match res {
        Ok(n) => n,
        Err(e) => {
            sh.eputs(&format!("Error: {}\n", exec::error_parts(&e).1));
            return Ok(1);
        }
    };
    if names.is_empty() {
        return Ok(0);
    }
    let maxlen = names.iter().map(|n| cstr(n).len()).max().unwrap_or(0);
    let ncol = (80 / (maxlen + 2)).max(1);
    let nrow = names.len().div_ceil(ncol);
    let mut o = Vec::new();
    for i in 0..nrow {
        let mut j = i;
        while j < names.len() {
            if j >= nrow {
                o.extend_from_slice(b"  ");
            }
            let z = cstr(&names[j]);
            o.extend_from_slice(z);
            o.extend(std::iter::repeat_n(b' ', maxlen - z.len()));
            j += nrow;
        }
        o.push(b'\n');
    }
    sh.oput(&o);
    Ok(0)
}

/// `.schema`.
fn dot_schema(sh: &mut Shell, args: &[Vec<u8>]) -> Result<i32, Exit> {
    sh.open_db(false)?;
    let mut pretty = false;
    let mut debug = false;
    let mut nosys = false;
    let mut name: Option<Vec<u8>> = None;
    for a in &args[1..] {
        if option_match(a, "indent") {
            pretty = true;
        } else if option_match(a, "debug") {
            debug = true;
        } else if option_match(a, "nosys") {
            nosys = true;
        } else if a.first() == Some(&b'-') {
            sh.eputs(&format!("Unknown option: \"{}\"\n", lossy(a)));
            return Ok(1);
        } else if name.is_none() {
            name = Some(a.clone());
        } else {
            sh.eputs("Usage: .schema ?--indent? ?--nosys? ?LIKE-PATTERN?\n");
            return Ok(1);
        }
    }
    let mode = if pretty { Mode::Pretty } else { Mode::Semi };
    let mut out = Vec::new();
    if let Some(n) = &name {
        let lower = String::from_utf8_lossy(n).to_ascii_lowercase();
        if ["sqlite_master", "sqlite_schema", "sqlite_temp_master", "sqlite_temp_schema"].contains(&lower.as_str()) {
            let t = format!(
                "CREATE TABLE {} (\n  type text,\n  name text,\n  tbl_name text,\n  rootpage integer,\n  sql text\n)",
                lossy(n)
            );
            out.extend(schema_row(mode, t.as_bytes()));
        }
    }
    let dbs = match query_text(sh.conn(), "SELECT name FROM pragma_database_list") {
        Ok(r) => r,
        Err(e) => {
            sh.eputs(&format!("Error: {}\n", exec::error_parts(&e).1));
            return Ok(1);
        }
    };
    // Duas versões da mesma consulta: a do shell.c (pro `--debug`) e a que roda, que traz o esquema e
    // o nome à parte pra o `shell_add_schema` (que precisa da conexão pra montar o comentário das
    // views) ser aplicado aqui.
    let mut shown = String::from("SELECT sql FROM");
    let mut sel = String::from("SELECT sql, sch, name, ismod FROM");
    let mut div = "(";
    for (i, r) in dbs.iter().enumerate() {
        let Some(Some(db)) = r.first() else { continue };
        let db = lossy(db);
        let sch = if !db.eq_ignore_ascii_case("main") { lossy(&text::squote(db.as_bytes())) } else { "NULL".to_string() };
        let tail = format!(
            ",name) AS sql, type, tbl_name, name, rowid,{} AS snum, {} AS sname FROM {}.sqlite_schema",
            i + 1,
            lossy(&text::squote(db.as_bytes())),
            lossy(&text::quote_ident_if_needed(db.as_bytes()))
        );
        shown.push_str(div);
        shown.push_str(&format!("SELECT shell_add_schema(sql,{sch}{tail}"));
        sel.push_str(div);
        sel.push_str(&format!(
            "SELECT sql AS sql, {sch} AS sch, 0 AS ismod, type, tbl_name, name, rowid,{} AS snum, {} AS sname FROM {}.sqlite_schema",
            i + 1,
            lossy(&text::squote(db.as_bytes())),
            lossy(&text::quote_ident_if_needed(db.as_bytes()))
        ));
        div = " UNION ALL ";
    }
    if name.is_some() {
        shown.push_str(" UNION ALL SELECT shell_module_schema(name), 'table', name, name, name, 9e+99, 'main' FROM pragma_module_list");
        sel.push_str(" UNION ALL SELECT '', NULL, 1, 'table', name, name, name, 9e+99, 'main' FROM pragma_module_list");
    }
    let mut cond = String::from(") WHERE ");
    if let Some(n) = &name {
        let glob = n.iter().any(|&c| c == b'*' || c == b'?' || c == b'[');
        if n.contains(&b'.') {
            cond.push_str("lower(printf('%s.%s',sname,tbl_name))");
        } else {
            cond.push_str("lower(tbl_name)");
        }
        cond.push_str(if glob { " GLOB " } else { " LIKE " });
        cond.push_str(&lossy(&text::squote(n)));
        if !glob {
            cond.push_str(" ESCAPE '\\' ");
        }
        cond.push_str(" AND ");
    }
    if nosys {
        cond.push_str("name NOT LIKE 'sqlite_%%' AND ");
    }
    cond.push_str("sql IS NOT NULL ORDER BY snum, rowid");
    shown.push_str(&cond);
    sel.push_str(&cond);
    if debug {
        sh.oput(&out);
        sh.oputs(&format!("SQL: {shown};\n"));
        return Ok(0);
    }
    let rows = query_text(sh.conn(), &sel);
    match rows {
        Ok(rows) => {
            for r in rows {
                let sql = r.first().cloned().flatten();
                let sch = r.get(1).cloned().flatten();
                let obj = r.get(2).cloned().flatten();
                let ismod = r.get(3).cloned().flatten().is_some_and(|v| v == b"1");
                let text = if ismod {
                    obj.and_then(|o| fake_schema(sh.conn(), None, &o)).map(|f| {
                        let mut t = b"/* ".to_vec();
                        t.extend(f);
                        t.extend_from_slice(b" */");
                        t
                    })
                } else {
                    sql.map(|s| add_schema(sh.conn(), s, sch.as_deref(), obj.as_deref()))
                };
                if let Some(t) = text {
                    out.extend(schema_row(mode, &t));
                }
            }
            sh.oput(&out);
            Ok(0)
        }
        Err(e) => {
            sh.oput(&out);
            sh.eputs(&format!("Error: {}\n", exec::error_parts(&e).1));
            Ok(1)
        }
    }
}

/// `shellFakeSchema`: `esquema.nome(col1,col2,...)` de uma view ou tabela virtual.
fn fake_schema(conn: &Connection, schema: Option<&[u8]>, name: &[u8]) -> Option<Vec<u8>> {
    let sql = format!(
        "PRAGMA \"{}\".table_info={};",
        lossy(&text::escape_dq(schema.unwrap_or(b"main"))),
        lossy(&text::squote(name))
    );
    let rows = query_text(conn, &sql).unwrap_or_default();
    if rows.is_empty() {
        return None;
    }
    let mut s = Vec::new();
    if let Some(sc) = schema {
        if text::needs_quote(sc) && !sc.eq_ignore_ascii_case(b"temp") {
            s.extend(text::dquote(sc));
        } else {
            s.extend_from_slice(sc);
        }
        s.push(b'.');
    }
    s.extend(text::quote_ident_if_needed(name));
    for (i, r) in rows.iter().enumerate() {
        s.push(if i == 0 { b'(' } else { b',' });
        let col = r.get(1).cloned().flatten().unwrap_or_default();
        s.extend(text::quote_ident_if_needed(&col));
    }
    s.push(b')');
    Some(s)
}

/// `shell_add_schema(sql, esquema, nome)`, com o comentário das views.
fn add_schema(conn: &Connection, z: Vec<u8>, schema: Option<&[u8]>, name: Option<&[u8]>) -> Vec<u8> {
    const PREFIX: [&str; 6] = ["TABLE", "INDEX", "UNIQUE INDEX", "VIEW", "TRIGGER", "VIRTUAL TABLE"];
    if !z.starts_with(b"CREATE ") {
        return z;
    }
    for p in PREFIX {
        let n = p.len();
        if !(z.len() > n + 7 && &z[7..7 + n] == p.as_bytes() && z[n + 7] == b' ') {
            continue;
        }
        let mut out: Option<Vec<u8>> = None;
        if let Some(s) = schema {
            let mut o = z[..n + 7].to_vec();
            o.push(b' ');
            if text::needs_quote(s) && !s.eq_ignore_ascii_case(b"temp") {
                o.extend(text::dquote(s));
            } else {
                o.extend_from_slice(s);
            }
            o.push(b'.');
            o.extend_from_slice(&z[n + 8..]);
            out = Some(o);
        }
        if let Some(nm) = name
            && p.starts_with('V')
            && let Some(fake) = fake_schema(conn, schema, nm)
        {
            let mut o = out.unwrap_or_else(|| z.clone());
            o.extend_from_slice(b"\n/* ");
            o.extend(fake);
            o.extend_from_slice(b" */");
            out = Some(o);
        }
        if let Some(o) = out {
            return o;
        }
    }
    z
}

/// Uma linha de `.schema`/`.fullschema` em MODE_Semi ou MODE_Pretty.
fn schema_row(mode: Mode, sql: &[u8]) -> Vec<u8> {
    match mode {
        Mode::Pretty => exec::pretty_schema(sql),
        _ => exec::schema_line(sql, b";\n"),
    }
}

fn dot_fullschema(sh: &mut Shell, args: &[Vec<u8>]) -> Result<i32, Exit> {
    let mut mode = Mode::Semi;
    let mut nargs = args.len();
    if nargs == 2 && option_match(&args[1], "indent") {
        mode = Mode::Pretty;
        nargs = 1;
    }
    if nargs != 1 {
        sh.eputs("Usage: .fullschema ?--indent?\n");
        return Ok(1);
    }
    sh.open_db(false)?;
    let rows = query_text(
        sh.conn(),
        "SELECT sql FROM  (SELECT sql sql, type type, tbl_name tbl_name, name name, rowid x     FROM sqlite_schema UNION ALL   SELECT sql, type, tbl_name, name, rowid FROM sqlite_temp_schema) WHERE type!='meta' AND sql NOTNULL AND name NOT LIKE 'sqlite_%' ORDER BY x",
    );
    let mut out = Vec::new();
    let mut do_stats = false;
    if let Ok(rows) = rows {
        for r in rows {
            if let Some(Some(sql)) = r.first() {
                out.extend(schema_row(mode, sql));
            }
        }
        do_stats = query_text(sh.conn(), "SELECT rowid FROM sqlite_schema WHERE name GLOB 'sqlite_stat[134]'")
            .map(|r| !r.is_empty())
            .unwrap_or(false);
    }
    sh.oput(&out);
    if !do_stats {
        sh.oputs("/* No STAT tables available */\n");
    } else {
        sh.oputs("ANALYZE sqlite_schema;\n");
        let saved = (sh.mode, sh.c_mode, sh.dest_table.clone(), sh.show_header);
        sh.mode = Mode::Insert;
        sh.c_mode = Mode::Insert;
        sh.show_header = false;
        sh.dest_table = Some(b"sqlite_stat1".to_vec());
        let _ = exec::shell_exec(sh, b"SELECT * FROM sqlite_stat1")?;
        sh.dest_table = Some(b"sqlite_stat4".to_vec());
        let _ = exec::shell_exec(sh, b"SELECT * FROM sqlite_stat4")?;
        (sh.mode, sh.c_mode, sh.dest_table, sh.show_header) = saved;
        sh.oputs("ANALYZE sqlite_schema;\n");
    }
    Ok(0)
}

/// `.dump`.
fn dot_dump(sh: &mut Shell, args: &[Vec<u8>]) -> Result<i32, Exit> {
    let saved_header = sh.show_header;
    let saved_flags = sh.flags;
    sh.flags &= !(flag::PRESERVE_ROWID | flag::NEWLINES | flag::ECHO | flag::DUMP_DATA_ONLY | flag::DUMP_NO_SYS);
    let mut like: Option<String> = None;
    for a in &args[1..] {
        if a.first() == Some(&b'-') {
            let z = if a.get(1) == Some(&b'-') { &a[2..] } else { &a[1..] };
            match z {
                b"preserve-rowids" => sh.flags |= flag::PRESERVE_ROWID,
                b"newlines" => sh.flags |= flag::NEWLINES,
                b"data-only" => sh.flags |= flag::DUMP_DATA_ONLY,
                b"nosys" => sh.flags |= flag::DUMP_NO_SYS,
                _ => {
                    sh.eputs(&format!("Unknown option \"{}\" on \".dump\"\n", lossy(a)));
                    sh.flags = saved_flags;
                    return Ok(1);
                }
            }
        } else {
            let q = lossy(&text::squote(a));
            let expr = format!(
                "name LIKE {q} ESCAPE '\\' OR EXISTS (  SELECT 1 FROM sqlite_schema WHERE     name LIKE {q} ESCAPE '\\' AND    sql LIKE 'CREATE VIRTUAL TABLE%' AND    substr(o.name, 1, length(name)+1) == (name||'_'))"
            );
            like = Some(match like {
                Some(l) => format!("{l} OR {expr}"),
                None => expr,
            });
        }
    }
    sh.open_db(false)?;
    let like_s = like.clone().unwrap_or_else(|| "true".into());
    if query_text(sh.conn(), &format!("SELECT 1 FROM sqlite_schema o WHERE sql LIKE 'CREATE VIRTUAL TABLE%' AND {like_s}"))
        .map(|r| !r.is_empty())
        .unwrap_or(false)
    {
        sh.oputs("/* WARNING: Script requires that SQLITE_DBCONFIG_DEFENSIVE be disabled */\n");
    }
    let data_only = sh.has_flag(flag::DUMP_DATA_ONLY);
    if !data_only {
        sh.oputs("PRAGMA foreign_keys=OFF;\n");
        sh.oputs("BEGIN TRANSACTION;\n");
    }
    let mut writable_schema = false;
    sh.show_header = false;
    let _ = exec_quiet(sh.conn(), "SAVEPOINT dump; PRAGMA writable_schema=ON");
    sh.n_err = 0;
    let q = format!(
        "SELECT name, type, sql FROM sqlite_schema AS o WHERE ({like_s}) AND type=='table'  AND sql NOT NULL ORDER BY tbl_name='sqlite_sequence', rowid"
    );
    let tables = query_text(sh.conn(), &q).unwrap_or_default();
    for r in tables {
        let (Some(Some(table)), Some(Some(kind)), Some(Some(sql))) = (r.first(), r.get(1), r.get(2)) else { continue };
        dump_table(sh, table, kind, sql, &mut writable_schema)?;
    }
    if !data_only {
        let q = format!(
            "SELECT sql FROM sqlite_schema AS o WHERE ({like_s}) AND sql NOT NULL  AND type IN ('index','trigger','view') ORDER BY type COLLATE NOCASE DESC"
        );
        let rows = query_text(sh.conn(), &q).unwrap_or_default();
        let mut o = Vec::new();
        for r in rows {
            let z = r.first().cloned().flatten().unwrap_or_default();
            o.extend_from_slice(cstr(&z));
            if z.windows(2).any(|w| w == b"--") {
                o.extend_from_slice(b"\n;\n");
            } else {
                o.extend_from_slice(b";\n");
            }
        }
        sh.oput(&o);
    }
    if writable_schema {
        sh.oputs("PRAGMA writable_schema=OFF;\n");
    }
    let _ = exec_quiet(sh.conn(), "PRAGMA writable_schema=OFF;");
    let _ = exec_quiet(sh.conn(), "RELEASE dump;");
    if !data_only {
        let end = if sh.n_err > 0 { "ROLLBACK; -- due to errors\n" } else { "COMMIT;\n" };
        sh.oputs(end);
    }
    sh.show_header = saved_header;
    sh.flags = saved_flags;
    Ok(0)
}

/// `dump_callback` de uma tabela.
fn dump_table(sh: &mut Shell, table: &[u8], kind: &[u8], sql: &[u8], writable: &mut bool) -> Result<(), Exit> {
    let data_only = sh.has_flag(flag::DUMP_DATA_ONLY);
    let no_sys = sh.has_flag(flag::DUMP_NO_SYS);
    let is_stat = table.len() == 12 && table.starts_with(b"sqlite_stat");
    if table == b"sqlite_sequence" && !no_sys {
        if !data_only {
            sh.oputs("DELETE FROM sqlite_sequence;\n");
        }
    } else if is_stat && !no_sys {
        if !data_only {
            sh.oputs("ANALYZE sqlite_schema;\n");
        }
    } else if table.starts_with(b"sqlite_") {
        return Ok(());
    } else if data_only {
    } else if sql.starts_with(b"CREATE VIRTUAL TABLE") {
        if !*writable {
            sh.oputs("PRAGMA writable_schema=ON;\n");
            *writable = true;
        }
        let mut o = b"INSERT INTO sqlite_schema(type,name,tbl_name,rootpage,sql)VALUES('table','".to_vec();
        o.extend(text::escape_sq(table));
        o.extend_from_slice(b"','");
        o.extend(text::escape_sq(table));
        o.extend_from_slice(b"',0,'");
        o.extend(text::escape_sq(sql));
        o.extend_from_slice(b"');\n");
        sh.oput(&o);
        return Ok(());
    } else {
        let line = exec::schema_line(sql, b";\n");
        sh.oput(&line);
    }
    if kind != b"table" {
        return Ok(());
    }
    let Some(cols) = table_column_list(sh, table) else {
        sh.n_err += 1;
        return Ok(());
    };
    let mut stable = text::quote_ident_if_needed(table);
    if let Some(rowid) = &cols.0 {
        stable.push(b'(');
        stable.extend_from_slice(rowid);
        for c in &cols.1 {
            stable.push(b',');
            stable.extend(text::quote_ident_if_needed(c));
        }
        stable.push(b')');
    }
    let mut select = b"SELECT ".to_vec();
    if let Some(rowid) = &cols.0 {
        select.extend_from_slice(rowid);
        select.push(b',');
    }
    for (i, c) in cols.1.iter().enumerate() {
        select.extend(text::quote_ident_if_needed(c));
        if i + 1 < cols.1.len() {
            select.push(b',');
        }
    }
    select.extend_from_slice(b" FROM ");
    select.extend(text::quote_ident_if_needed(table));
    let saved = (sh.dest_table.clone(), sh.mode, sh.c_mode);
    sh.dest_table = Some(stable);
    sh.mode = Mode::Insert;
    sh.c_mode = Mode::Insert;
    let r = exec::shell_exec(sh, &select)?;
    if let Some(e) = &r
        && e.rc == 11
    {
        sh.oputs("/****** CORRUPTION ERROR *******/\n");
    }
    (sh.dest_table, sh.mode, sh.c_mode) = saved;
    if r.is_some() {
        sh.n_err += 1;
    }
    Ok(())
}

/// `tableColumnList`: (coluna do rowid a preservar, colunas).
fn table_column_list(sh: &mut Shell, table: &[u8]) -> Option<(Option<Vec<u8>>, Vec<Vec<u8>>)> {
    let conn = sh.conn();
    let rows = query_text(conn, &format!("PRAGMA table_info={}", lossy(&text::squote(table)))).ok()?;
    let mut cols = Vec::new();
    let mut n_pk = 0;
    let mut is_ipk = false;
    for r in &rows {
        cols.push(r.get(1).cloned().flatten().unwrap_or_default());
        let pk = r.get(5).cloned().flatten().map(|v| v != b"0").unwrap_or(false);
        if pk {
            n_pk += 1;
            let ty = r.get(2).cloned().flatten().unwrap_or_default();
            is_ipk = n_pk == 1 && ty.eq_ignore_ascii_case(b"INTEGER");
        }
    }
    if rows.is_empty() {
        return None;
    }
    let mut preserve = sh.has_flag(flag::PRESERVE_ROWID);
    if preserve && is_ipk {
        preserve = query_text(conn, &format!("SELECT 1 FROM pragma_index_list({}) WHERE origin='pk'", lossy(&text::squote(table))))
            .map(|r| !r.is_empty())
            .unwrap_or(false);
    }
    let mut rowid = None;
    if preserve {
        for cand in ["rowid", "_rowid_", "oid"] {
            if !cols.iter().any(|c| c.eq_ignore_ascii_case(cand.as_bytes())) {
                let ok = conn
                    .prepare(&format!("SELECT {cand} FROM {} LIMIT 0", lossy(&text::dquote(table))))
                    .is_ok();
                unwind::reraise();
                if ok {
                    rowid = Some(cand.as_bytes().to_vec());
                }
                break;
            }
        }
    }
    Some((rowid, cols))
}

/// `.import`.
fn dot_import(sh: &mut Shell, args: &[Vec<u8>]) -> Result<i32, Exit> {
    fail_if_safe(sh, "cannot run .import in safe mode")?;
    let mut file: Option<Vec<u8>> = None;
    let mut table: Option<Vec<u8>> = None;
    let mut schema: Option<Vec<u8>> = None;
    let mut verbose = 0;
    let mut skip = 0i64;
    let mut use_output_mode = true;
    let mut ascii = sh.mode == Mode::Ascii;
    let (mut col_sep, mut row_sep) = (0u8, 0u8);
    let mut i = 1;
    while i < args.len() {
        let z0 = &args[i];
        let z: &[u8] = if z0.starts_with(b"--") { &z0[1..] } else { z0 };
        if z.first() != Some(&b'-') {
            if file.is_none() {
                file = Some(z.to_vec());
            } else if table.is_none() {
                table = Some(z.to_vec());
            } else {
                sh.oputs(&format!("ERROR: extra argument: \"{}\".  Usage:\n", lossy(z)));
                show_help(sh, Some(b"import"));
                return Ok(1);
            }
        } else if z == b"-v" {
            verbose += 1;
        } else if z == b"-schema" && i < args.len() - 1 {
            i += 1;
            schema = Some(args[i].clone());
        } else if z == b"-skip" && i < args.len() - 1 {
            i += 1;
            skip = integer_value(&args[i]);
        } else if z == b"-ascii" {
            col_sep = 0x1f;
            row_sep = 0x1e;
            ascii = true;
            use_output_mode = false;
        } else if z == b"-csv" {
            col_sep = b',';
            row_sep = b'\n';
            ascii = false;
            use_output_mode = false;
        } else {
            sh.oputs(&format!("ERROR: unknown option: \"{}\".  Usage:\n", lossy(z)));
            show_help(sh, Some(b"import"));
            return Ok(1);
        }
        i += 1;
    }
    let Some(table) = table else {
        sh.oputs(&format!("ERROR: missing {} argument. Usage:\n", if file.is_none() { "FILE" } else { "TABLE" }));
        show_help(sh, Some(b"import"));
        return Ok(1);
    };
    let file = file.unwrap_or_default();
    sh.seen_interrupt = 0;
    sh.open_db(false)?;
    if use_output_mode {
        if sh.col_sep.is_empty() {
            sh.eputs("Error: non-null column separator required for import\n");
            return Ok(1);
        }
        if sh.col_sep.len() > 1 {
            sh.eputs("Error: multi-character column separators not allowed for import\n");
            return Ok(1);
        }
        if sh.row_sep.is_empty() {
            sh.eputs("Error: non-null row separator required for import\n");
            return Ok(1);
        }
        if sh.row_sep.len() == 2 && sh.mode == Mode::Csv && sh.row_sep == SEP_CRLF {
            sh.row_sep = SEP_ROW.to_vec();
        }
        if sh.row_sep.len() > 1 {
            sh.eputs("Error: multi-character row separators not allowed for import\n");
            return Ok(1);
        }
        col_sep = sh.col_sep[0];
        row_sep = sh.row_sep[0];
    }
    let (data, shown_name) = if file.first() == Some(&b'|') {
        match popen(&file[1..], false) {
            Some((fd, pid)) => {
                let d = sys::read_to_end(fd).unwrap_or_default();
                let _ = sys::close(fd);
                wait_status(pid);
                (d, b"<pipe>".to_vec())
            }
            None => {
                sh.eputs(&format!("Error: cannot open \"{}\"\n", lossy(&file)));
                return Ok(1);
            }
        }
    } else {
        match sys::read_file(&file) {
            Ok(d) => (d, file.clone()),
            Err(_) => {
                sh.eputs(&format!("Error: cannot open \"{}\"\n", lossy(&file)));
                return Ok(1);
            }
        }
    };
    if verbose >= 2 || (verbose >= 1 && use_output_mode) {
        let mut o = b"Column separator ".to_vec();
        o.extend(text::c_string(&[col_sep]));
        o.extend_from_slice(b", row separator ");
        o.extend(text::c_string(&[row_sep]));
        o.push(b'\n');
        sh.oput(&o);
    }
    funcs::import(sh, funcs::ImportArgs {
        data: &data,
        file_name: &shown_name,
        table: &table,
        schema: schema.as_deref(),
        col_sep,
        row_sep,
        ascii,
        skip,
        verbose,
    })
}
