//! Pseudoterminais Unix 98 (`drivers/tty/pty.c`), a disciplina de linha N_TTY (`drivers/tty/n_tty.c`)
//! e o terminal de controle (`drivers/tty/tty_jobctrl.c`), como no Linux 6.12.
//!
//! - `/dev/ptmx` (5,2) abre um mestre novo e cria `/dev/pts/N` (136,N), dono quem abriu, grupo `tty`
//!   (gid 5), modo 0620, travado até o `TIOCSPTLCK` com 0 (o `unlockpt`). Abrir o escravo travado, ou
//!   sem mestre, dá EIO.
//! - O par tem um estado só de termios e winsize: as ioctls de modo no mestre agem no escravo, como o
//!   `tty_pair_get_tty` do kernel.
//! - O que o mestre escreve passa pela disciplina de linha e vira entrada do escravo; o que o escravo
//!   escreve passa pelo OPOST e vira saída que o mestre lê, junto com o eco.
//! - Fechar o último mestre apaga o nó e desliga o escravo (hangup): leitura dá EOF, escrita dá EIO, e o
//!   líder da sessão recebe SIGHUP e SIGCONT. Fechar o último escravo faz a leitura do mestre dar EIO
//!   depois de esvaziar (`TTY_OTHER_CLOSED`).
//! - O terminal de controle é da sessão: o pty guarda a sessão e o grupo em primeiro plano, e quem
//!   procura o terminal de controle de um processo procura o pty da sessão dele.

use std::collections::VecDeque;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use sysabi::termios::*;
use sysabi::{AtFlags, Errno, Pid, PollEvents, SetAttrWhen, Signal, Termios, Winsize};
use vfs::Start;

use crate::park::{Parker, Wake, WaitList, locked};
use crate::pipe::Try;
use crate::sandbox::SbInner;

/// `/dev/ptmx` (TTYAUX_MAJOR, 2).
pub(crate) const DEV_PTMX: (u32, u32) = (5, 2);
/// Major dos escravos Unix 98 (`UNIX98_PTY_SLAVE_MAJOR`).
pub(crate) const PTS_MAJOR: u32 = 136;
/// Grupo `tty` do Debian (o `gid=5` da montagem do devpts no contêiner).
pub(crate) const TTY_GID: u32 = 5;
/// Modo dos nós `/dev/pts/N` (`mode=620`).
pub(crate) const PTS_MODE: u32 = 0o620;
/// Máximo de ptys (`NR_UNIX98_PTY_DEFAULT`).
pub(crate) const PTY_MAX: u32 = 4096;
/// Buffer de leitura da N_TTY.
pub(crate) const N_TTY_BUF_SIZE: usize = 4096;
/// Quanto de saída do escravo fica pendente antes de a escrita esperar o mestre ler (o limite do
/// buffer de flip do tty).
pub(crate) const PTY_OUT_LIMIT: usize = 65536;

const SIGHUP: Signal = Signal(1);
const SIGINT: Signal = Signal(2);
const SIGQUIT: Signal = Signal(3);
const SIGCONT: Signal = Signal(18);
const SIGTSTP: Signal = Signal(20);
const SIGWINCH: Signal = Signal(28);

fn iscntrl(c: u8) -> bool {
    c < 0x20 || c == 0x7f
}

fn is_continuation(c: u8) -> bool {
    c & 0xc0 == 0x80
}

/// O que o `set_termios` do tty guarda de fato: o pty força CS8 e CREAD e tira PARENB
/// (`pty_set_termios`), e as velocidades do `termios2` espelham os códigos `Bnnn` quando não são
/// `BOTHER` (`tty_termios_baud_rate` e `tty_termios_input_baud_rate`).
pub(crate) fn sanitize(mut t: Termios) -> Termios {
    t.c_cflag &= !(CSIZE | PARENB);
    t.c_cflag |= CS8 | CREAD;
    let ob = t.c_cflag & CBAUD;
    if ob != BOTHER
        && let Some(b) = baud_of(ob)
    {
        t.c_ospeed = b;
    }
    let ib = (t.c_cflag >> IBSHIFT) & CBAUD;
    if ib == B0 {
        t.c_ispeed = t.c_ospeed;
    } else if ib != BOTHER
        && let Some(b) = baud_of(ib)
    {
        t.c_ispeed = b;
    }
    t
}

// ------------------------------------------------------------------------------------------------
// Disciplina de linha
// ------------------------------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum EraseKind {
    Erase,
    Werase,
    Kill,
}

/// A N_TTY de um tty: entrada (canônica ou não), eco e processamento de saída.
#[derive(Debug, Clone)]
pub(crate) struct Ldisc {
    pub(crate) termios: Termios,
    /// Linhas completas do modo canônico. A terminada por EOF não leva o delimitador (e vazia dá a
    /// leitura de 0 bytes).
    lines: VecDeque<Vec<u8>>,
    /// Linha em edição.
    line: Vec<u8>,
    /// Entrada do modo não canônico.
    raw: VecDeque<u8>,
    /// Saída pendente pro mestre: o que o escravo escreveu e o eco.
    pub(crate) out: VecDeque<u8>,
    /// Coluna do cursor (`ldata->column`), pra apagar tabulação.
    column: u32,
    /// Coluna onde a linha em edição começou.
    canon_column: u32,
    /// O próximo caractere é literal (VLNEXT).
    lnext: bool,
}

