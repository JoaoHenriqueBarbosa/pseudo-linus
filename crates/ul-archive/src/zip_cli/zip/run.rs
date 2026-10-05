//! A leitura da linha de comando do zip (a primeira metade do `main` do zip.c): opções, argumentos,
//! senha, arquivo de log e as verificações de combinações de opções.

use sysabi::sys;
use sysabi::{Fd, OFlags};

use super::consts::*;
use super::helpers::read_num_string;
use super::opts::*;
use super::state::{R, Zip};
use super::text;
use super::times::dostime;
use crate::sysutil;

/// `abbrevmatch(match, abbrev, 0, 1)`: `abbrev` é abreviação de `matchs`, sem distinguir caixa.
fn abbrevmatch(matchs: &str, abbrev: &[u8]) -> bool {
    let m = matchs.as_bytes();
    let mut cnt = 0usize;
    let n = m.len().min(abbrev.len());
    for i in 0..n {
        cnt += 1;
        if !m[i].eq_ignore_ascii_case(&abbrev[i]) {
            return false;
        }
    }
    if cnt < 1 {
        return false;
    }
    abbrev.len() <= m.len()
}

/// Lê um inteiro com largura máxima como o `%Nd` do `sscanf`.
fn scan_int(s: &[u8], pos: &mut usize, maxw: usize) -> Option<i64> {
    while *pos < s.len() && (s[*pos] == b' ' || (9..=13).contains(&s[*pos])) {
        *pos += 1;
    }
    let mut n = 0usize;
    let mut neg = false;
    if *pos < s.len() && (s[*pos] == b'+' || s[*pos] == b'-') && n < maxw {
        neg = s[*pos] == b'-';
        *pos += 1;
        n += 1;
    }
    let mut v: i64 = 0;
    let mut digits = 0;
    while *pos < s.len() && s[*pos].is_ascii_digit() && n < maxw {
        v = v * 10 + (s[*pos] - b'0') as i64;
        *pos += 1;
        n += 1;
        digits += 1;
    }
    if digits == 0 {
        return None;
    }
    Some(if neg { -v } else { v })
}

fn scan_date(s: &[u8], widths: [usize; 3], sep: Option<u8>) -> Option<[i64; 3]> {
    let mut pos = 0usize;
    let mut out = [0i64; 3];
    for i in 0..3 {
        out[i] = scan_int(s, &mut pos, widths[i])?;
        if i < 2
            && let Some(c) = sep {
                if s.get(pos) == Some(&c) {
                    pos += 1;
                } else {
                    return None;
                }
            }
    }
    Some(out)
}

/// A data de `-t` e `-tt`: `yyyy-mm-dd` ou `mmddyyyy`.
fn parse_date(value: &[u8]) -> Option<u64> {
    let (yyyy, mm, dd) = if let Some([y, m, d]) = scan_date(value, [4, 2, 2], Some(b'-')) {
        (y, m, d)
    } else if let Some([m, d, y]) = scan_date(value, [2, 2, 4], None) {
        (y, m, d)
    } else {
        return None;
    };
    if !(1..=12).contains(&mm) || !(1..=31).contains(&dd) {
        return None;
    }
    Some(dostime(yyyy, mm, dd, 0, 0, 0))
}

impl Zip {
    /// O `main` do zip: ajuda e versão sem argumentos, ambiente, opções e execução.
    pub fn run_main(&mut self, argv: Vec<Vec<u8>>) -> R<i32> {
        let s = sys::current();
        if argv.len() == 1 && s.isatty(Fd::STDOUT) {
            self.help();
            return Ok(ZE_OK);
        }
        if argv.len() == 2 && argv[1] == b"-v" && (s.isatty(Fd::STDOUT) || s.isatty(Fd::STDIN)) {
            self.version_info();
            return Ok(ZE_OK);
        }
        let argv = envargs(argv, sysutil::getenv);
        let argc = argv.len();
        if let Some(code) = self.parse_cmdline(argv)? {
            return Ok(code);
        }
        self.execute(argc)
    }

