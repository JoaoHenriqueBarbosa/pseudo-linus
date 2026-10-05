//! A linha de comando do top: o `getopt_long` da glibc com o remendo do próprio top (`GETOPTFIX`,
//! que pega a palavra seguinte como argumento de qualquer opção), `parse_args` e as conversões
//! `mkfloat` e `user_certify`.

use ul_misc::util::io;

use super::fields::{EU_MAXPFLGS, NAMES};
use super::{R, Top};
use crate::ps::util::strtoul0;

/// `W_MIN_COL` e `SCREENMAX`: os limites de `-w`.
const W_MIN_COL: f32 = 3.0;
const SCREENMAX: f32 = 512.0;
/// `MONPIDMAX`: quantos pids `-p` aceita.
const MONPIDMAX: usize = 20;

/// O texto de `-h` (`HELP_cmdline_fmt`); o `%s` é o nome do programa.
const HELP: &str = "\nUsage:\n %s [options]\n\nOptions:\n -b, --batch-mode                run in non-interactive batch mode\n -c, --cmdline-toggle            reverse last remembered 'c' state\n -d, --delay =SECS [.TENTHS]     iterative delay as SECS [.TENTHS]\n -E, --scale-summary-mem =SCALE  set mem as: k,m,g,t,p,e for SCALE\n -e, --scale-task-mem =SCALE     set mem with: k,m,g,t,p for SCALE\n -H, --threads-show              show tasks plus all their threads\n -i, --idle-toggle               reverse last remembered 'i' state\n -n, --iterations =NUMBER        exit on maximum iterations NUMBER\n -O, --list-fields               output all field names, then exit\n -o, --sort-override =FIELD      force sorting on this named FIELD\n -p, --pid =PIDLIST              monitor only the tasks in PIDLIST\n -S, --accum-time-toggle         reverse last remembered 'S' state\n -s, --secure-mode               run with secure mode restrictions\n -U, --filter-any-user =USER     show only processes owned by USER\n -u, --filter-only-euser =USER   show only processes owned by USER\n -w, --width [=COLUMNS]          change print width [,use COLUMNS]\n -1, --single-cpu-toggle         reverse last remembered '1' state\n\n -h, --help                      display this help text, then exit\n -V, --version                   output version information & exit\n\nFor more details see top(1).";

/// As opções curtas (`sopts`).
const SHORT: &[u8] = b"bcd:E:e:Hhin:Oo:p:SsU:u:Vw::1";

/// As opções longas: nome, se leva argumento (0 não, 1 sim, 2 opcional) e o código curto.
const LONGS: [(&str, u8, u8); 19] = [
    ("batch-mode", 0, b'b'),
    ("cmdline-toggle", 0, b'c'),
    ("delay", 1, b'd'),
    ("scale-summary-mem", 1, b'E'),
    ("scale-task-mem", 1, b'e'),
    ("threads-show", 0, b'H'),
    ("help", 0, b'h'),
    ("idle-toggle", 0, b'i'),
    ("iterations", 1, b'n'),
    ("list-fields", 0, b'O'),
    ("sort-override", 1, b'o'),
    ("pid", 1, b'p'),
    ("accum-time-toggle", 0, b'S'),
    ("secure-mode", 0, b's'),
    ("filter-any-user", 1, b'U'),
    ("filter-only-euser", 1, b'u'),
    ("version", 0, b'V'),
    ("width", 2, b'w'),
    ("single-cpu-toggle", 0, b'1'),
];

/// O `getopt_long` da glibc 2.41 (com permutação e as mensagens de erro no stderr).
struct Getopt {
    argv: Vec<Vec<u8>>,
    optind: usize,
    optarg: Option<Vec<u8>>,
    nextchar: Vec<u8>,
    first_nonopt: usize,
    last_nonopt: usize,
}

impl Getopt {
    fn new(argv: Vec<Vec<u8>>) -> Getopt {
        Getopt { argv, optind: 1, optarg: None, nextchar: Vec::new(), first_nonopt: 1, last_nonopt: 1 }
    }

    fn prog(&self) -> String {
        io::lossy(&self.argv[0])
    }

    fn nonoption(&self, i: usize) -> bool {
        let a = &self.argv[i];
        a.first() != Some(&b'-') || a.len() == 1
    }

