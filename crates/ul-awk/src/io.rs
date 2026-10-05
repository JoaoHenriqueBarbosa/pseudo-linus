//! E/S do awk sobre `sysabi`: leitores de registro (RS em todas as formas), fluxos de saída com buffer
//! (arquivos, `/dev/stdout`, `/dev/stderr`, pipes e coprocessos) e os processos filhos (`sh -c`).
//!
//! Pipes de saída nascem "pendentes": os dados ficam no buffer até o primeiro ponto de descarga do gawk
//! (`fflush`, `system`, abertura de outro pipe de saída, `close`, fim do programa ou buffer cheio). Aí o
//! pipe é criado, o que está pendente é escrito sem bloquear até onde o pipe aceitar, e o filho é
//! criado lendo dele; o resto vai depois, bloqueando. No kernel real o filho roda junto (como o
//! `popen` do gawk); no testkit, que roda o filho até o fim dentro do `spawn`, o filho já encontra os
//! dados no pipe.

use std::rc::Rc;
use std::sync::Arc;

use sysabi::{Errno, Fd, FdAction, OFlags, Pid, ProcAttrs, Signal, SigDisposition, SpawnSpec, Syscalls, WaitOptions, WaitStatus, WaitTarget};

use crate::regex::Regex;

/// Tamanho de leitura.
const READ_CHUNK: usize = 64 * 1024;
/// Acima disso o buffer de um fluxo de saída é descarregado.
const OUT_LIMIT: usize = 64 * 1024;

/// Um registro lido e o terminador dele (`RT`), `None` no fim do fluxo.
pub type RecordResult = Result<Option<(Vec<u8>, Vec<u8>)>, Errno>;

/// Separador de registros vigente.
#[derive(Clone)]
pub enum RsMode {
    /// `RS = "\n"`.
    Newline,
    /// `RS` de um caractere (os bytes dele em UTF-8); `icase` vale pra letra ASCII.
    Char(Vec<u8>, bool),
    /// `RS = ""`: parágrafos.
    Paragraph,
    /// `RS` com mais de um caractere: regex.
    Regex(Rc<Regex>),
}

/// Leitor de registros sobre um fd.
pub struct Reader {
    pub fd: Fd,
    buf: Vec<u8>,
    pos: usize,
    eof: bool,
    /// Ainda não consumiu nada do início do fluxo (pra `^` em RS regex e o parágrafo inicial).
    at_start: bool,
    /// Fecha o fd no fim (não fecha o stdin herdado).
    owns_fd: bool,
}

impl Reader {
    pub fn new(fd: Fd, owns_fd: bool) -> Reader {
        Reader { fd, buf: Vec::new(), pos: 0, eof: false, at_start: true, owns_fd }
    }

    pub fn owns_fd(&self) -> bool {
        self.owns_fd
    }

