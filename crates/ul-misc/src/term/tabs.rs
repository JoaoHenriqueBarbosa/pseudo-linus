//! `tabs` do ncurses 6.5.20250216 (`tabs.c`): define as paradas de tabulação do terminal.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Fd, sys};

use super::terminfo::{SetupOpts, Term, isatty, setupterm};
use super::tparm::{ParmState, tiparm, tputs};
use super::{StdoutSink, VERSION, c_isspace, rootname, save_tty_settings};
use crate::util::io;

struct Tabs {
    progname: String,
    max_cols: i32,
    term: Term,
    state: ParmState,
}

fn out(data: &[u8]) {
    let _ = io::stdout().write_all(data);
}

fn usage(progname: &str) -> ! {
    // `fflush(stdout)` antes de escrever o erro.
    let _ = io::flush_stdout();
    let msg = "\nOptions:\n  -0       reset tabs\n  -8       set tabs to standard interval\n  -a       Assembler, IBM S/370, first format\n  -a2      Assembler, IBM S/370, second format\n  -c       COBOL, normal format\n  -c2      COBOL compact format\n  -c3      COBOL compact format extended\n  -d       debug (show ruler with expected/actual tab positions)\n  -f       FORTRAN\n  -n       no-op (do not modify terminal settings)\n  -p       PL/I\n  -s       SNOBOL\n  -u       UNIVAC 1100 Assembler\n  -T name  use terminal type 'name'\n  -V       print version\n\nA tabstop-list is an ordered list of column numbers, e.g., 1,11,21\nor 1,+10,+10 which is the same.\n";
    io::eprint(format!("Usage: {progname} [options] [tabstop-list]\n{msg}"));
    sys::exit(1)
}

fn skip_csi(value: &[u8]) -> &[u8] {
    if value.first() == Some(&0x9b) {
        &value[1..]
    } else if value.starts_with(b"\x1b[") {
        &value[2..]
    } else {
        value
    }
}

/// Com o `ct` ANSI (`\E[3g`) não precisa ir à margem esquerda antes.
fn ansi_clear_tabs(term: &Term) -> bool {
    term.tt.sv("clear_all_tabs").is_some_and(|c| skip_csi(c) == b"3g")
}

fn skip_list(value: &[u8]) -> usize {
    let mut i = 0;
    while i < value.len() && (value[i].is_ascii_digit() || c_isspace(value[i]) || b"+,".contains(&value[i])) {
        i += 1;
    }
    i
}

fn trimmed_tab_list(source: &[u8]) -> Vec<u8> {
    let mut result: Vec<u8> = Vec::new();
    let mut last: u8 = 0;
    for &ch0 in source {
        let mut ch = ch0;
        if c_isspace(ch) {
            if last == 0 {
                continue;
            } else if last.is_ascii_digit() || last == b',' {
                ch = b',';
            }
        } else if ch == b',' {
        } else {
            if last == b',' {
                result.push(last);
            }
            result.push(ch);
        }
        last = ch;
    }
    result
}

fn comma_is_needed(source: Option<&[u8]>) -> bool {
    match source {
        Some(s) if !s.is_empty() => s[s.len() - 1] != b',',
        _ => false,
    }
}

/// `add_to_tab_list`: acrescenta (separando com vírgula) e devolve se `append` mudou.
fn add_to_tab_list(append: &mut Option<Vec<u8>>, value: &[u8]) {
    let copied = trimmed_tab_list(value);
    if !copied.is_empty() {
        let comma: &[u8] = if copied[0] == b',' || !comma_is_needed(append.as_deref()) { b"" } else { b"," };
        let mut result = append.take().unwrap_or_default();
        result.extend_from_slice(comma);
        result.extend_from_slice(&copied);
        *append = Some(result);
    }
}

impl Tabs {
    fn putch(&self, c: u8) {
        out(&[c]);
    }

    fn tput_cap(&self, s: &[u8]) {
        tputs(Some(&self.term.tt), s, 1, false, &mut StdoutSink);
    }

    fn do_tabs(&self, tab_list: &[i32]) {
        let mut last = 1;
        let mut first = true;
        let set_tab = self.term.tt.sv("set_tab").map(<[u8]>::to_vec).unwrap_or_default();
        for &stop in tab_list {
            if stop <= 0 {
                break;
            }
            if first {
                first = false;
                self.putch(b'\r');
            }
            if last < stop {
                loop {
                    let l = last;
                    last += 1;
                    if l >= stop {
                        break;
                    }
                    if last > self.max_cols {
                        break;
                    }
                    self.putch(b' ');
                }
            }
            if stop <= self.max_cols {
                self.tput_cap(&set_tab);
                last = stop;
            } else {
                break;
            }
        }
        self.putch(b'\r');
    }