impl Default for Ldisc {
    fn default() -> Self {
        Ldisc::new(Termios::default())
    }
}

impl Ldisc {
    pub(crate) fn new(termios: Termios) -> Ldisc {
        Ldisc {
            termios,
            lines: VecDeque::new(),
            line: Vec::new(),
            raw: VecDeque::new(),
            out: VecDeque::new(),
            column: 0,
            canon_column: 0,
            lnext: false,
        }
    }

    fn lflag(&self, f: u32) -> bool {
        self.termios.c_lflag & f != 0
    }

    fn iflag(&self, f: u32) -> bool {
        self.termios.c_iflag & f != 0
    }

    fn oflag(&self, f: u32) -> bool {
        self.termios.c_oflag & f != 0
    }

    /// Caractere especial ligado (0 é `_POSIX_VDISABLE`).
    fn is_cc(&self, c: u8, idx: usize) -> bool {
        let v = self.termios.c_cc[idx];
        v != 0 && v == c
    }

    pub(crate) fn canon(&self) -> bool {
        self.lflag(ICANON)
    }

    pub(crate) fn input_len(&self) -> usize {
        self.lines.iter().map(Vec::len).sum::<usize>() + self.line.len() + self.raw.len()
    }

    /// Espaço pra entrada no modo não canônico.
    pub(crate) fn input_room(&self) -> usize {
        (N_TTY_BUF_SIZE - 1).saturating_sub(self.input_len())
    }

    /// Há o que ler (uma linha completa no modo canônico, um byte no outro).
    pub(crate) fn has_input(&self) -> bool {
        if self.canon() { !self.lines.is_empty() } else { !self.raw.is_empty() }
    }

    pub(crate) fn raw_len(&self) -> usize {
        self.raw.len()
    }

    /// Entrada vinda do mestre (`n_tty_receive_buf`). Devolve os sinais de ISIG gerados, na ordem.
    pub(crate) fn receive(&mut self, data: &[u8]) -> Vec<Signal> {
        let mut sigs = Vec::new();
        for &b in data {
            if let Some(s) = self.receive_char(b) {
                sigs.push(s);
            }
        }
        sigs
    }

    fn receive_char(&mut self, b: u8) -> Option<Signal> {
        let mut c = if self.iflag(ISTRIP) { b & 0x7f } else { b };
        if self.lnext {
            self.lnext = false;
            if self.lflag(ECHO) {
                self.mark_canon_col();
                self.echo_char(c);
            }
            self.put_char(c);
            return None;
        }
        if self.lflag(ISIG) {
            let sig = if self.is_cc(c, VINTR) {
                Some(SIGINT)
            } else if self.is_cc(c, VQUIT) {
                Some(SIGQUIT)
            } else if self.is_cc(c, VSUSP) {
                Some(SIGTSTP)
            } else {
                None
            };
            if let Some(s) = sig {
                // `isig`: sem NOFLSH, descarta a entrada e a saída pendente do mestre.
                if !self.lflag(NOFLSH) {
                    self.flush_input();
                    self.out.clear();
                }
                if self.lflag(ECHO) {
                    self.echo_char(c);
                }
                return Some(s);
            }
        }
        // `\n` que chega pelo caminho especial ecoa cru; o do caminho comum ecoa como `^J`.
        let mut special_nl = false;
        if c == b'\r' {
            if self.iflag(IGNCR) {
                return None;
            }
            if self.iflag(ICRNL) {
                c = b'\n';
                special_nl = true;
            }
        } else if c == b'\n' && self.iflag(INLCR) {
            c = b'\r';
        }
        if self.canon() {
            let ext = self.lflag(IEXTEN);
            if self.is_cc(c, VERASE) || self.is_cc(c, VKILL) || (ext && self.is_cc(c, VWERASE)) {
                self.eraser(c);
                return None;
            }
            if ext && self.is_cc(c, VLNEXT) {
                self.lnext = true;
                if self.lflag(ECHO) && self.lflag(ECHOCTL) {
                    self.mark_canon_col();
                    self.put_output(b'^');
                    self.put_output(0x08);
                }
                return None;
            }
            if ext && self.lflag(ECHO) && self.is_cc(c, VREPRINT) {
                self.echo_char(c);
                self.put_output(b'\n');
                self.canon_column = self.column;
                let line = self.line.clone();
                for b in line {
                    self.echo_char(b);
                }
                return None;
            }
            if c == b'\n' {
                if self.lflag(ECHO) || self.lflag(ECHONL) {
                    self.put_output(b'\n');
                }
                self.line.push(c);
                self.finish_line();
                return None;
            }
            if self.is_cc(c, VEOF) {
                self.finish_line();
                return None;
            }
            if self.is_cc(c, VEOL) || (ext && self.is_cc(c, VEOL2)) {
                if self.lflag(ECHO) {
                    self.mark_canon_col();
                    self.echo_char(c);
                }
                self.line.push(c);
                self.finish_line();
                return None;
            }
        }
        if self.lflag(ECHO) {
            if c == b'\n' && special_nl {
                self.put_output(b'\n');
            } else {
                self.mark_canon_col();
                self.echo_char(c);
            }
        }
        self.put_char(c);
        None
    }

