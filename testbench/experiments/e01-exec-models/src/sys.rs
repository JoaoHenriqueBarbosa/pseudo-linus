//! API de processo dos modelos síncronos (A e B). Todo processo recebe um `&dyn Sys`.
//!
//! As chamadas que esperam (read, write com buffer cheio, wait) são o mesmo laço nos dois modelos:
//! tentam a operação no estilo `poll`, e se ela não está pronta chamam `block()`, que é o que muda de
//! modelo pra modelo. Sinal fatal é entregue na entrada de cada chamada e na volta de cada bloqueio.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Poll, Waker};

use crate::kernel::{Core, Errno, ExitStatus, Fd, File, FileRef, Pid, ProcCommon, SIGPIPE, SysResult, new_pipe};

/// Corpo de um processo dos modelos síncronos.
pub type ProcMain = Box<dyn FnOnce(&dyn Sys) -> i32 + Send + 'static>;

pub trait Sys {
    fn proc(&self) -> &Arc<ProcCommon>;
    fn core(&self) -> &Core;
    /// Waker do próprio processo.
    fn waker(&self) -> &Waker;
    /// Espera até ser acordado pelo waker (pode voltar espuriamente; quem chama repete a operação).
    fn block(&self);
    /// Cede a CPU virtual se houver outro processo pronto.
    fn yield_now(&self);
    /// Cria um processo com a tabela de descritores dada.
    fn spawn_with(&self, files: Vec<(Fd, File)>, main: ProcMain) -> Pid;
    /// Gancho do modelo quando o processo reconhece `attention` (o modelo A usa pra tentar desfazer o
    /// rebaixamento de prioridade do watchdog).
    fn on_attention_ack(&self) {}

    fn pid(&self) -> Pid {
        self.proc().pid
    }

    /// Ponto de checagem: uma leitura de `AtomicBool`. Só o caminho lento é chamada de verdade.
    #[inline(always)]
    fn checkpoint(&self) {
        if self.proc().attention.load(Ordering::Relaxed) {
            self.checkpoint_slow();
        }
    }

    /// Caminho lento: entrega sinal (unwind) e cede a CPU virtual se houver outro pronto.
    fn checkpoint_slow(&self) {
        let set_ns = self.core().ack_attention(self.proc());
        self.on_attention_ack();
        self.core().probe_before_yield(set_ns);
        self.yield_now();
    }

    fn read(&self, fd: Fd, buf: &mut [u8]) -> SysResult<usize> {
        self.proc().check_signals();
        match self.proc().file_ref(fd)? {
            FileRef::Read(pipe) => loop {
                match pipe.poll_read(buf, self.waker()) {
                    Poll::Ready(n) => return Ok(n),
                    Poll::Pending => {
                        self.block();
                        self.proc().check_signals();
                    }
                }
            },
            FileRef::Sink(_) => Ok(0),
            FileRef::Write(_) => Err(Errno::Badf),
        }
    }

    /// Escreve tudo (bloqueando quando o pipe enche). Sem leitores, entrega SIGPIPE ao próprio processo.
    fn write(&self, fd: Fd, data: &[u8]) -> SysResult<usize> {
        self.proc().check_signals();
        match self.proc().file_ref(fd)? {
            FileRef::Write(pipe) => {
                let mut off = 0;
                while off < data.len() {
                    match pipe.poll_write(&data[off..], self.waker()) {
                        Poll::Ready(Ok(n)) => off += n,
                        Poll::Ready(Err(_)) => {
                            self.proc().raise(SIGPIPE);
                            self.proc().check_signals();
                            return Err(Errno::Pipe);
                        }
                        Poll::Pending => {
                            self.block();
                            self.proc().check_signals();
                        }
                    }
                }
                Ok(off)
            }
            FileRef::Sink(c) => {
                c.fetch_add(data.len() as u64, Ordering::Relaxed);
                Ok(data.len())
            }
            FileRef::Read(_) => Err(Errno::Badf),
        }
    }

    fn close(&self, fd: Fd) -> SysResult<()> {
        self.proc().close(fd)
    }

    fn pipe(&self) -> (Fd, Fd) {
        let (r, w) = new_pipe();
        let rfd = self.proc().install(r);
        let wfd = self.proc().install(w);
        (rfd, wfd)
    }

    /// Cria um filho herdando os descritores pedidos: `(fd_no_filho, fd_no_pai)`.
    fn spawn(&self, inherit: &[(Fd, Fd)], main: ProcMain) -> SysResult<Pid> {
        let mut files = Vec::with_capacity(inherit.len());
        for &(child_fd, parent_fd) in inherit {
            files.push((child_fd, self.proc().dup_file(parent_fd)?));
        }
        Ok(self.spawn_with(files, main))
    }

    fn wait(&self, pid: Pid) -> SysResult<ExitStatus> {
        self.proc().check_signals();
        loop {
            match self.core().poll_wait(pid, self.waker()) {
                Poll::Ready(r) => return r,
                Poll::Pending => {
                    self.block();
                    self.proc().check_signals();
                }
            }
        }
    }

    fn kill(&self, pid: Pid, sig: i32) -> SysResult<()> {
        self.core().kill(pid, sig)
    }

    /// Termina o processo de qualquer profundidade da pilha, rodando os Drops no caminho.
    fn exit(&self, code: i32) -> ! {
        crate::kernel::exit_process(code)
    }
}