    /// `decode_tabs`: `None` quando as paradas não estão em ordem crescente.
    fn decode_tabs(&self, tab_list: &[u8], margin: i32) -> Option<Vec<i32>> {
        let mut result: Vec<i32> = Vec::new();
        let mut n = 0usize;
        let mut value = 0i32;
        let mut prior = 0i32;
        let margin = margin.max(0);
        for &ch in tab_list {
            if ch.is_ascii_digit() {
                value = value.wrapping_mul(10).wrapping_add(i32::from(ch - b'0'));
                if value > self.max_cols {
                    value = self.max_cols;
                }
            } else if ch == b',' {
                let v = value + prior + margin;
                if result.len() <= n {
                    result.resize(n + 1, 0);
                }
                result[n] = v;
                if n > 0 && result[n] <= result[n - 1] {
                    io::eprint(format!(
                        "{}: tab-stops are not in increasing order: {} {}\n",
                        self.progname, value, result[n - 1]
                    ));
                    return None;
                }
                n += 1;
                value = 0;
                prior = 0;
            } else if ch == b'+' && n > 0 {
                prior = result[n - 1];
            }
        }
        // Um só valor é uma opção como `-8`: o passo entre as paradas.
        if n == 0 && value > 0 {
            let step = value;
            value = 1;
            while (n as i32) < self.max_cols - 1 {
                if result.len() <= n {
                    result.resize(n + 1, 0);
                }
                result[n] = value + margin;
                n += 1;
                value += step;
            }
        }
        if result.len() <= n {
            result.resize(n + 1, 0);
        }
        result[n] = value + prior + margin;
        n += 1;
        result.truncate(n);
        result.push(0);
        Some(result)
    }

    fn print_ruler(&self, tab_list: &[i32], new_line: &[u8]) {
        let mut n = 0;
        while n < self.max_cols {
            let ch = 1 + n / 10;
            let mark = if ch < 10 { (ch as u8) + b'0' } else { (ch as u8) + b'A' - 10 };
            let buffer = format!("----+----{}", mark as char);
            let take = if self.max_cols - n > 10 { 10 } else { (self.max_cols - n) as usize };
            out(&buffer.as_bytes()[..take.min(buffer.len())]);
            n += 10;
        }
        out(new_line);
        let mut last = 0;
        let mut idx = 0;
        while idx < tab_list.len() && tab_list[idx] > 0 && last < self.max_cols {
            let stop = tab_list[idx];
            loop {
                last += 1;
                if last >= stop {
                    break;
                }
                if last <= self.max_cols {
                    out(b"-");
                } else {
                    break;
                }
            }
            if last <= self.max_cols {
                out(b"*");
                last = stop;
            } else {
                break;
            }
            idx += 1;
        }
        loop {
            last += 1;
            if last > self.max_cols {
                break;
            }
            out(b"-");
        }
        out(new_line);
    }

    fn write_tabs(&self, tab_list: &[i32], new_line: &[u8]) {
        let mut idx = 0;
        let mut stop;
        loop {
            stop = tab_list.get(idx).copied().unwrap_or(0);
            idx += 1;
            if !(stop > 0 && stop <= self.max_cols) {
                break;
            }
            out(if stop == 1 { b"*" } else { b"\t*" });
        }
        if stop < self.max_cols {
            out(b"\t+");
        }
        out(new_line);
    }

    /// `do_set_margin`: devolve se o terminal aceita margem.
    fn do_set_margin(&mut self, margin: i32, no_op: bool) -> bool {
        let tt = &self.term.tt;
        let clear_margins = tt.sv("clear_margins").map(<[u8]>::to_vec);
        let set_left = tt.sv("set_left_margin").map(<[u8]>::to_vec);
        let column_address = tt.sv("column_address").map(<[u8]>::to_vec);
        let parm_right = tt.sv("parm_right_cursor").map(<[u8]>::to_vec);
        let set_left_parm = tt.sv("set_left_margin_parm").map(<[u8]>::to_vec);
        let set_right_parm = tt.sv("set_right_margin_parm").map(<[u8]>::to_vec);
        let set_lr = tt.sv("set_lr_margin").map(<[u8]>::to_vec);
        let mut margin = margin;
        if margin == 0 {
            // 0 só serve pra desfazer a margem: sem `clear_margins` nada mais se tenta.
            if let Some(c) = clear_margins {
                if !no_op {
                    self.tput_cap(&c);
                }
                return true;
            }
            return false;
        }
        let neg = margin < 0;
        margin -= 1;
        if neg {
            return true;
        }
        if let Some(sl) = set_left {
            if !no_op {
                if let Some(ca) = column_address {
                    let r = tiparm(&self.term.tt, &mut self.state, 1, &ca, &[i64::from(margin)]);
                    if let Some(r) = r {
                        self.tput_cap(&r);
                    }
                } else if margin >= 1 {
                    if let Some(pr) = parm_right {
                        let r = tiparm(&self.term.tt, &mut self.state, 1, &pr, &[i64::from(margin)]);
                        if let Some(r) = r {
                            self.tput_cap(&r);
                        }
                    } else {
                        let mut m = margin;
                        while m > 0 {
                            self.putch(b' ');
                            m -= 1;
                        }
                    }
                }
                self.tput_cap(&sl);
            }
            return true;
        }
        self.after_margin_set(no_op, margin, set_left_parm, set_right_parm, set_lr)
    }