    fn out_stdout(&mut self, b: &[u8]) {
        let _ = sys::write_all(Fd::STDOUT, b);
    }

    /// `help`: a ajuda (com o aviso de direitos autorais) no stdout.
    fn help(&mut self) {
        self.out_stdout(text::HELP);
    }

    /// `version_info`: a tela de `zip -v`, com os valores de `ZIP` e `ZIPOPT` do ambiente.
    fn version_info(&mut self) {
        let mut out = text::VERSION_HEAD.to_vec();
        for name in ["ZIP", "ZIPOPT"] {
            let v = sysutil::getenv(name).filter(|v| !v.is_empty());
            out.extend_from_slice(format!("{:>16}:  ", name).as_bytes());
            match v {
                Some(v) => out.extend_from_slice(&v),
                None => out.extend_from_slice(b"[none]"),
            }
            out.push(b'\n');
        }
        self.out_stdout(&out);
    }

    /// `zipstdout`: prepara a saída do zip pelo stdout (as mensagens passam a ir ao stderr).
    fn zipstdout(&mut self) -> R<()> {
        self.mesg_to_stderr = true;
        if sys::current().isatty(Fd::STDOUT) {
            return Err(self.ziperr(ZE_PARMS, "cannot write zip file to terminal"));
        }
        self.zipfile = b"-".to_vec();
        Ok(())
    }

    fn asctime_now(&self) -> String {
        let now = self.now_sec();
        crate::tz::format(now, 0, &self.tz, "%a %b %e %H:%M:%S %Y")
    }