    /// `exchange`: troca o bloco de não-opções `[first, last)` com as opções `[last, optind)`.
    fn exchange(&mut self) {
        let bottom = self.first_nonopt;
        let middle = self.last_nonopt;
        let top = self.optind;
        let block: Vec<Vec<u8>> = self.argv[bottom..top].to_vec();
        let nonopts = &block[..middle - bottom];
        let opts = &block[middle - bottom..];
        let mut merged: Vec<Vec<u8>> = Vec::new();
        merged.extend_from_slice(opts);
        merged.extend_from_slice(nonopts);
        for (k, v) in merged.into_iter().enumerate() {
            self.argv[bottom + k] = v;
        }
        self.first_nonopt += self.optind - self.last_nonopt;
        self.last_nonopt = self.optind;
    }

    /// `process_long_option` para o prefixo `--`.
    fn process_long_option(&mut self) -> u8 {
        let next = std::mem::take(&mut self.nextchar);
        let nameend = next.iter().position(|b| *b == b'=').unwrap_or(next.len());
        let name = &next[..nameend];
        let shown = io::lossy(&next);
        let mut pfound: Option<usize> = None;
        let mut ambig: Vec<usize> = Vec::new();
        for (i, l) in LONGS.iter().enumerate() {
            if !l.0.as_bytes().starts_with(name) {
                continue;
            }
            if l.0.len() == name.len() {
                pfound = Some(i);
                ambig.clear();
                break;
            }
            match pfound {
                None => pfound = Some(i),
                Some(f) => {
                    if LONGS[f].1 != l.1 || LONGS[f].2 != l.2 {
                        if ambig.is_empty() {
                            ambig.push(f);
                        }
                        ambig.push(i);
                    }
                }
            }
        }
        if !ambig.is_empty() {
            let mut m = format!("{}: option '--{shown}' is ambiguous; possibilities:", self.prog());
            for i in ambig {
                m.push_str(&format!(" '--{}'", LONGS[i].0));
            }
            m.push('\n');
            io::eprint(m);
            self.optind += 1;
            return b'?';
        }
        let Some(idx) = pfound else {
            io::eprint(format!("{}: unrecognized option '--{shown}'\n", self.prog()));
            self.optind += 1;
            return b'?';
        };
        let (long_name, has_arg, val) = LONGS[idx];
        self.optind += 1;
        if nameend < next.len() {
            if has_arg != 0 {
                self.optarg = Some(next[nameend + 1..].to_vec());
            } else {
                io::eprint(format!("{}: option '--{long_name}' doesn't allow an argument\n", self.prog()));
                return b'?';
            }
        } else if has_arg == 1 {
            if self.optind < self.argv.len() {
                self.optarg = Some(self.argv[self.optind].clone());
                self.optind += 1;
            } else {
                io::eprint(format!("{}: option '--{long_name}' requires an argument\n", self.prog()));
                return b'?';
            }
        }
        val
    }

    /// A próxima opção (`Some(código)`, `b'?'` para erro) ou `None` quando acabaram.
    fn next(&mut self) -> Option<u8> {
        self.optarg = None;
        let argc = self.argv.len();
        if self.nextchar.is_empty() {
            if self.last_nonopt > self.optind {
                self.last_nonopt = self.optind;
            }
            if self.first_nonopt > self.optind {
                self.first_nonopt = self.optind;
            }
            if self.first_nonopt != self.last_nonopt && self.last_nonopt != self.optind {
                self.exchange();
            } else if self.last_nonopt != self.optind {
                self.first_nonopt = self.optind;
            }
            while self.optind < argc && self.nonoption(self.optind) {
                self.optind += 1;
            }
            self.last_nonopt = self.optind;
            if self.optind != argc && self.argv[self.optind] == b"--" {
                self.optind += 1;
                if self.first_nonopt != self.last_nonopt && self.last_nonopt != self.optind {
                    self.exchange();
                } else if self.first_nonopt == self.last_nonopt {
                    self.first_nonopt = self.optind;
                }
                self.last_nonopt = argc;
                self.optind = argc;
            }
            if self.optind == argc {
                if self.first_nonopt != self.last_nonopt {
                    self.optind = self.first_nonopt;
                }
                return None;
            }
            let cur = self.argv[self.optind].clone();
            if cur.get(1) == Some(&b'-') {
                self.nextchar = cur[2..].to_vec();
                return Some(self.process_long_option());
            }
            self.nextchar = cur[1..].to_vec();
        }
        let c = self.nextchar.remove(0);
        let temp = SHORT.iter().position(|b| *b == c);
        if self.nextchar.is_empty() {
            self.optind += 1;
        }
        let Some(t) = temp else {
            io::eprint(format!("{}: invalid option -- '{}'\n", self.prog(), char::from(c)));
            return Some(b'?');
        };
        if c == b':' || c == b';' {
            io::eprint(format!("{}: invalid option -- '{}'\n", self.prog(), char::from(c)));
            return Some(b'?');
        }
        if SHORT.get(t + 1) == Some(&b':') {
            if SHORT.get(t + 2) == Some(&b':') {
                if !self.nextchar.is_empty() {
                    self.optarg = Some(std::mem::take(&mut self.nextchar));
                    self.optind += 1;
                }
            } else if !self.nextchar.is_empty() {
                self.optarg = Some(std::mem::take(&mut self.nextchar));
                self.optind += 1;
            } else if self.optind == argc {
                io::eprint(format!("{}: option requires an argument -- '{}'\n", self.prog(), char::from(c)));
                return Some(b'?');
            } else {
                self.optarg = Some(self.argv[self.optind].clone());
                self.optind += 1;
            }
            self.nextchar.clear();
        }
        Some(c)
    }
}

