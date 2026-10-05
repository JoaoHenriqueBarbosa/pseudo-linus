//! A parte do `reset_cmd.c` que não mexe nos modos do terminal: o envio das cadeias de
//! inicialização (`init`/`reset`) e os avisos de caracteres de controle. O `sysabi` não expõe o
//! termios, então os modos (`ECHO`, `ICRNL`...) não são lidos nem gravados aqui.

use sysabi::{Errno, Fd, sys};

use super::terminfo::Term;
use super::tparm::{ParmState, Sink, tiparm, tputs};
use super::{FdSink, err_system};
use crate::util::io;

/// O estado de `reset_cmd.c`: pra onde as cadeias vão (`my_file`) e se é `reset` ou `init`.
#[derive(Debug)]
pub struct Reset<'a> {
    pub term: &'a Term,
    pub out: FdSink,
    pub use_reset: bool,
    pub use_init: bool,
    pub progname: String,
    pub state: ParmState,
    /// O `columns` depois do `set_window_size`.
    pub columns: i32,
}

impl<'a> Reset<'a> {
    pub fn new(term: &'a Term, out_fd: Fd, use_reset: bool, use_init: bool, progname: &str) -> Reset<'a> {
        let columns = term.tt.n("columns");
        Reset { term, out: FdSink::new(out_fd), use_reset, use_init, progname: progname.to_string(), state: ParmState::default(), columns }
    }

    /// `failed()`: mensagem com o erro, uma linha em branco no `my_file` e a saída `4 + errno`.
    fn failed(&mut self, msg: &[u8], errno: Errno) -> ! {
        io::eprint(format!("{}: {}: {}\n", self.progname, io::lossy(msg), errno.message()));
        self.out.write_all(b"\n");
        self.out.finish();
        sys::exit(err_system(errno.0));
    }

    fn cat_file(&mut self, file: Option<&[u8]>) -> bool {
        let Some(file) = file else { return false };
        let data = match sys::read_file(file) {
            Ok(d) => d,
            Err(e) => {
                // `safe_fopen` recusa dispositivos e diretórios; a mensagem é a do errno.
                let e = if e == Errno::EISDIR { Errno::ENOENT } else { e };
                self.failed(file, e)
            }
        };
        self.out.write_all(&data);
        !data.is_empty()
    }

    fn sent_string<S: AsRef<[u8]>>(&mut self, s: Option<S>) -> bool {
        match s {
            Some(s) => {
                tputs(Some(&self.term.tt), s.as_ref(), 0, false, &mut self.out);
                true
            }
            None => false,
        }
    }

    fn out_char(&mut self, c: u8) {
        self.out.put(c);
    }

    fn move_to_left_margin(&mut self) -> bool {
        match self.term.tt.sv("carriage_return") {
            Some(cr) => {
                let cr = cr.to_vec();
                self.sent_string(Some(&cr));
            }
            None => self.out_char(b'\r'),
        }
        true
    }

    /// `reset_tabstops`: usa `ct`, `st` e `it`, antes de `if`/`is`, pra poder recuperar de erros.
    fn reset_tabstops(&mut self, wide: i32) -> bool {
        let mut init_tabs = self.term.tt.n("init_tabs");
        let set_tab = self.term.tt.sv("set_tab").map(<[u8]>::to_vec);
        let clear_all = self.term.tt.sv("clear_all_tabs").map(<[u8]>::to_vec);
        if init_tabs != 8
            && init_tabs >= 0
            && let (Some(set_tab), Some(clear_all)) = (set_tab, clear_all)
        {
            self.move_to_left_margin();
            self.sent_string(Some(&clear_all));
            if init_tabs > 1 {
                if init_tabs > wide {
                    init_tabs = wide;
                }
                let mut c = init_tabs;
                while c < wide {
                    self.out.write_all(&vec![b' '; init_tabs as usize]);
                    self.sent_string(Some(&set_tab));
                    c += init_tabs;
                }
                self.move_to_left_margin();
            }
            return true;
        }
        false
    }

    /// `send_init_strings`: as cadeias de `init`/`reset`; devolve se mandou algo.
    pub fn send_init_strings(&mut self) -> bool {
        let term: &'a Term = self.term;
        let tt = &term.tt;
        let mut need_flush = false;
        if !(self.use_reset || self.use_init) {
            return false;
        }
        let pick = |reset: bool, r: &str, i: &str| -> Option<Vec<u8>> {
            if reset && tt.s(r).valid() { tt.sv(r).map(<[u8]>::to_vec) } else { tt.sv(i).map(<[u8]>::to_vec) }
        };
        let columns = self.columns;
        let (r1, r2, r3) = (
            pick(self.use_reset, "reset_1string", "init_1string"),
            pick(self.use_reset, "reset_2string", "init_2string"),
            pick(self.use_reset, "reset_3string", "init_3string"),
        );
        let file = if self.use_reset && tt.s("reset_file").valid() { tt.sv("reset_file") } else { tt.sv("init_file") }.map(<[u8]>::to_vec);
        let clear_margins = tt.sv("clear_margins").map(<[u8]>::to_vec);
        let set_lr_margin = tt.sv("set_lr_margin").map(<[u8]>::to_vec);
        let set_left_parm = tt.sv("set_left_margin_parm").map(<[u8]>::to_vec);
        let set_right_parm = tt.sv("set_right_margin_parm").map(<[u8]>::to_vec);
        let set_left = tt.sv("set_left_margin").map(<[u8]>::to_vec);
        let set_right = tt.sv("set_right_margin").map(<[u8]>::to_vec);
        let parm_right = tt.sv("parm_right_cursor").map(<[u8]>::to_vec);

        need_flush |= self.sent_string(r1.as_deref());
        need_flush |= self.sent_string(r2.as_deref());
        if clear_margins.is_some() {
            need_flush |= self.sent_string(clear_margins.as_deref());
        } else if let Some(lr) = set_lr_margin {
            let s = tiparm(&self.term.tt, &mut self.state, 2, &lr, &[0, i64::from(columns) - 1]);
            need_flush |= self.sent_string(s.as_deref());
        } else if let (Some(l), Some(r)) = (&set_left_parm, &set_right_parm) {
            let s = tiparm(&self.term.tt, &mut self.state, 1, l, &[0]);
            need_flush |= self.sent_string(s.as_deref());
            let s = tiparm(&self.term.tt, &mut self.state, 1, r, &[i64::from(columns) - 1]);
            need_flush |= self.sent_string(s.as_deref());
        } else if let (Some(l), Some(r)) = (&set_left, &set_right) {
            need_flush |= self.move_to_left_margin();
            need_flush |= self.sent_string(Some(l));
            if let Some(p) = &parm_right {
                let s = tiparm(&self.term.tt, &mut self.state, 1, p, &[i64::from(columns) - 1]);
                need_flush |= self.sent_string(s.as_deref());
            } else {
                for _ in 0..(columns - 1).max(0) {
                    self.out_char(b' ');
                    need_flush = true;
                }
            }
            need_flush |= self.sent_string(Some(r));
            need_flush |= self.move_to_left_margin();
        }
        need_flush |= self.reset_tabstops(columns);
        need_flush |= self.cat_file(file.as_deref());
        need_flush |= self.sent_string(r3.as_deref());
        need_flush
    }

    pub fn flush(&mut self) {
        self.out.finish();
    }
}