    /// Lê e interpreta a linha de comando. Devolve `Some(código)` se o programa deve terminar já
    /// (ajuda, versão, `-sf`...), ou `None` para seguir.
    pub fn parse_cmdline(&mut self, argv: Vec<Vec<u8>>) -> R<Option<i32>> {
        let mut st = OptState::new(argv);
        let mut s_flag = false;
        let mut key_needed = false;
        let mut seen_doubledash = false;
        let mut show_options = false;
        let mut show_args = false;
        self.kk = 0;

        loop {
            let (option, value, negated) = match get_option(&mut st) {
                Ok(r) => r,
                Err(m) => return Err(self.ziperr(ZE_PARMS, &m)),
            };
            if option == 0 {
                break;
            }
            let val: Vec<u8> = value.clone().unwrap_or_default();
            match option {
                x if x == c0('0') => {
                    self.method = STORE;
                    self.level = 0;
                }
                x if (c0('1')..=c0('9')).contains(&x) => self.level = (x - c0('0')) as i32,
                x if x == c0('A') => self.adjust = true,
                x if x == c0('b') => {
                    self.tempdir = true;
                    self.tempath = value;
                }
                x if x == c0('c') => self.comadd = true,
                x if x == c0('d') => {
                    if self.action != ADD {
                        return Err(self.ziperr(ZE_PARMS, "specify just one action"));
                    }
                    self.action = DELETE;
                }
                O_DB => self.display_bytes = !negated,
                O_DC => self.display_counts = !negated,
                O_DD => {
                    self.display_globaldots = false;
                    if negated {
                        self.dot_count = 0;
                    } else {
                        if self.dot_count == 0 {
                            self.dot_size = 10 * 0x100000;
                        }
                        self.dot_count = -1;
                    }
                }
                O_DG => {
                    if negated {
                        self.display_globaldots = false;
                    } else {
                        self.display_globaldots = true;
                        if self.dot_count == 0 {
                            self.dot_size = 10 * 0x100000;
                        }
                        self.dot_count = -1;
                    }
                }
                O_DS => {
                    if val.is_empty() {
                        self.dot_size = 10 * 0x100000;
                    } else {
                        let mut warns: Vec<(String, Vec<u8>)> = Vec::new();
                        let r = read_num_string(&val, &mut |m, v| warns.push((m.to_string(), v.to_vec())));
                        for (m, v) in warns {
                            self.zipwarn(m, v);
                        }
                        match r {
                            None => {
                                let msg = format!("option -ds (--dot-size) has bad size:  '{}'", String::from_utf8_lossy(&val));
                                return Err(self.ziperr(ZE_PARMS, &msg));
                            }
                            Some(v) => {
                                self.dot_size = v as i64;
                                if self.dot_size < 0x400 {
                                    self.dot_size *= 0x100000;
                                } else if self.dot_size < 0x400 * 32 {
                                    let msg = format!("dot size must be at least 32 KB:  '{}'", String::from_utf8_lossy(&val));
                                    return Err(self.ziperr(ZE_PARMS, &msg));
                                }
                            }
                        }
                    }
                    self.dot_count = -1;
                }
                O_DU => self.display_usize = !negated,
                O_DV => self.display_volume = !negated,
                x if x == c0('D') => self.dirnames = false,
                O_DF_ => {
                    self.diff_mode = true;
                    self.allow_empty_archive = true;
                }
                x if x == c0('e') => key_needed = true,
                x if x == c0('F') => self.fix = 1,
                O_FF => self.fix = 2,
                O_FI => self.allow_fifo = !negated,
                O_FS => self.filesync = true,
                x if x == c0('f') => {
                    if self.action != ADD {
                        return Err(self.ziperr(ZE_PARMS, "specify just one action"));
                    }
                    self.action = FRESHEN;
                }
                x if x == c0('g') => self.grow = true,
                x if x == c0('h') => {
                    self.help();
                    return Ok(Some(self.finish(ZE_OK)?));
                }
                O_H2 => {
                    self.out_stdout(text::HELP2);
                    return Ok(Some(self.finish(ZE_OK)?));
                }
                x if x == c0('j') => self.pathput = false,
                x if x == c0('J') => self.junk_sfx = true,
                x if x == c0('k') => self.dosify = true,
                x if x == c0('l') => self.translate_eol = 1,
                O_LL => self.translate_eol = 2,
                O_LF => self.logfile_path = value,
                O_LA => self.logfile_append = !negated,
                O_LI => self.logall = !negated,
                x if x == c0('L') => {
                    self.out_stdout(text::LICENSE);
                    return Ok(Some(self.finish(ZE_OK)?));
                }
                x if x == c0('m') => self.dispose = true,
                O_MM_LOWER => return Err(self.ziperr(ZE_PARMS, "-mm not supported, Must_Match is -MM")),
                O_MM => self.bad_open_is_error = true,
                x if x == c0('n') => self.special = value,
                O_NW => self.no_wild = true,
                x if x == c0('o') => self.latest = true,
                x if x == c0('O') => {
                    self.out_path = self.ziptyp(&val);
                    self.have_out = true;
                }
                x if x == c0('p') => {}
                x if x == c0('P') => {
                    self.key = value;
                    key_needed = false;
                }
                x if x == c0('q') => {
                    self.noisy = false;
                    if self.verbose > 0 {
                        self.verbose -= 1;
                    }
                }
                x if x == c0('r') => {
                    if self.recurse == 2 {
                        return Err(self.ziperr(ZE_PARMS, "do not specify both -r and -R"));
                    }
                    self.recurse = 1;
                }
                x if x == c0('R') => {
                    if self.recurse == 1 {
                        return Err(self.ziperr(ZE_PARMS, "do not specify both -r and -R"));
                    }
                    self.recurse = 2;
                }
                O_RE => self.allow_regex = true,
                O_SC => show_args = true,
                O_SD => self.show_sd = true,
                O_SF => self.show_files = if negated { 2 } else { 1 },
                O_SO => show_options = true,
                O_SU => self.show_files = if negated { 4 } else { 3 },
                O_SU_UPPER => self.show_files = if negated { 6 } else { 5 },
                x if x == c0('s') => {
                    if val == b"-" {
                        // -s-: sem divisão.
                    } else {
                        let mut warns: Vec<(String, Vec<u8>)> = Vec::new();
                        let r = read_num_string(&val, &mut |m, v| warns.push((m.to_string(), v.to_vec())));
                        for (m, v) in warns {
                            self.zipwarn(m, v);
                        }
                        match r {
                            None => {
                                let msg = format!("bad split size:  '{}'", String::from_utf8_lossy(&val));
                                return Err(self.ziperr(ZE_PARMS, &msg));
                            }
                            Some(0) => {}
                            Some(mut v) => {
                                self.split_requested = true;
                                if v < 0x400 {
                                    v *= 0x100000;
                                }
                                if v < 0x400 * 64 {
                                    let msg = format!("minimum split size is 64 KB:  '{}'", String::from_utf8_lossy(&val));
                                    return Err(self.ziperr(ZE_PARMS, &msg));
                                }
                            }
                        }
                    }
                }
                O_SB | O_SV => {}
                O_SP => {
                    self.use_descriptors = true;
                    self.split_requested = true;
                }
                x if x == c0('t') => match parse_date(&val) {
                    Some(t) => self.before = t,
                    None => return Err(self.ziperr(ZE_PARMS, "invalid date entered for -t option - use mmddyyyy or yyyy-mm-dd")),
                },
                O_TT => match parse_date(&val) {
                    Some(t) => self.after = t,
                    None => return Err(self.ziperr(ZE_PARMS, "invalid date entered for -tt option - use mmddyyyy or yyyy-mm-dd")),
                },
                x if x == c0('T') => self.test = true,
                O_TT_UPPER => self.unzip_path = value,
                x if x == c0('U') => {
                    if self.action != ADD {
                        return Err(self.ziperr(ZE_PARMS, "specify just one action"));
                    }
                    self.action = ARCHIVE;
                }
                O_UN => {
                    if abbrevmatch("quit", &val) {
                        self.unicode_mismatch = 0;
                    } else if abbrevmatch("warn", &val) {
                        self.unicode_mismatch = 1;
                    } else if abbrevmatch("ignore", &val) {
                        self.unicode_mismatch = 2;
                    } else if abbrevmatch("no", &val) {
                        self.unicode_mismatch = 3;
                    } else if abbrevmatch("escape", &val) {
                        self.unicode_escape_all = true;
                    } else if abbrevmatch("UTF8", &val) {
                        self.utf8_force = true;
                    } else {
                        self.zipwarn("-UN must be Quit, Warn, Ignore, No, Escape, or UTF8: ", &val);
                        return Err(self.ziperr(ZE_PARMS, "-UN (unicode) bad value"));
                    }
                }
                x if x == c0('u') => {
                    if self.action != ADD {
                        return Err(self.ziperr(ZE_PARMS, "specify just one action"));
                    }
                    self.action = UPDATE;
                }
                x if x == c0('v') || x == O_VE => {
                    if x == O_VE || (st.args.len() == 2 && st.args[1].len() == 2) {
                        self.version_info();
                        return Ok(Some(self.finish(ZE_OK)?));
                    }
                    self.noisy = true;
                    self.verbose += 1;
                }
                O_WS => self.wild_stop_at_dir = true,
                x if x == c0('i') || x == c0('x') => {
                    if x == c0('i') {
                        self.allow_empty_archive = true;
                    }
                    self.add_filter(x as u8, &val)?;
                }
                x if x == c0('y') => self.linkput = true,
                x if x == c0('z') => self.zipedit = true,
                x if x == c0('Z') => {
                    if abbrevmatch("deflate", &val) {
                        self.method = DEFLATE;
                    } else if abbrevmatch("store", &val) {
                        self.method = STORE;
                    } else if abbrevmatch("bzip2", &val) {
                        self.method = BZIP2;
                    } else {
                        self.zipwarn("valid compression methods are:  store, deflate, bzip2", "");
                        self.zipwarn("unknown compression method found:  ", &val);
                        return Err(self.ziperr(ZE_PARMS, "Option -Z (--compression-method):  unknown method"));
                    }
                }
                x if x == c0('@') => {
                    self.comment_stdin = false;
                    s_flag = true;
                }
                x if x == c0('X') => self.extra_fields = if negated { 2 } else { 0 },
                O_DES => self.use_descriptors = true,
                O_Z64 => self.force_zip64 = if negated { 0 } else { 1 },
                O_NON_OPTION_ARG => {
                    if self.recurse != 2 && self.kk == 0 && self.patterns.is_empty() {
                        self.filterlist_to_patterns();
                    }
                    if val == b"--" && !seen_doubledash {
                        seen_doubledash = true;
                        if self.kk == 0 {
                            return Err(self.ziperr(ZE_PARMS, "can't use -- before archive name"));
                        }
                    } else if self.kk == 6 {
                        self.add_filter(b'R', &val)?;
                        if self.first_listarg == 0 {
                            self.first_listarg = st.argnum;
                        }
                    } else if self.kk == 0 {
                        if val == b"-" {
                            self.zipstdout()?;
                        } else {
                            self.zipfile = self.ziptyp(&val);
                        }
                        if self.show_sd {
                            let m = format!("Zipfile name '{}'", String::from_utf8_lossy(&self.zipfile));
                            self.sd(&m);
                        }
                        if self.in_path.is_empty() {
                            self.in_path = self.zipfile.clone();
                        }
                        if self.out_path.is_empty() {
                            self.out_path = self.zipfile.clone();
                        }
                        self.kk = 3;
                        if s_flag {
                            let names = self.read_names(Fd::STDIN);
                            for pp in names {
                                self.kk = 4;
                                if self.recurse == 2 {
                                    self.add_filter(b'R', &pp)?;
                                } else {
                                    self.filelist.push(pp);
                                }
                            }
                            s_flag = false;
                        }
                        if self.recurse == 2 {
                            self.kk = 6;
                        }
                    } else if self.kk == 3 || self.kk == 4 {
                        if s_flag && val == b"-" {
                            return Err(self.ziperr(ZE_PARMS, "can't read input (-) and filenames (-@) both from stdin"));
                        }
                        self.filelist.push(val.clone());
                        if self.kk == 3 {
                            self.first_listarg = st.argnum;
                            self.kk = 4;
                        }
                    }
                }
                _ => {
                    let m = format!("no such option ID: {}", option);
                    return Err(self.ziperr(ZE_PARMS, &m));
                }
            }
        }
        self.args_final = st.args.clone();

        // A senha, se pedida e ainda não dada.
        if key_needed {
            let pw = self.ask_password()?;
            self.key = Some(pw);
        }
        if let Some(k) = &self.key
            && k.is_empty() {
                return Err(self.ziperr(ZE_PARMS, "zero length password not allowed"));
            }
        if self.show_sd {
            self.sd("Command line read");
        }
        if show_args {
            self.mesg_raw(b"command line:\n");
            let args = self.args_final.clone();
            for a in &args {
                let mut l = b"'".to_vec();
                l.extend_from_slice(a);
                l.extend_from_slice(b"'  ");
                self.mesg_raw(&l);
            }
            self.mesg_raw(b"\n");
            return Err(self.ziperr(ZE_ABORT, "show command line"));
        }
        if show_options {
            let mut out = String::new();
            out.push_str("available options:\n");
            out.push_str(&format!(" {:<2}  {:<18} {:<4} {:<3} {:<30}\n", "sh", "long", "val", "neg", "description"));
            out.push_str(&format!(" {:<2}  {:<18} {:<4} {:<3} {:<30}\n", "--", "----", "---", "---", "-----------"));
            for o in OPTIONS {
                out.push_str(&format!(" {:<2}  {:<18} ", o.short, o.long));
                out.push_str(match o.vt {
                    Vt::NoValue => "     ",
                    Vt::Required => "req  ",
                    Vt::ValueList => "list ",
                });
                out.push_str(if o.neg { "neg " } else { "    " });
                if o.name.is_empty() {
                    out.push('\n');
                } else {
                    out.push_str(&format!("{:<30}\n", o.name));
                }
            }
            self.out_stdout(out.as_bytes());
            return Ok(Some(self.finish(ZE_OK)?));
        }

        // O arquivo de log.
        if let Some(lp) = self.logfile_path.clone() {
            let mut path = lp.clone();
            let lastp = path.iter().rposition(|&c| c == b'/').map(|i| i + 1).unwrap_or(0);
            if !path[lastp..].contains(&b'.') {
                path.extend_from_slice(b".log");
            }
            let flags = if self.logfile_append { OFlags::WRONLY | OFlags::CREAT | OFlags::APPEND } else { OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC };
            match sys::open(&path, flags | OFlags::CLOEXEC, 0o666) {
                Ok(fd) => self.logfile = Some(fd),
                Err(_) => {
                    let msg = format!("could not open logfile '{}'", String::from_utf8_lossy(&path));
                    return Err(self.ziperr(ZE_PARMS, &msg));
                }
            }
            let mut head = b"---------\n".to_vec();
            head.extend_from_slice(format!("Zip log opened {}\n", self.asctime_now()).as_bytes());
            head.extend_from_slice(b"command line arguments:\n ");
            for a in self.args_final.iter().skip(1) {
                let has_space = a.iter().any(|c| c.is_ascii_whitespace() || *c == 0x0b);
                if has_space {
                    head.push(b'"');
                    head.extend_from_slice(a);
                    head.extend_from_slice(b"\" ");
                } else {
                    head.extend_from_slice(a);
                    head.push(b' ');
                }
            }
            head.extend_from_slice(b"\n\n");
            self.log_raw(&head);
        } else {
            self.logall = false;
        }

        if self.split_requested {
            return Err(self.ziperr(ZE_COMPERR, "split archives (-s, -sp) are not supported by this zip"));
        }
        if self.verbose > 0 && self.dot_size == 0 && self.dot_count == 0 {
            self.dot_size = 10 * 0x100000;
        }
        if self.pcount() > 0 && self.patterns.is_empty() {
            self.filterlist_to_patterns();
        }
        if self.have_out && self.kk == 3 {
            self.copy_only = true;
            self.action = ARCHIVE;
        }
        if self.have_out && super::matching::namecmp(&self.in_path, &self.out_path) == 0 {
            let msg = format!("--out path must be different than in path: {}", String::from_utf8_lossy(&self.out_path));
            return Err(self.ziperr(ZE_PARMS, &msg));
        }
        if self.fix != 0 && self.diff_mode {
            return Err(self.ziperr(ZE_PARMS, "can't use --diff (-DF) with fix (-F or -FF)"));
        }
        if self.action == ARCHIVE && !self.have_out && self.show_files == 0 {
            return Err(self.ziperr(ZE_PARMS, "-U (--copy) requires -O (--out)"));
        }
        if self.fix != 0 && !self.have_out {
            self.zipwarn("fix options -F and -FF require --out:\n", "                     zip -F indamagedarchive --out outfixedarchive");
            return Err(self.ziperr(ZE_PARMS, "fix options require --out"));
        }
        if self.fix != 0 && !self.copy_only {
            return Err(self.ziperr(ZE_PARMS, "no other actions allowed when fixing archive (-F or -FF)"));
        }
        if !self.have_out && self.diff_mode {
            return Err(self.ziperr(ZE_PARMS, "-DF (--diff) requires -O (--out)"));
        }
        if self.diff_mode && (self.action == ARCHIVE || self.action == DELETE) {
            return Err(self.ziperr(ZE_PARMS, "can't use --diff (-DF) with -d or -U"));
        }
        if self.action != ARCHIVE && (self.recurse == 2 || self.pcount() > 0) && self.first_listarg == 0 && self.filelist.is_empty() && (self.kk < 3 || (self.action != UPDATE && self.action != FRESHEN)) {
            return Err(self.ziperr(ZE_PARMS, "nothing to select from"));
        }
        if self.fix != 0 {
            return Err(self.ziperr(ZE_COMPERR, "-F and -FF (fixing archives) are not supported by this zip"));
        }

        // Sem arquivo zip nem lista: o zip age como filtro (stdin para stdout).
        if self.kk < 3 {
            self.zipstdout()?;
            self.comment_stdin = false;
            match self.procname(b"-", false)? {
                ZE_OK => {}
                _ => {
                    if self.bad_open_is_error {
                        self.zipwarn("name not matched: ", "-");
                        return Err(self.ziperr(ZE_OPEN, "-"));
                    }
                    self.zipwarn("name not matched: ", "-");
                }
            }
            self.kk = 4;
            if s_flag {
                return Err(self.ziperr(ZE_PARMS, "can't use - and -@ together"));
            }
        }
        if self.zipfile == b"-" {
            if self.show_sd {
                self.sd("Zipping to stdout");
            }
            self.zip_to_stdout = true;
        }
        // Combinações de opções.
        match self.special.clone() {
            None => return Err(self.ziperr(ZE_PARMS, "missing suffix list")),
            Some(sp) => {
                if self.level == 9 || sp == b";" || sp == b":" {
                    self.special = None;
                }
            }
        }
        if self.action == DELETE && (self.method != BEST || self.dispose || self.recurse != 0 || self.key.is_some() || self.comadd || self.zipedit) {
            self.zipwarn("invalid option(s) used with -d; ignored.", "");
            self.method = BEST;
            self.dispose = false;
            self.recurse = 0;
            self.key = None;
            self.comadd = false;
            self.zipedit = false;
        }
        if self.action == ARCHIVE && (self.method != BEST || self.dispose || self.recurse != 0 || self.comadd || self.zipedit) {
            self.zipwarn("can't set method, move, recurse, or comments with copy mode.", "");
            self.method = BEST;
            self.dispose = false;
            self.recurse = 0;
            self.comadd = false;
            self.zipedit = false;
        }
        if self.linkput && self.dosify {
            self.zipwarn("can't use -y with -k, -y ignored", "");
            self.linkput = false;
        }
        if self.test && self.zip_to_stdout {
            self.test = false;
            self.zipwarn("can't use -T on stdout, -T ignored", "");
        }
        if (self.action != ADD || self.grow) && self.filesync {
            return Err(self.ziperr(ZE_PARMS, "can't use -d, -f, -u, -U, or -g with filesync -FS\n"));
        }
        if (self.action != ADD || self.grow) && self.zip_to_stdout {
            return Err(self.ziperr(ZE_PARMS, "can't use -d, -f, -u, -U, or -g on stdout\n"));
        }
        Ok(None)
    }