/// `strtof` no prefixo de `s`: o valor e quantos bytes foram lidos (0 se não há número).
fn strtof_prefix(s: &[u8]) -> (f32, usize) {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let neg = s.get(i) == Some(&b'-');
    if matches!(s.get(i), Some(b'-' | b'+')) {
        i += 1;
    }
    let rest = &s[i..];
    let lower: Vec<u8> = rest.iter().take(8).map(u8::to_ascii_lowercase).collect();
    let signed = |v: f32| if neg { -v } else { v };
    if lower.starts_with(b"infinity") {
        return (signed(f32::INFINITY), i + 8);
    }
    if lower.starts_with(b"inf") {
        return (signed(f32::INFINITY), i + 3);
    }
    if lower.starts_with(b"nan") {
        return (f32::NAN, i + 3);
    }
    // Hexadecimal: 0x, dígitos, fração opcional e expoente binário opcional.
    if lower.starts_with(b"0x") && rest.get(2).is_some_and(|b| b.is_ascii_hexdigit() || *b == b'.') {
        let mut j = 2;
        let mut mant = 0f64;
        let mut any = false;
        while j < rest.len() && rest[j].is_ascii_hexdigit() {
            mant = mant * 16.0 + f64::from((rest[j] as char).to_digit(16).unwrap_or(0));
            any = true;
            j += 1;
        }
        if rest.get(j) == Some(&b'.') {
            let mut scale = 1.0 / 16.0;
            let mut k = j + 1;
            while k < rest.len() && rest[k].is_ascii_hexdigit() {
                mant += f64::from((rest[k] as char).to_digit(16).unwrap_or(0)) * scale;
                scale /= 16.0;
                any = true;
                k += 1;
            }
            if any {
                j = k;
            }
        }
        if any {
            let mut exp = 0i32;
            if matches!(rest.get(j), Some(b'p' | b'P')) {
                let mut k = j + 1;
                let eneg = rest.get(k) == Some(&b'-');
                if matches!(rest.get(k), Some(b'-' | b'+')) {
                    k += 1;
                }
                let ds = k;
                let mut e = 0i32;
                while k < rest.len() && rest[k].is_ascii_digit() {
                    e = e.saturating_mul(10).saturating_add(i32::from(rest[k] - b'0'));
                    k += 1;
                }
                if k > ds {
                    exp = if eneg { -e } else { e };
                    j = k;
                }
            }
            return (signed((mant * 2f64.powi(exp)) as f32), i + j);
        }
    }
    let mut j = 0;
    while j < rest.len() && rest[j].is_ascii_digit() {
        j += 1;
    }
    let int_digits = j;
    let mut frac_digits = 0;
    if rest.get(j) == Some(&b'.') {
        let mut k = j + 1;
        while k < rest.len() && rest[k].is_ascii_digit() {
            k += 1;
        }
        frac_digits = k - j - 1;
        if int_digits + frac_digits > 0 {
            j = k;
        }
    }
    if int_digits + frac_digits == 0 {
        return (0.0, 0);
    }
    if matches!(rest.get(j), Some(b'e' | b'E')) {
        let mut k = j + 1;
        if matches!(rest.get(k), Some(b'-' | b'+')) {
            k += 1;
        }
        let ds = k;
        while k < rest.len() && rest[k].is_ascii_digit() {
            k += 1;
        }
        if k > ds {
            j = k;
        }
    }
    let text = String::from_utf8_lossy(&rest[..j]).into_owned();
    (signed(text.parse::<f32>().unwrap_or(0.0)), i + j)
}