    /// Lê mais dados pro buffer. Devolve falso no fim do fluxo.
    fn fill(&mut self, sys: &Arc<dyn Syscalls>) -> Result<bool, Errno> {
        if self.eof {
            return Ok(false);
        }
        if self.pos > 0 && self.pos == self.buf.len() {
            self.buf.clear();
            self.pos = 0;
        } else if self.pos > READ_CHUNK {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        let old = self.buf.len();
        self.buf.try_reserve(READ_CHUNK).map_err(|_| Errno::ENOMEM)?;
        self.buf.resize(old + READ_CHUNK, 0);
        loop {
            match sys.read(self.fd, &mut self.buf[old..]) {
                Ok(0) => {
                    self.buf.truncate(old);
                    self.eof = true;
                    return Ok(false);
                }
                Ok(n) => {
                    self.buf.truncate(old + n);
                    return Ok(true);
                }
                Err(Errno::EINTR) => continue,
                Err(e) => {
                    self.buf.truncate(old);
                    return Err(e);
                }
            }
        }
    }

    fn consume(&mut self, n: usize) {
        self.pos += n;
        if n > 0 {
            self.at_start = false;
        }
    }

    /// Próximo registro e o terminador (`RT`), ou `None` no fim.
    pub fn read_record(&mut self, sys: &Arc<dyn Syscalls>, rs: &RsMode) -> RecordResult {
        match rs {
            RsMode::Newline => self.read_until_byte(sys, b'\n'),
            RsMode::Char(c, icase) => {
                if c.len() == 1 && !*icase {
                    self.read_until_byte(sys, c[0])
                } else {
                    self.read_until_seq(sys, c, *icase)
                }
            }
            RsMode::Paragraph => self.read_paragraph(sys),
            RsMode::Regex(re) => self.read_regex(sys, re),
        }
    }

    fn read_until_byte(&mut self, sys: &Arc<dyn Syscalls>, sep: u8) -> RecordResult {
        let mut scan_from = self.pos;
        loop {
            if let Some(i) = memchr(sep, &self.buf[scan_from..]) {
                let end = scan_from + i;
                let rec = self.buf[self.pos..end].to_vec();
                let n = end + 1 - self.pos;
                self.consume(n);
                return Ok(Some((rec, vec![sep])));
            }
            let scanned = self.buf.len() - self.pos;
            let before = self.pos;
            if !self.fill(sys)? {
                if self.pos < self.buf.len() {
                    let rec = self.buf[self.pos..].to_vec();
                    let n = self.buf.len() - self.pos;
                    self.consume(n);
                    return Ok(Some((rec, Vec::new())));
                }
                return Ok(None);
            }
            // O `fill` pode ter compactado o buffer.
            scan_from = self.pos + scanned;
            let _ = before;
        }
    }

    fn read_until_seq(&mut self, sys: &Arc<dyn Syscalls>, sep: &[u8], icase: bool) -> RecordResult {
        loop {
            let hay = &self.buf[self.pos..];
            let found = if icase {
                hay.windows(sep.len()).position(|w| w.eq_ignore_ascii_case(sep))
            } else {
                hay.windows(sep.len()).position(|w| w == sep)
            };
            if let Some(i) = found {
                let rec = hay[..i].to_vec();
                let rt = hay[i..i + sep.len()].to_vec();
                self.consume(i + sep.len());
                return Ok(Some((rec, rt)));
            }
            if !self.fill(sys)? {
                if self.pos < self.buf.len() {
                    let rec = self.buf[self.pos..].to_vec();
                    let n = self.buf.len() - self.pos;
                    self.consume(n);
                    return Ok(Some((rec, Vec::new())));
                }
                return Ok(None);
            }
        }
    }

    fn read_paragraph(&mut self, sys: &Arc<dyn Syscalls>) -> RecordResult {
        // Pula os newlines do começo.
        loop {
            while self.pos < self.buf.len() && self.buf[self.pos] == b'\n' {
                self.consume(1);
            }
            if self.pos < self.buf.len() {
                break;
            }
            if !self.fill(sys)? {
                return Ok(None);
            }
        }
        // Deslocamentos relativos a `pos` (o `fill` pode compactar o buffer, mas preserva isso).
        let mut scan = 0usize;
        loop {
            let hay = &self.buf[self.pos..];
            let mut found = None;
            let mut i = scan;
            while let Some(j) = memchr(b'\n', &hay[i..]) {
                let k = i + j;
                if k + 1 < hay.len() && hay[k + 1] == b'\n' {
                    found = Some(k);
                    break;
                }
                i = k + 1;
            }
            if let Some(sep) = found {
                // Junta todos os newlines seguidos (pode precisar ler mais).
                let mut end = sep;
                loop {
                    while self.pos + end < self.buf.len() && self.buf[self.pos + end] == b'\n' {
                        end += 1;
                    }
                    if self.pos + end < self.buf.len() || !self.fill(sys)? {
                        break;
                    }
                }
                let rec = self.buf[self.pos..self.pos + sep].to_vec();
                let rt = self.buf[self.pos + sep..self.pos + end].to_vec();
                self.consume(end);
                return Ok(Some((rec, rt)));
            }
            // Recomeça a busca um byte antes do fim (um newline final pode formar par com o próximo).
            scan = (self.buf.len() - self.pos).saturating_sub(1);
            if !self.fill(sys)? {
                if self.pos < self.buf.len() {
                    let mut rec = self.buf[self.pos..].to_vec();
                    let mut rt = Vec::new();
                    while rec.last() == Some(&b'\n') {
                        rec.pop();
                        rt.push(b'\n');
                    }
                    let n = self.buf.len() - self.pos;
                    self.consume(n);
                    return Ok(Some((rec, rt)));
                }
                return Ok(None);
            }
        }
    }

    fn read_regex(&mut self, sys: &Arc<dyn Syscalls>, re: &Regex) -> RecordResult {
        loop {
            if self.pos >= self.buf.len() && !self.fill(sys)? {
                return Ok(None);
            }
            let not_bol = !self.at_start;
            let hay = &self.buf[self.pos..];
            // Casada não vazia mais à esquerda.
            let mut from = 0;
            let mut found = None;
            while from <= hay.len() {
                match re.find_at(hay, from, not_bol) {
                    Some((s, e)) if e > s => {
                        found = Some((s, e));
                        break;
                    }
                    Some((s, _)) => from = s + 1,
                    None => break,
                }
            }
            match found {
                Some((s, e)) if e < hay.len() || self.eof => {
                    let rec = hay[..s].to_vec();
                    let rt = hay[s..e].to_vec();
                    self.consume(e);
                    return Ok(Some((rec, rt)));
                }
                _ => {
                    if !self.fill(sys)? {
                        // Fim: tenta de novo sem esperar mais dados (a casada pode ir até o fim).
                        let hay = &self.buf[self.pos..];
                        if hay.is_empty() {
                            return Ok(None);
                        }
                        let mut from = 0;
                        let mut found = None;
                        while from <= hay.len() {
                            match re.find_at(hay, from, not_bol) {
                                Some((s, e)) if e > s => {
                                    found = Some((s, e));
                                    break;
                                }
                                Some((s, _)) => from = s + 1,
                                None => break,
                            }
                        }
                        let (rec, rt, n) = match found {
                            Some((s, e)) => (hay[..s].to_vec(), hay[s..e].to_vec(), e),
                            None => (hay.to_vec(), Vec::new(), hay.len()),
                        };
                        self.consume(n);
                        return Ok(Some((rec, rt)));
                    }
                }
            }
        }
    }
}

pub fn memchr(c: u8, hay: &[u8]) -> Option<usize> {
    hay.iter().position(|b| *b == c)
}

/// Estado do lado de escrita de um pipe.
pub enum PipeState {
    /// Ainda não existe processo: os dados estão no buffer.
    Pending,
    Running { fd: Fd, pid: Pid },
    /// Fechado do lado de escrita (`close(cmd, "to")`), com o pid ainda a colher.
    WriteClosed { pid: Pid },
}

pub enum OutKind {
    File(Fd),
    /// `/dev/stdout` (e o próprio stdout).
    Stdout,
    /// `/dev/stderr`: sem buffer.
    Stderr,
    Pipe(PipeState),
    /// Coprocesso (`|&`): escrita como pipe, leitura pelo `reader`.
    Coproc { write: PipeState, reader: Option<Reader>, pid: Option<Pid> },
}

pub struct OutStream {
    pub name: Vec<u8>,
    pub kind: OutKind,
    pub buf: Vec<u8>,
}

/// Erro de E/S com o errno e a operação, pra mensagem do gawk.
#[derive(Debug)]
pub struct IoFail(pub Errno);

/// Executa `sh -c cmd` com as ações de fd dadas; devolve o pid.
pub fn spawn_shell(sys: &Arc<dyn Syscalls>, cmd: &[u8], fd_actions: Vec<FdAction>) -> Result<Pid, Errno> {
    let attrs = ProcAttrs { fd_actions, reset_signals: vec![Signal::SIGPIPE], ..ProcAttrs::default() };
    sys.spawn(SpawnSpec { path: b"/bin/sh".to_vec(), argv: vec![b"sh".to_vec(), b"-c".to_vec(), cmd.to_vec()], attrs })
}

/// Status no formato do `close`/`system` do gawk: código de saída, ou 256 + sinal.
pub fn status_value(st: WaitStatus) -> f64 {
    match st {
        WaitStatus::Exited(c) => (c & 0xff) as f64,
        WaitStatus::Signaled { signal, core_dumped } => (if core_dumped { 512 } else { 256 } + signal.0) as f64,
        WaitStatus::Stopped(s) => (256 + s.0) as f64,
        WaitStatus::Continued => 0.0,
    }
}

/// Espera um filho e devolve o status.
pub fn wait_pid(sys: &Arc<dyn Syscalls>, pid: Pid) -> Option<WaitStatus> {
    loop {
        match sys.wait4(WaitTarget::Pid(pid), WaitOptions::empty()) {
            Ok(Some((_, st))) => return Some(st),
            Ok(None) => return None,
            Err(Errno::EINTR) => continue,
            Err(_) => return None,
        }
    }
}

/// Escreve tudo num fd; EPIPE volta como erro (o chamador decide se é fatal).
pub fn write_all(sys: &Arc<dyn Syscalls>, fd: Fd, mut data: &[u8]) -> Result<(), Errno> {
    while !data.is_empty() {
        match sys.write(fd, data) {
            Ok(0) => return Err(Errno::EIO),
            Ok(n) => data = &data[n..],
            Err(Errno::EINTR) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Escreve num pipe de redirecionamento com o SIGPIPE ignorado (o gawk trata EPIPE como erro fatal
/// de `print`, sem morrer do sinal).
pub fn write_pipe(sys: &Arc<dyn Syscalls>, fd: Fd, data: &[u8]) -> Result<(), Errno> {
    let old = sys.sigaction(Signal::SIGPIPE, SigDisposition::Ignore).ok();
    let r = write_all(sys, fd, data);
    if let Some(old) = old {
        let _ = sys.sigaction(Signal::SIGPIPE, old);
    }
    r
}

/// Cria o filho de um pipe de saída pendente, já entregando o que está no buffer.
/// Devolve o fd de escrita e o pid; o buffer fica com o que não coube sem bloquear (já escrito depois).
pub fn start_output_pipe(sys: &Arc<dyn Syscalls>, cmd: &[u8], buf: &mut Vec<u8>) -> Result<(Fd, Pid), Errno> {
    let (r, w) = sys.pipe2(OFlags::CLOEXEC)?;
    // Escreve sem bloquear o que couber antes do filho existir.
    let _ = sys.set_status_flags(w, OFlags::NONBLOCK);
    let mut written = 0;
    let old = sys.sigaction(Signal::SIGPIPE, SigDisposition::Ignore).ok();
    while written < buf.len() {
        match sys.write(w, &buf[written..]) {
            Ok(0) => break,
            Ok(n) => written += n,
            Err(Errno::EINTR) => {}
            Err(_) => break,
        }
    }
    if let Some(old) = old {
        let _ = sys.sigaction(Signal::SIGPIPE, old);
    }
    let _ = sys.set_status_flags(w, OFlags::empty());
    let pid = match spawn_shell(sys, cmd, vec![FdAction::Dup2 { from: r, to: Fd::STDIN }]) {
        Ok(p) => p,
        Err(e) => {
            let _ = sys.close(r);
            let _ = sys.close(w);
            return Err(e);
        }
    };
    let _ = sys.close(r);
    buf.drain(..written);
    if !buf.is_empty() {
        let rest = std::mem::take(buf);
        write_pipe(sys, w, &rest)?;
    }
    Ok((w, pid))
}

/// `cmd | getline`: cria o filho com a saída num pipe e devolve o leitor e o pid.
pub fn start_input_pipe(sys: &Arc<dyn Syscalls>, cmd: &[u8]) -> Result<(Reader, Pid), Errno> {
    let (r, w) = sys.pipe2(OFlags::CLOEXEC)?;
    let pid = match spawn_shell(sys, cmd, vec![FdAction::Dup2 { from: w, to: Fd::STDOUT }]) {
        Ok(p) => p,
        Err(e) => {
            let _ = sys.close(r);
            let _ = sys.close(w);
            return Err(e);
        }
    };
    let _ = sys.close(w);
    Ok((Reader::new(r, true), pid))
}

/// Coprocesso: cria o filho com stdin e stdout em pipes, entregando o que estava pendente.
pub fn start_coproc(sys: &Arc<dyn Syscalls>, cmd: &[u8], pending: &mut Vec<u8>, close_write: bool) -> Result<(Option<Fd>, Reader, Pid), Errno> {
    let (in_r, in_w) = sys.pipe2(OFlags::CLOEXEC)?;
    let (out_r, out_w) = sys.pipe2(OFlags::CLOEXEC)?;
    let _ = sys.set_status_flags(in_w, OFlags::NONBLOCK);
    let mut written = 0;
    while written < pending.len() {
        match sys.write(in_w, &pending[written..]) {
            Ok(0) => break,
            Ok(n) => written += n,
            Err(Errno::EINTR) => {}
            Err(_) => break,
        }
    }
    let _ = sys.set_status_flags(in_w, OFlags::empty());
    pending.drain(..written);
    let actions = vec![FdAction::Dup2 { from: in_r, to: Fd::STDIN }, FdAction::Dup2 { from: out_w, to: Fd::STDOUT }];
    if close_write && pending.is_empty() {
        // O filho deve ver EOF logo: fecha nossa ponta antes de criá-lo.
        let _ = sys.close(in_w);
        let pid = spawn_shell(sys, cmd, actions);
        let _ = sys.close(in_r);
        let _ = sys.close(out_w);
        return Ok((None, Reader::new(out_r, true), pid?));
    }
    let pid = spawn_shell(sys, cmd, actions);
    let _ = sys.close(in_r);
    let _ = sys.close(out_w);
    let pid = pid?;
    if !pending.is_empty() {
        let rest = std::mem::take(pending);
        write_pipe(sys, in_w, &rest)?;
    }
    if close_write {
        let _ = sys.close(in_w);
        return Ok((None, Reader::new(out_r, true), pid));
    }
    Ok((Some(in_w), Reader::new(out_r, true), pid))
}

pub const OUT_FLUSH_LIMIT: usize = OUT_LIMIT;