    fn after_margin_set(&mut self, no_op: bool, margin: i32, set_left_parm: Option<Vec<u8>>, set_right_parm: Option<Vec<u8>>, set_lr: Option<Vec<u8>>) -> bool {
        if let Some(sl) = set_left_parm {
            if !no_op {
                if set_right_parm.is_some() {
                    if let Some(r) = tiparm(&self.term.tt, &mut self.state, 1, &sl, &[i64::from(margin)]) {
                        self.tput_cap(&r);
                    }
                } else if let Some(r) = tiparm(&self.term.tt, &mut self.state, 2, &sl, &[i64::from(margin), i64::from(self.max_cols)]) {
                    self.tput_cap(&r);
                }
            }
            return true;
        }
        if let Some(lr) = set_lr {
            if !no_op {
                if let Some(r) = tiparm(&self.term.tt, &mut self.state, 2, &lr, &[i64::from(margin), i64::from(self.max_cols)]) {
                    self.tput_cap(&r);
                }
            }
            return true;
        }
        false
    }
}

fn legal_tab_list(progname: &str, tab_list: Option<&[u8]>) -> bool {
    match tab_list {
        Some(t) if !t.is_empty() => {
            if comma_is_needed(Some(t)) {
                for &ch in t {
                    if !(ch.is_ascii_digit() || ch == b',' || ch == b'+') {
                        let mut msg = format!("{progname}: unexpected character found '").into_bytes();
                        msg.push(ch);
                        msg.extend_from_slice(b"'\n");
                        io::eprint(msg);
                        return false;
                    }
                }
                true
            } else {
                io::eprint(format!("{progname}: trailing comma found '{}'\n", io::lossy(t)));
                false
            }
        }
        _ => true,
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let progname = io::lossy(rootname(&argv[0]));
    let mut term_name: Option<Vec<u8>> = Some(sys::getenv("TERM").unwrap_or_else(|| b"ansi+tabs".to_vec()));
    let mut debug = false;
    let mut no_op = false;
    let mut append: Option<Vec<u8>> = None;
    let mut tab_list: Option<Vec<u8>> = None;
    // `tab_list == append` no original compara ponteiros.
    let mut list_is_append = false;
    let mut margin: i32 = -1;
    let argc = argv.len();
    let mut n = 1usize;
    while n < argc {
        let arg = argv[n].clone();
        let at = |i: usize| arg.get(i).copied().unwrap_or(0);
        match at(0) {
            b'-' => {
                let mut option = 0usize;
                loop {
                    option += 1;
                    let ch = at(option);
                    if ch == 0 {
                        break;
                    }
                    match ch {
                        b'a' => {
                            option += 1;
                            if at(option) == b'2' {
                                tab_list = Some(b"1,10,16,40,72".to_vec());
                            } else {
                                tab_list = Some(b"1,10,16,36,72".to_vec());
                                option -= 1;
                            }
                            list_is_append = false;
                        }
                        b'c' => {
                            option += 1;
                            match at(option) {
                                b'2' => tab_list = Some(b"1,6,10,14,49".to_vec()),
                                b'3' => tab_list = Some(b"1,6,10,14,18,22,26,30,34,38,42,46,50,54,58,62,67".to_vec()),
                                _ => {
                                    tab_list = Some(b"1,8,12,16,20,55".to_vec());
                                    option -= 1;
                                }
                            }
                            list_is_append = false;
                        }
                        b'd' => debug = true,
                        b'f' => {
                            tab_list = Some(b"1,7,11,15,19,23".to_vec());
                            list_is_append = false;
                        }
                        b'n' => no_op = true,
                        b'p' => {
                            tab_list = Some(b"1,5,9,13,17,21,25,29,33,37,41,45,49,53,57,61".to_vec());
                            list_is_append = false;
                        }
                        b's' => {
                            tab_list = Some(b"1,10,55".to_vec());
                            list_is_append = false;
                        }
                        b'u' => {
                            tab_list = Some(b"1,12,20,44".to_vec());
                            list_is_append = false;
                        }
                        b'T' => {
                            n += 1;
                            option += 1;
                            if at(option) != 0 {
                                term_name = Some(arg[option..].to_vec());
                            } else {
                                term_name = argv.get(n).cloned();
                                option -= 1;
                            }
                            // `option += strlen(option) - 1`
                            let rest = arg.len().saturating_sub(option);
                            option += rest.saturating_sub(1);
                            continue;
                        }
                        b'V' => {
                            let _ = writeln!(io::stdout(), "{VERSION}");
                            return 0;
                        }
                        _ => {
                            if ch.is_ascii_digit() {
                                let len = skip_list(&arg[option..]);
                                tab_list = Some(arg[option..option + len].to_vec());
                                list_is_append = false;
                                option = option + len - 1;
                            } else {
                                usage(&progname);
                            }
                        }
                    }
                }
            }
            b'+' => {
                let ch = at(1);
                if ch != 0 {
                    if ch == b'm' {
                        let mut digits = 0;
                        let mut number = 0i32;
                        let mut option = 1usize;
                        loop {
                            option += 1;
                            let c = at(option);
                            if c == 0 {
                                break;
                            }
                            if c.is_ascii_digit() {
                                digits += 1;
                                number = number.wrapping_mul(10).wrapping_add(i32::from(c - b'0'));
                            } else {
                                usage(&progname);
                            }
                        }
                        if digits == 0 {
                            number = 10;
                        }
                        margin = number;
                    } else {
                        add_to_tab_list(&mut append, &arg);
                        tab_list = append.clone();
                        list_is_append = true;
                    }
                }
            }
            _ => {
                if append.is_some() && !(list_is_append && tab_list.is_some()) {
                    // uma das opções predefinidas foi usada
                    append = None;
                }
                add_to_tab_list(&mut append, &arg);
                tab_list = append.clone();
                list_is_append = true;
            }
        }
        n += 1;
    }

    let fd = save_tty_settings(&progname, false);
    let term = match setupterm(term_name.as_deref(), fd, SetupOpts::default()) {
        Ok(t) => t,
        Err(f) => {
            io::eprint(f.message);
            return 1;
        }
    };
    let columns = term.tt.n("columns");
    let mut max_cols = if columns > 0 { columns } else { 80 };
    if margin > 0 {
        max_cols -= margin;
    }
    let mut tabs = Tabs { progname: progname.clone(), max_cols, term, state: ParmState::default() };
    let tname = term_name.as_deref().map(io::lossy).unwrap_or_else(|| "(null)".to_string());
    let mut rc = 1;
    if !tabs.term.tt.s("clear_all_tabs").valid() {
        io::eprint(format!("{progname}: terminal type '{tname}' cannot reset tabs\n"));
    } else if !tabs.term.tt.s("set_tab").valid() {
        io::eprint(format!("{progname}: terminal type '{tname}' cannot set tabs\n"));
    } else if legal_tab_list(&progname, tab_list.as_deref()) {
        if tab_list.is_none() {
            add_to_tab_list(&mut append, b"8");
            tab_list = append.clone();
        }
        let tab_list_bytes = tab_list.clone().unwrap_or_default();
        let mut new_line: &[u8] = b"\n";
        if !no_op {
            // Com o stdout num terminal o original desliga `ocrnl` pra aceitar o `\r`.
            if isatty(Fd::STDOUT) {
                new_line = b"\r\n";
            }
            if !ansi_clear_tabs(&tabs.term) {
                tabs.putch(b'\r');
            }
            if let Some(c) = tabs.term.tt.sv("clear_all_tabs") {
                let c = c.to_vec();
                tabs.tput_cap(&c);
            }
        }
        if margin >= 0 {
            tabs.putch(b'\r');
            if margin > 0 && tabs.do_set_margin(0, no_op) {
                tabs.putch(b'\r');
            }
            if tabs.do_set_margin(margin, no_op) {
                margin = -1;
            }
        }
        let list = tabs.decode_tabs(&tab_list_bytes, margin);
        match list {
            Some(list) => {
                if !no_op {
                    tabs.do_tabs(&list);
                }
                if debug {
                    out(b"tabs ");
                    out(&tab_list_bytes);
                    out(new_line);
                    tabs.print_ruler(&list, new_line);
                    tabs.write_tabs(&list, new_line);
                }
            }
            None => {
                if debug {
                    out(b"tabs ");
                    out(&tab_list_bytes);
                    out(new_line);
                }
            }
        }
        rc = 0;
    }
    rc
}