/// `mkfloat`: o número de `s` (só inteiro com `whole`), aceitando `.` e `,` como separador.
fn mkfloat(s: &[u8], whole: bool) -> Option<f32> {
    if whole {
        let (v, used) = strtoul0(s);
        let num = (v as i64) as f32;
        return (used != 0 && used == s.len() && num < i32::MAX as f32).then_some(num);
    }
    let mut tmp: Vec<u8> = s.iter().copied().take(127).collect();
    let (mut num, mut used) = strtof_prefix(&tmp);
    if used != tmp.len() {
        match tmp.get(used) {
            Some(b'.') => tmp[used] = b',',
            Some(b',') => tmp[used] = b'.',
            _ => {}
        }
        (num, used) = strtof_prefix(&tmp);
    }
    (used != 0 && used == tmp.len() && num < i32::MAX as f32).then_some(num)
}

/// `sscanf(s, "%d", &n)`: o primeiro inteiro do texto, se há.
fn scan_int(s: &[u8]) -> Option<i32> {
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_whitespace() {
        i += 1;
    }
    let neg = s.get(i) == Some(&b'-');
    if matches!(s.get(i), Some(b'-' | b'+')) {
        i += 1;
    }
    let ds = i;
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = v.saturating_mul(10).saturating_add(i64::from(s[i] - b'0'));
        i += 1;
    }
    (i > ds).then(|| (if neg { -v } else { v }) as i32)
}

impl Top {
    /// `error_exit`: `top: mensagem` no stderr e saída 1.
    pub fn error_exit<T>(&self, msg: &str) -> R<T> {
        io::eprint(format!("{}: {msg}\n", self.myname));
        Err(1)
    }