    fn mark_canon_col(&mut self) {
        if self.line.is_empty() {
            self.canon_column = self.column;
        }
    }

    /// Guarda um byte de entrada; cheio, o byte se perde (como no `n_tty`).
    fn put_char(&mut self, c: u8) {
        if self.canon() {
            if self.input_len() < N_TTY_BUF_SIZE - 1 {
                self.line.push(c);
            }
        } else if self.raw.len() < N_TTY_BUF_SIZE - 1 {
            self.raw.push_back(c);
        }
    }

    fn finish_line(&mut self) {
        let l = std::mem::take(&mut self.line);
        self.lines.push_back(l);
    }

    /// `echo_char`: com ECHOCTL, controle (menos tab) vira `^X`.
    fn echo_char(&mut self, c: u8) {
        if self.lflag(ECHOCTL) && iscntrl(c) && c != b'\t' {
            self.out.push_back(b'^');
            self.out.push_back(c ^ 0x40);
            self.column += 2;
        } else {
            self.put_output(c);
        }
    }

    /// Um byte de saída com o processamento do OPOST (`do_output_char`).
    pub(crate) fn put_output(&mut self, c: u8) {
        if !self.oflag(OPOST) {
            self.out.push_back(c);
            return;
        }
        match c {
            b'\n' => {
                if self.oflag(ONLRET) {
                    self.column = 0;
                }
                if self.oflag(ONLCR) {
                    self.column = 0;
                    self.canon_column = 0;
                    self.out.extend(*b"\r\n");
                    return;
                }
                self.canon_column = self.column;
                self.out.push_back(c);
            }
            b'\r' => {
                if self.oflag(ONOCR) && self.column == 0 {
                    return;
                }
                let mut c = c;
                if self.oflag(OCRNL) {
                    c = b'\n';
                    if self.oflag(ONLRET) {
                        self.column = 0;
                        self.canon_column = 0;
                    }
                } else {
                    self.column = 0;
                    self.canon_column = 0;
                }
                self.out.push_back(c);
            }
            b'\t' => {
                let spaces = 8 - (self.column % 8);
                self.column += spaces;
                if self.termios.c_oflag & TABDLY == XTABS {
                    self.out.extend(std::iter::repeat_n(b' ', spaces as usize));
                } else {
                    self.out.push_back(c);
                }
            }
            0x08 => {
                self.column = self.column.saturating_sub(1);
                self.out.push_back(c);
            }
            _ => {
                let mut c = c;
                if !iscntrl(c) {
                    if self.oflag(OLCUC) {
                        c = c.to_ascii_uppercase();
                    }
                    if !(self.iflag(IUTF8) && is_continuation(c)) {
                        self.column += 1;
                    }
                }
                self.out.push_back(c);
            }
        }
    }

    /// Coluna em que o fim da linha em edição está, pelo eco (pra apagar tabulação).
    fn line_column(&self) -> u32 {
        let mut col = self.canon_column;
        for &b in &self.line {
            if b == b'\t' {
                col = (col | 7) + 1;
            } else if iscntrl(b) {
                if self.lflag(ECHOCTL) {
                    col += 2;
                }
            } else if !(self.iflag(IUTF8) && is_continuation(b)) {
                col += 1;
            }
        }
        col
    }

    fn erase_cell(&mut self) {
        self.out.extend([0x08, b' ', 0x08]);
        self.column = self.column.saturating_sub(1);
    }

    /// ERASE, WERASE e KILL (`eraser`).
    fn eraser(&mut self, c: u8) {
        if self.line.is_empty() {
            return;
        }
        let kind = if self.is_cc(c, VERASE) {
            EraseKind::Erase
        } else if self.is_cc(c, VWERASE) {
            EraseKind::Werase
        } else {
            EraseKind::Kill
        };
        let echo = self.lflag(ECHO);
        if kind == EraseKind::Kill {
            if !echo {
                self.line.clear();
                return;
            }
            if !self.lflag(ECHOK) || !self.lflag(ECHOKE) || !self.lflag(ECHOE) {
                self.line.clear();
                self.echo_char(c);
                if self.lflag(ECHOK) {
                    self.put_output(b'\n');
                }
                return;
            }
        }
        let mut seen_alnums = 0;
        while !self.line.is_empty() {
            // O caractere inteiro: com IUTF8, a sequência de continuação vai junto.
            let mut start = self.line.len() - 1;
            if self.iflag(IUTF8) {
                while start > 0 && is_continuation(self.line[start]) {
                    start -= 1;
                }
            }
            let ch = self.line[start];
            if kind == EraseKind::Werase {
                if ch.is_ascii_alphanumeric() || ch == b'_' {
                    seen_alnums += 1;
                } else if seen_alnums > 0 {
                    break;
                }
            }
            self.line.truncate(start);
            if echo {
                if kind == EraseKind::Erase && !self.lflag(ECHOE) {
                    let erase = self.termios.c_cc[VERASE];
                    self.echo_char(erase);
                } else if ch == b'\t' {
                    let col = self.line_column();
                    let n = self.column.saturating_sub(col);
                    self.out.extend(std::iter::repeat_n(0x08, n as usize));
                    self.column -= n;
                } else {
                    if iscntrl(ch) && self.lflag(ECHOCTL) {
                        self.erase_cell();
                    }
                    if !iscntrl(ch) || self.lflag(ECHOCTL) {
                        self.erase_cell();
                    }
                }
            }
            if kind == EraseKind::Erase {
                break;
            }
        }
    }