    /// `encr_passwd` duas vezes (senha e verificação), lendo do terminal sem eco quando existe um.
    fn ask_password(&mut self) -> R<Vec<u8>> {
        let tty = match sys::open(b"/dev/tty", OFlags::RDONLY | OFlags::CLOEXEC, 0) {
            Ok(fd) => fd,
            Err(_) => return Err(self.ziperr(ZE_PARMS, "stderr is not a tty")),
        };
        let read_line = |this: &mut Zip, prompt: &str| -> Vec<u8> {
            this.stderr_raw(prompt.as_bytes());
            let mut line = Vec::new();
            let mut c = [0u8; 1];
            loop {
                match sys::read(tty, &mut c) {
                    Ok(1) => {
                        if c[0] == b'\n' {
                            break;
                        }
                        line.push(c[0]);
                    }
                    _ => break,
                }
            }
            this.stderr_raw(b"\n");
            line.truncate(IZ_PWLEN);
            line
        };
        let first = read_line(self, "Enter password: ");
        if first.is_empty() {
            let _ = sys::close(tty);
            return Err(self.ziperr(ZE_PARMS, "zero length password not allowed"));
        }
        let second = read_line(self, "Verify password: ");
        let _ = sys::close(tty);
        if first != second {
            return Err(self.ziperr(ZE_PARMS, "password verification failed"));
        }
        Ok(first)
    }
}

fn c0(ch: char) -> u32 {
    ch as u32
}