    /// `user_certify`: valida `-u`/`-U` (nome ou número, com `!` para negar). `Some(msg)` é o erro.
    fn user_certify(&mut self, s: &[u8], typ: u8) -> Option<&'static str> {
        self.monpids.clear();
        self.w.usrseltyp = 0;
        self.w.usrselflg = true;
        if s.is_empty() {
            return None;
        }
        let mut s = s;
        if s[0] == b'!' {
            s = &s[1..];
            self.w.usrselflg = false;
        }
        let (num, used) = strtoul0(s);
        let uid: u32 = if used == s.len() {
            let num = num as u32;
            // Usuário de fora (de um chroot, por exemplo) também vale.
            if self.ps.names.user(num).is_none() {
                self.w.usrseluid = num;
                self.w.usrseltyp = typ;
                return None;
            }
            num
        } else {
            match std::str::from_utf8(s).ok().and_then(|n| self.ps.names.uid_of(n)) {
                Some(u) => u,
                None => return Some("Invalid user"),
            }
        };
        self.w.usrseluid = uid;
        self.w.usrseltyp = typ;
        None
    }

    /// `parse_args`: as opções do top. `Err(código)` encerra o programa.
    pub fn parse_args(&mut self, argv: Vec<Vec<u8>>) -> R<()> {
        let mut g = Getopt::new(argv);
        let mut tmp_delay = f32::MAX;
        while let Some(ch) = g.next() {
            let mut cp: Option<Vec<u8>> = g.optarg.clone();
            // O remendo do top: a palavra seguinte vira o argumento de qualquer opção.
            if cp.is_none() && g.optind < g.argv.len() && g.argv[g.optind].first() != Some(&b'-') {
                cp = Some(g.argv[g.optind].clone());
                g.optind += 1;
            }
            if let Some(c) = cp.take() {
                let mut c: &[u8] = &c;
                if c.first() == Some(&b'=') {
                    c = &c[1..];
                }
                if c.is_empty() {
                    cp = g.argv.get(g.optind).cloned();
                    g.optind += 1;
                } else {
                    cp = Some(c.to_vec());
                }
                if cp.is_none() {
                    return self.error_exit(&format!("-{} argument missing", char::from(ch)));
                }
            }
            match ch {
                b'1' => self.w.view_cpusum = !self.w.view_cpusum,
                b'b' => self.batch = true,
                b'c' => self.w.show_cmdline = !self.w.show_cmdline,
                b'd' => {
                    let a = cp.take().unwrap_or_default();
                    let Some(v) = mkfloat(&a, false) else {
                        return self.error_exit(&format!("bad delay interval '{}'", io::lossy(&a)));
                    };
                    tmp_delay = v;
                    if tmp_delay < 0.0 {
                        return self.error_exit("-d requires positive argument");
                    }
                }
                b'E' => {
                    let a = cp.take().unwrap_or_default();
                    match scale_index(&a, b"kmgtpe") {
                        Some(i) => self.summ_mscale = i,
                        None => return self.error_exit(&format!("bad memory scaling arg '{}'", io::lossy(&a))),
                    }
                }
                b'e' => {
                    let a = cp.take().unwrap_or_default();
                    match scale_index(&a, b"kmgtp") {
                        Some(i) => self.task_mscale = i,
                        None => return self.error_exit(&format!("bad memory scaling arg '{}'", io::lossy(&a))),
                    }
                }
                b'H' => self.thread_mode = true,
                b'h' => {
                    self.puts(&HELP.replace("%s", &self.myname.clone()));
                    return Err(self.bye_ok());
                }
                b'i' => self.w.show_idleps = !self.w.show_idleps,
                b'n' => {
                    let a = cp.take().unwrap_or_default();
                    match mkfloat(&a, true) {
                        Some(v) if v >= 1.0 => self.loops = v as i32,
                        _ => return self.error_exit(&format!("bad iterations argument '{}'", io::lossy(&a))),
                    }
                }
                b'O' => {
                    for n in NAMES.iter().take(EU_MAXPFLGS) {
                        self.puts(n);
                    }
                    return Err(self.bye_ok());
                }
                b'o' => {
                    let a = cp.take().unwrap_or_default();
                    let mut name: &[u8] = &a;
                    if name.first() == Some(&b'+') {
                        self.w.qsrt_normal = true;
                        name = &name[1..];
                    } else if name.first() == Some(&b'-') {
                        self.w.qsrt_normal = false;
                        name = &name[1..];
                    }
                    match NAMES.iter().position(|n| n.as_bytes() == name) {
                        Some(i) => self.w.sortindx = i,
                        None => {
                            return self.error_exit(&format!("unrecognized field name '{}'", io::lossy(name)));
                        }
                    }
                }
                b'p' => {
                    if self.w.usrseltyp != 0 {
                        return self.error_exit("conflicting process selections (U/p/u)");
                    }
                    let list = cp.take().unwrap_or_default();
                    let mut rest: &[u8] = &list;
                    loop {
                        if self.monpids.len() >= MONPIDMAX {
                            return self.error_exit(&format!("pid limit ({MONPIDMAX}) exceeded"));
                        }
                        let bad = rest.iter().any(|b| matches!(b, b'+' | b'-' | b'.'));
                        let Some(mut pid) = scan_int(rest).filter(|_| !bad) else {
                            return self.error_exit(&format!("bad pid '{}'", io::lossy(rest)));
                        };
                        if pid == 0 {
                            pid = sysabi::sys::current().getpid();
                        }
                        if !self.monpids.contains(&pid) {
                            self.monpids.push(pid);
                        }
                        match rest.iter().position(|b| *b == b',') {
                            Some(k) => rest = &rest[k + 1..],
                            None => break,
                        }
                        if rest.is_empty() {
                            break;
                        }
                    }
                }
                b'S' => self.w.show_ctimes = !self.w.show_ctimes,
                b's' => self.secure = true,
                b'U' | b'u' => {
                    if !self.monpids.is_empty() || self.w.usrseltyp != 0 {
                        return self.error_exit("conflicting process selections (U/p/u)");
                    }
                    let a = cp.take().unwrap_or_default();
                    if let Some(m) = self.user_certify(&a, ch) {
                        return self.error_exit(m);
                    }
                }
                b'V' => {
                    self.puts(&format!("{} from procps-ng 4.0.4", self.myname));
                    return Err(self.bye_ok());
                }
                b'w' => {
                    let mut tmp = -1.0f32;
                    if let Some(a) = cp.take() {
                        match mkfloat(&a, true) {
                            Some(v) if (W_MIN_COL..=SCREENMAX).contains(&v) => tmp = v,
                            _ => return self.error_exit(&format!("bad width arg '{}'", io::lossy(&a))),
                        }
                    }
                    self.width_mode = tmp as i32;
                }
                _ => return Err(1),
            }
            // Opção sem argumento que ganhou uma palavra solta: o remendo reclama dela.
            if let Some(extra) = cp {
                return self.error_exit(&format!("unknown option '{}'", io::lossy(&extra)));
            }
        }
        if g.optind < g.argv.len() {
            return self.error_exit(&format!("unknown option '{}'", io::lossy(&g.argv[g.optind])));
        }
        if tmp_delay < f32::MAX {
            if self.secure {
                return self.error_exit("-d disallowed in \"secure\" mode");
            }
            self.delay = tmp_delay;
        }
        Ok(())
    }
}

/// A posição de `*cp` (em minúscula) em `letters`, se `cp` tem um caractere só.
fn scale_index(cp: &[u8], letters: &[u8]) -> Option<usize> {
    if cp.len() != 1 {
        return None;
    }
    letters.iter().position(|b| *b == cp[0].to_ascii_lowercase())
}