    /// Leitura canônica: até uma linha (`None` se ainda não há linha completa).
    pub(crate) fn read_canon(&mut self, buf: &mut [u8]) -> Option<usize> {
        let front = self.lines.front_mut()?;
        let n = buf.len().min(front.len());
        buf[..n].copy_from_slice(&front[..n]);
        front.drain(..n);
        if front.is_empty() {
            self.lines.pop_front();
        }
        Some(n)
    }

    pub(crate) fn read_raw(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.raw.len());
        for (d, s) in buf.iter_mut().zip(self.raw.drain(..n)) {
            *d = s;
        }
        n
    }

    pub(crate) fn read_out(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.out.len());
        for (d, s) in buf.iter_mut().zip(self.out.drain(..n)) {
            *d = s;
        }
        n
    }

    /// Troca o termios. Saindo do modo canônico, o que foi digitado vira entrada crua; entrando, a
    /// entrada crua vira a linha em edição (o `n_tty_set_termios`).
    pub(crate) fn set_termios(&mut self, t: Termios) {
        let was = self.canon();
        self.termios = t;
        let now = self.canon();
        if was && !now {
            let mut v: VecDeque<u8> = self.lines.drain(..).flatten().collect();
            v.extend(self.line.drain(..));
            v.extend(self.raw.drain(..));
            self.raw = v;
        } else if !was && now {
            let r: Vec<u8> = self.raw.drain(..).collect();
            self.line.extend(r);
        }
        self.lnext = false;
    }

    pub(crate) fn flush_input(&mut self) {
        self.lines.clear();
        self.line.clear();
        self.raw.clear();
        self.lnext = false;
    }
}

// ------------------------------------------------------------------------------------------------
// O par mestre e escravo
// ------------------------------------------------------------------------------------------------

#[derive(Debug)]
struct PtyState {
    ld: Ldisc,
    winsize: Winsize,
    /// `TIOCSPTLCK`: o escravo não abre enquanto travado.
    locked: bool,
    masters: u32,
    slaves: u32,
    /// O último escravo fechou (`TTY_OTHER_CLOSED` do mestre).
    slave_closed: bool,
    /// Sessão de que este é o terminal de controle, e o grupo em primeiro plano.
    session: Option<Pid>,
    pgrp: Option<Pid>,
    /// Espera pela entrada: leitores do escravo e escritores do mestre.
    in_wait: WaitList,
    /// Espera pela saída: leitores do mestre e escritores do escravo.
    out_wait: WaitList,
}

/// Um pseudoterminal (o par mestre e escravo).
pub(crate) struct Pty {
    pub index: u32,
    st: Mutex<PtyState>,
    sb: Weak<SbInner>,
}

impl std::fmt::Debug for Pty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pty({})", self.index)
    }
}

impl Pty {
    pub(crate) fn new(index: u32, sb: Weak<SbInner>) -> Arc<Pty> {
        Arc::new(Pty {
            index,
            sb,
            st: Mutex::new(PtyState {
                ld: Ldisc::default(),
                winsize: Winsize::default(),
                locked: true,
                masters: 0,
                slaves: 0,
                slave_closed: false,
                session: None,
                pgrp: None,
                in_wait: WaitList::default(),
                out_wait: WaitList::default(),
            }),
        })
    }

    /// `/dev/pts/N`.
    pub(crate) fn path(&self) -> Vec<u8> {
        format!("/dev/pts/{}", self.index).into_bytes()
    }

    /// Abre uma ponta.
    pub(crate) fn attach(self: &Arc<Self>, master: bool) -> PtyEnd {
        let mut s = self.st.lock();
        if master {
            s.masters += 1;
        } else {
            s.slaves += 1;
            s.slave_closed = false;
        }
        drop(s);
        PtyEnd { pty: self.clone(), master }
    }

    /// O escravo pode abrir: mestre aberto e destravado (senão EIO, como o `pty_open`).
    pub(crate) fn slave_openable(&self) -> Result<(), Errno> {
        let s = self.st.lock();
        if s.masters == 0 || s.locked { Err(Errno::EIO) } else { Ok(()) }
    }

    pub(crate) fn master_open(&self) -> bool {
        self.st.lock().masters > 0
    }

    pub(crate) fn set_locked(&self, locked: bool) {
        self.st.lock().locked = locked;
    }

    pub(crate) fn termios(&self) -> Termios {
        self.st.lock().ld.termios
    }

    pub(crate) fn set_termios(&self, t: &Termios, when: SetAttrWhen) {
        let w = {
            let mut s = self.st.lock();
            if when == SetAttrWhen::Flush {
                s.ld.flush_input();
            }
            s.ld.set_termios(sanitize(*t));
            let mut w = s.in_wait.take();
            w.merge(s.out_wait.take());
            w
        };
        w.run();
    }

    pub(crate) fn winsize(&self) -> Winsize {
        self.st.lock().winsize
    }

    /// Troca o tamanho. Devolve o grupo em primeiro plano a avisar com SIGWINCH quando mudou
    /// (`tty_do_resize`).
    pub(crate) fn set_winsize(&self, ws: Winsize) -> Option<Pid> {
        let mut s = self.st.lock();
        if s.winsize == ws {
            return None;
        }
        s.winsize = ws;
        s.pgrp
    }

    pub(crate) fn session(&self) -> Option<Pid> {
        self.st.lock().session
    }

    pub(crate) fn pgrp(&self) -> Option<Pid> {
        self.st.lock().pgrp
    }

    pub(crate) fn set_pgrp(&self, pgrp: Pid) {
        self.st.lock().pgrp = Some(pgrp);
    }

    /// Vira o terminal de controle de `session`, com `pgrp` em primeiro plano (`__proc_set_tty`).
    pub(crate) fn set_ctty(&self, session: Pid, pgrp: Pid) {
        let mut s = self.st.lock();
        s.session = Some(session);
        s.pgrp = Some(pgrp);
    }

    /// Deixa de ser terminal de controle. Devolve o grupo que estava em primeiro plano.
    pub(crate) fn clear_ctty(&self) -> Option<Pid> {
        let mut s = self.st.lock();
        s.session = None;
        s.pgrp.take()
    }

    /// Leitura do escravo. No modo canônico lê até uma linha; no outro, espera `need` bytes (0 = não
    /// espera nada). Desligado (sem mestre) é EOF.
    pub(crate) fn try_slave_read(&self, buf: &mut [u8], nonblock: bool, need: usize, waiter: &Arc<Parker>) -> Try<Result<usize, Errno>> {
        locked(&self.st, |s| {
            if s.masters == 0 || buf.is_empty() {
                s.in_wait.unregister(waiter);
                return (Try::Ready(Ok(0)), Wake::none());
            }
            let got = if s.ld.canon() {
                s.ld.read_canon(buf)
            } else {
                let avail = s.ld.raw_len();
                if need == 0 || avail >= need || (nonblock && avail > 0) { Some(s.ld.read_raw(buf)) } else { None }
            };
            match got {
                Some(n) => {
                    s.in_wait.unregister(waiter);
                    (Try::Ready(Ok(n)), s.in_wait.take())
                }
                None if nonblock => (Try::Ready(Err(Errno::EAGAIN)), Wake::none()),
                None => {
                    s.in_wait.register(waiter);
                    (Try::Pending, Wake::none())
                }
            }
        })
    }

    /// Leitura do mestre: a saída do escravo e o eco; sem escravo (depois de ter havido um) é EIO.
    pub(crate) fn try_master_read(&self, buf: &mut [u8], nonblock: bool, waiter: &Arc<Parker>) -> Try<Result<usize, Errno>> {
        locked(&self.st, |s| {
            if buf.is_empty() {
                return (Try::Ready(Ok(0)), Wake::none());
            }
            if !s.ld.out.is_empty() {
                let n = s.ld.read_out(buf);
                s.out_wait.unregister(waiter);
                (Try::Ready(Ok(n)), s.out_wait.take())
            } else if s.slave_closed {
                s.out_wait.unregister(waiter);
                (Try::Ready(Err(Errno::EIO)), Wake::none())
            } else if nonblock {
                (Try::Ready(Err(Errno::EAGAIN)), Wake::none())
            } else {
                s.out_wait.register(waiter);
                (Try::Pending, Wake::none())
            }
        })
    }

    /// Escrita do mestre: vira entrada do escravo pela disciplina de linha. Retomável (`done`). Os
    /// sinais de ISIG pro grupo em primeiro plano saem em `sigs`.
    pub(crate) fn try_master_write(
        &self,
        data: &[u8],
        done: &mut usize,
        nonblock: bool,
        sigs: &mut Vec<(Pid, Signal)>,
        waiter: &Arc<Parker>,
    ) -> Try<Result<usize, Errno>> {
        locked(&self.st, |s| {
            let mut w = Wake::none();
            let rest = &data[*done..];
            // No modo canônico a entrada nunca espera: o que não cabe na linha se perde.
            let n = if s.ld.canon() { rest.len() } else { rest.len().min(s.ld.input_room()) };
            if n > 0 {
                let got = s.ld.receive(&rest[..n]);
                *done += n;
                if let Some(pg) = s.pgrp {
                    sigs.extend(got.into_iter().map(|g| (pg, g)));
                }
                w.merge(s.in_wait.take());
                w.merge(s.out_wait.take());
            }
            (s.in_wait.write_outcome(*done, data.len(), nonblock, Errno::EAGAIN, waiter), w)
        })
    }

    /// Escrita do escravo: OPOST e pro mestre. Desligado é EIO.
    pub(crate) fn try_slave_write(&self, data: &[u8], done: &mut usize, nonblock: bool, waiter: &Arc<Parker>) -> Try<Result<usize, Errno>> {
        locked(&self.st, |s| {
            if s.masters == 0 {
                s.out_wait.unregister(waiter);
                return (Try::Ready(if *done > 0 { Ok(*done) } else { Err(Errno::EIO) }), Wake::none());
            }
            let before = *done;
            while *done < data.len() && s.ld.out.len() < PTY_OUT_LIMIT {
                let c = data[*done];
                s.ld.put_output(c);
                *done += 1;
            }
            let w = if *done > before { s.out_wait.take() } else { Wake::none() };
            (s.out_wait.write_outcome(*done, data.len(), nonblock, Errno::EAGAIN, waiter), w)
        })
    }

    /// Prontidão pro `poll` (`n_tty_poll` e `hung_up_tty_poll`).
    pub(crate) fn poll(&self, master: bool, waiter: Option<&Arc<Parker>>) -> PollEvents {
        let mut s = self.st.lock();
        let mut ev = PollEvents::empty();
        if master {
            if !s.ld.out.is_empty() {
                ev |= PollEvents::IN;
            }
            if s.slave_closed {
                ev |= PollEvents::HUP;
            }
            if s.ld.canon() || s.ld.input_room() > 0 {
                ev |= PollEvents::OUT;
            }
        } else if s.masters == 0 {
            ev |= PollEvents::IN | PollEvents::OUT | PollEvents::ERR | PollEvents::HUP;
        } else {
            if s.ld.has_input() {
                ev |= PollEvents::IN;
            }
            if s.ld.out.len() < PTY_OUT_LIMIT {
                ev |= PollEvents::OUT;
            }
        }
        if let Some(w) = waiter {
            s.in_wait.register(w);
            s.out_wait.register(w);
        }
        ev
    }

    pub(crate) fn unregister(&self, waiter: &Arc<Parker>) {
        let mut s = self.st.lock();
        s.in_wait.unregister(waiter);
        s.out_wait.unregister(waiter);
    }

    /// O último mestre fechou: o nó some e o líder da sessão leva SIGHUP e SIGCONT (`pty_close` e
    /// `tty_vhangup` do escravo).
    fn master_closed(&self, session: Option<Pid>) {
        let Some(sb) = self.sb.upgrade() else { return };
        let cx = sb.root_caller();
        let _ = sb.ns.unlink(&cx, &Start::Cwd, &self.path(), AtFlags::empty());
        if let Some(sid) = session {
            signal_pid(&sb, sid, SIGHUP);
            signal_pid(&sb, sid, SIGCONT);
        }
    }
}

/// Uma ponta aberta de um pty (dentro de uma descrição de arquivo aberto).
pub(crate) struct PtyEnd {
    pub pty: Arc<Pty>,
    pub master: bool,
}

impl std::fmt::Debug for PtyEnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PtyEnd({}, {})", self.pty.index, if self.master { "master" } else { "slave" })
    }
}

impl Clone for PtyEnd {
    fn clone(&self) -> Self {
        self.pty.attach(self.master)
    }
}

impl PartialEq for PtyEnd {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.pty, &other.pty) && self.master == other.master
    }
}

impl Eq for PtyEnd {}

impl Drop for PtyEnd {
    fn drop(&mut self) {
        let (w, hangup) = {
            let mut s = self.pty.st.lock();
            let mut w = s.in_wait.take();
            w.merge(s.out_wait.take());
            let mut hangup = None;
            if self.master {
                s.masters = s.masters.saturating_sub(1);
                if s.masters == 0 {
                    s.pgrp = None;
                    hangup = Some(s.session.take());
                }
            } else {
                s.slaves = s.slaves.saturating_sub(1);
                if s.slaves == 0 {
                    s.slave_closed = true;
                }
            }
            (w, hangup)
        };
        w.run();
        if let Some(session) = hangup {
            self.pty.master_closed(session);
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Sinais
// ------------------------------------------------------------------------------------------------

/// Manda `sig` pra um grupo de processos (`kill_pgrp`).
pub(crate) fn signal_pgrp(sb: &SbInner, pgrp: Pid, sig: Signal) {
    let procs = sb.table.lock().group_members(pgrp);
    for p in &procs {
        crate::sys::generate_signal(p, sig);
    }
}

fn signal_pid(sb: &SbInner, pid: Pid, sig: Signal) {
    let p = sb.table.lock().proc(pid);
    if let Some(p) = p {
        crate::sys::generate_signal(&p, sig);
    }
}

/// SIGWINCH do `TIOCSWINSZ`.
pub(crate) fn sigwinch(sb: &SbInner, pgrp: Pid) {
    signal_pgrp(sb, pgrp, SIGWINCH);
}

/// SIGHUP e SIGCONT pro grupo em primeiro plano (`disassociate_ctty` fora do exit).
pub(crate) fn hangup_pgrp(sb: &SbInner, pgrp: Pid) {
    signal_pgrp(sb, pgrp, SIGHUP);
    signal_pgrp(sb, pgrp, SIGCONT);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(ld: &mut Ldisc) -> Vec<u8> {
        ld.out.drain(..).collect()
    }

    fn read_line(ld: &mut Ldisc) -> Option<Vec<u8>> {
        let mut b = [0u8; 256];
        ld.read_canon(&mut b).map(|n| b[..n].to_vec())
    }

    #[test]
    fn default_termios_is_tty_std_termios() {
        let t = Termios::default();
        assert_eq!(sanitize(t), t);
    }

    #[test]
    fn canonical_line_with_echo_and_onlcr() {
        let mut ld = Ldisc::default();
        assert!(ld.receive(b"ls -l\r").is_empty());
        assert_eq!(out(&mut ld), b"ls -l\r\n");
        assert_eq!(read_line(&mut ld).unwrap(), b"ls -l\n");
        assert_eq!(read_line(&mut ld), None);
    }

    #[test]
    fn partial_line_is_not_readable() {
        let mut ld = Ldisc::default();
        ld.receive(b"abc");
        assert!(!ld.has_input());
        assert_eq!(read_line(&mut ld), None);
    }

    #[test]
    fn erase_with_echoe() {
        let mut ld = Ldisc::default();
        ld.receive(b"abd\x7fc\n");
        assert_eq!(out(&mut ld), b"abd\x08 \x08c\r\n");
        assert_eq!(read_line(&mut ld).unwrap(), b"abc\n");
    }

    #[test]
    fn erase_control_char_takes_two_cells() {
        let mut ld = Ldisc::default();
        ld.receive(b"a\x01\x7f");
        assert_eq!(out(&mut ld), b"a^A\x08 \x08\x08 \x08");
    }

    #[test]
    fn erase_without_echoe_echoes_the_erase_char() {
        let mut ld = Ldisc::default();
        let mut t = ld.termios;
        t.c_lflag &= !ECHOE;
        ld.set_termios(t);
        ld.receive(b"ab\x7f");
        assert_eq!(out(&mut ld), b"ab^?");
    }

    #[test]
    fn kill_erases_the_whole_line_with_echoke() {
        let mut ld = Ldisc::default();
        ld.receive(b"xyz\x15ok\n");
        assert_eq!(out(&mut ld), b"xyz\x08 \x08\x08 \x08\x08 \x08ok\r\n");
        assert_eq!(read_line(&mut ld).unwrap(), b"ok\n");
    }

    #[test]
    fn kill_without_echoke_echoes_and_newlines() {
        let mut ld = Ldisc::default();
        let mut t = ld.termios;
        t.c_lflag &= !ECHOKE;
        ld.set_termios(t);
        ld.receive(b"xy\x15");
        assert_eq!(out(&mut ld), b"xy^U\r\n");
    }

    #[test]
    fn werase_removes_last_word_and_spaces() {
        let mut ld = Ldisc::default();
        ld.receive(b"foo bar  \x17\n");
        assert_eq!(read_line(&mut ld).unwrap(), b"foo \n");
    }

    #[test]
    fn eof_terminates_line_and_empty_eof_reads_zero() {
        let mut ld = Ldisc::default();
        ld.receive(b"abc\x04\x04");
        assert_eq!(out(&mut ld), b"abc");
        assert_eq!(read_line(&mut ld).unwrap(), b"abc");
        assert_eq!(read_line(&mut ld).unwrap(), b"");
        assert_eq!(read_line(&mut ld), None);
    }

    #[test]
    fn isig_generates_signals_flushes_and_echoes() {
        let mut ld = Ldisc::default();
        ld.out.extend(b"pending");
        let sigs = ld.receive(b"abc\x03");
        assert_eq!(sigs, vec![SIGINT]);
        assert_eq!(out(&mut ld), b"^C");
        assert_eq!(ld.input_len(), 0);
        assert_eq!(ld.receive(b"\x1c\x1a"), vec![SIGQUIT, SIGTSTP]);
    }

    #[test]
    fn noflsh_keeps_input() {
        let mut ld = Ldisc::default();
        let mut t = ld.termios;
        t.c_lflag |= NOFLSH;
        ld.set_termios(t);
        ld.receive(b"abc\x03");
        assert_eq!(ld.input_len(), 3);
    }

    #[test]
    fn input_cr_nl_translations() {
        let mut ld = Ldisc::default();
        let mut t = ld.termios;
        t.c_lflag &= !(ICANON | ECHO);
        t.c_iflag = IGNCR;
        ld.set_termios(t);
        ld.receive(b"a\rb\n");
        let mut b = [0u8; 8];
        let n = ld.read_raw(&mut b);
        assert_eq!(&b[..n], b"ab\n");
        t.c_iflag = INLCR;
        ld.set_termios(t);
        ld.receive(b"\n");
        let n = ld.read_raw(&mut b);
        assert_eq!(&b[..n], b"\r");
    }

    #[test]
    fn noncanonical_echo_of_cr_and_nl() {
        let mut ld = Ldisc::default();
        let mut t = ld.termios;
        t.c_lflag &= !ICANON;
        ld.set_termios(t);
        ld.receive(b"\r\n");
        // O CR traduzido pelo ICRNL ecoa como quebra de linha; o LF literal, como ^J.
        assert_eq!(out(&mut ld), b"\r\n^J");
        assert_eq!(ld.raw_len(), 2);
    }

    #[test]
    fn opost_off_writes_raw() {
        let mut ld = Ldisc::default();
        let mut t = ld.termios;
        t.c_oflag &= !OPOST;
        ld.set_termios(t);
        for &c in b"a\nb" {
            ld.put_output(c);
        }
        assert_eq!(out(&mut ld), b"a\nb");
    }

    #[test]
    fn xtabs_expands_tabs() {
        let mut ld = Ldisc::default();
        let mut t = ld.termios;
        t.c_oflag |= XTABS;
        ld.set_termios(t);
        for &c in b"ab\tc" {
            ld.put_output(c);
        }
        assert_eq!(out(&mut ld), b"ab      c");
    }

    #[test]
    fn leaving_canonical_mode_makes_typed_text_readable() {
        let mut ld = Ldisc::default();
        ld.receive(b"line\nhalf");
        let mut t = ld.termios;
        t.c_lflag &= !ICANON;
        ld.set_termios(t);
        let mut b = [0u8; 16];
        let n = ld.read_raw(&mut b);
        assert_eq!(&b[..n], b"line\nhalf");
    }

    #[test]
    fn lnext_makes_next_char_literal() {
        let mut ld = Ldisc::default();
        ld.receive(b"\x16\x03\n");
        assert_eq!(read_line(&mut ld).unwrap(), b"\x03\n");
    }

    #[test]
    fn sanitize_forces_cs8_and_mirrors_speed() {
        let mut t = Termios::default();
        t.c_cflag = (t.c_cflag & !(CBAUD | CSIZE)) | B9600 | CS7 | PARENB;
        let s = sanitize(t);
        assert_eq!(s.c_cflag & CSIZE, CS8);
        assert_eq!(s.c_cflag & PARENB, 0);
        assert_eq!((s.c_ispeed, s.c_ospeed), (9600, 9600));
    }

    #[test]
    fn pty_pair_round_trip_and_lock() {
        let pty = Pty::new(0, Weak::new());
        let waiter = Parker::new();
        let m = pty.attach(true);
        assert_eq!(pty.slave_openable(), Err(Errno::EIO));
        pty.set_locked(false);
        pty.slave_openable().unwrap();
        let s = pty.attach(false);
        let mut done = 0;
        let mut sigs = Vec::new();
        assert!(matches!(pty.try_master_write(b"hi\r", &mut done, false, &mut sigs, &waiter), Try::Ready(Ok(3))));
        let mut buf = [0u8; 16];
        assert!(matches!(pty.try_slave_read(&mut buf, false, 1, &waiter), Try::Ready(Ok(3))));
        assert_eq!(&buf[..3], b"hi\n");
        // Eco no mestre.
        assert!(matches!(pty.try_master_read(&mut buf, false, &waiter), Try::Ready(Ok(4))));
        assert_eq!(&buf[..4], b"hi\r\n");
        let mut done = 0;
        assert!(matches!(pty.try_slave_write(b"ok\n", &mut done, false, &waiter), Try::Ready(Ok(3))));
        assert!(matches!(pty.try_master_read(&mut buf, false, &waiter), Try::Ready(Ok(4))));
        assert_eq!(&buf[..4], b"ok\r\n");
        // Sem escravo, o mestre lê EIO; sem mestre, o escravo lê EOF e escreve EIO.
        drop(s);
        assert!(matches!(pty.try_master_read(&mut buf, true, &waiter), Try::Ready(Err(Errno::EIO))));
        let s2 = pty.attach(false);
        drop(m);
        assert!(matches!(pty.try_slave_read(&mut buf, false, 1, &waiter), Try::Ready(Ok(0))));
        let mut done = 0;
        assert!(matches!(pty.try_slave_write(b"x", &mut done, false, &waiter), Try::Ready(Err(Errno::EIO))));
        assert!(pty.poll(false, None).contains(PollEvents::HUP));
        drop(s2);
    }

    #[test]
    fn pty_isig_goes_to_foreground_group() {
        let pty = Pty::new(1, Weak::new());
        let waiter = Parker::new();
        let _m = pty.attach(true);
        pty.set_ctty(10, 12);
        let mut done = 0;
        let mut sigs = Vec::new();
        let _ = pty.try_master_write(b"\x03", &mut done, false, &mut sigs, &waiter);
        assert_eq!(sigs, vec![(12, SIGINT)]);
    }

    #[test]
    fn winsize_change_reports_pgrp_once() {
        let pty = Pty::new(2, Weak::new());
        pty.set_ctty(5, 7);
        let ws = Winsize { rows: 24, cols: 80, xpixel: 0, ypixel: 0 };
        assert_eq!(pty.set_winsize(ws), Some(7));
        assert_eq!(pty.set_winsize(ws), None);
        assert_eq!(pty.winsize(), ws);
    }

    #[test]
    fn noncanonical_min_and_nonblock() {
        let pty = Pty::new(3, Weak::new());
        let waiter = Parker::new();
        let _m = pty.attach(true);
        let mut t = pty.termios();
        t.c_lflag &= !(ICANON | ECHO);
        pty.set_termios(&t, SetAttrWhen::Now);
        let mut buf = [0u8; 8];
        assert!(matches!(pty.try_slave_read(&mut buf, true, 1, &waiter), Try::Ready(Err(Errno::EAGAIN))));
        assert!(matches!(pty.try_slave_read(&mut buf, false, 0, &waiter), Try::Ready(Ok(0))));
        let mut done = 0;
        let mut sigs = Vec::new();
        let _ = pty.try_master_write(b"ab", &mut done, false, &mut sigs, &waiter);
        assert!(matches!(pty.try_slave_read(&mut buf, false, 3, &waiter), Try::Pending));
        assert!(matches!(pty.try_slave_read(&mut buf, false, 2, &waiter), Try::Ready(Ok(2))));
    }
}
