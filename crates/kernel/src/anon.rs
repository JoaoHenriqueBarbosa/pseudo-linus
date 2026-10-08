//! `eventfd2(2)` e `timerfd_create(2)`: os objetos atrás de um fd `anon_inode:[eventfd]` e
//! `anon_inode:[timerfd]`.
//!
//! O eventfd é um contador de 64 bits sob uma trava, com a fila dos leitores (contador zero) e a dos
//! escritores (soma que estouraria), como o `fs/eventfd.c`. O timerfd guarda o prazo e o intervalo e conta os
//! vencimentos de forma preguiçosa (na leitura e no `poll`); quem espera por ele (`poll`, `epoll_wait`) acorda
//! por uma thread do host que dorme até o prazo e que o rearme ou o fechamento cancelam.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use sysabi::{Errno, PollEvents};

use crate::park::{locked, Parker, WaitList, Wake};
use crate::pipe::Try;

/// O `eventfd-id` vem do `eventfd_ida` global do kernel: `ida_alloc` devolve o menor número livre do sistema
/// inteiro e `ida_free` o devolve ao fechar o eventfd. Num Debian em container o host já mantém eventfds
/// vivos (ids abaixo de `HOST_HELD`, que nunca voltam), então o primeiro do processo não é o 0; o valor
/// exato depende do host, e por isso os casos normalizam o número.
struct Ida {
    next: u64,
    freed: BTreeSet<u64>,
}

const HOST_HELD: u64 = 343;

static EVENTFD_IDA: Mutex<Ida> = Mutex::new(Ida { next: HOST_HELD, freed: BTreeSet::new() });

impl Ida {
    fn alloc(&mut self) -> u64 {
        self.freed.pop_first().unwrap_or_else(|| {
            self.next += 1;
            self.next - 1
        })
    }
}

/// O objeto de um fd `anon_inode` de contador ou de relógio.
pub(crate) enum Anon {
    Eventfd(Eventfd),
    Timerfd(Arc<Timerfd>),
}

impl Anon {
    /// O alvo do link `/proc/<pid>/fd/N`.
    pub(crate) fn link_text(&self) -> &'static [u8] {
        match self {
            Anon::Eventfd(_) => b"anon_inode:[eventfd]",
            Anon::Timerfd(_) => b"anon_inode:[timerfd]",
        }
    }

    pub(crate) fn poll(&self, waiter: Option<&Arc<Parker>>) -> PollEvents {
        match self {
            Anon::Eventfd(e) => e.poll(waiter),
            Anon::Timerfd(t) => t.poll(waiter),
        }
    }

    pub(crate) fn unregister(&self, waiter: &Arc<Parker>) {
        match self {
            Anon::Eventfd(e) => e.unregister(waiter),
            Anon::Timerfd(t) => t.unregister(waiter),
        }
    }

    /// As linhas do `fdinfo` que o `eventfd_show_fdinfo` e o `timerfd_show` acrescentam.
    pub(crate) fn fdinfo_lines(&self) -> String {
        match self {
            Anon::Eventfd(e) => e.fdinfo_lines(),
            Anon::Timerfd(t) => t.fdinfo_lines(),
        }
    }
}

impl std::fmt::Debug for Anon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Anon::Eventfd(_) => write!(f, "Eventfd"),
            Anon::Timerfd(_) => write!(f, "Timerfd"),
        }
    }
}

// ---- eventfd ----

#[derive(Default)]
struct EventState {
    count: u64,
    rwait: WaitList,
    wwait: WaitList,
}

pub(crate) struct Eventfd {
    id: u64,
    semaphore: bool,
    st: Mutex<EventState>,
}

impl Drop for Eventfd {
    fn drop(&mut self) {
        EVENTFD_IDA.lock().freed.insert(self.id);
    }
}

impl Eventfd {
    pub(crate) fn new(initval: u32, semaphore: bool) -> Eventfd {
        let st = EventState { count: u64::from(initval), ..EventState::default() };
        Eventfd { id: EVENTFD_IDA.lock().alloc(), semaphore, st: Mutex::new(st) }
    }

    /// `eventfd_read`: o contador, que zera (no modo semáforo, 1, que o decrementa).
    pub(crate) fn try_read(&self, nonblock: bool, waiter: &Arc<Parker>) -> Try<Result<u64, Errno>> {
        locked(&self.st, |s| {
            if s.count == 0 {
                if nonblock {
                    return (Try::Ready(Err(Errno::EAGAIN)), Wake::none());
                }
                s.rwait.register(waiter);
                return (Try::Pending, Wake::none());
            }
            s.rwait.unregister(waiter);
            let value = if self.semaphore { 1 } else { s.count };
            s.count -= value;
            (Try::Ready(Ok(value)), s.wwait.take_key(sysabi::epoll::OUT))
        })
    }

    /// `eventfd_write`: soma `value` ao contador; espera (ou EAGAIN) se a soma passaria de `ULLONG_MAX - 1`.
    pub(crate) fn try_write(&self, value: u64, nonblock: bool, waiter: &Arc<Parker>) -> Try<Result<(), Errno>> {
        locked(&self.st, |s| {
            if u64::MAX - s.count <= value {
                if nonblock {
                    return (Try::Ready(Err(Errno::EAGAIN)), Wake::none());
                }
                s.wwait.register(waiter);
                return (Try::Pending, Wake::none());
            }
            s.wwait.unregister(waiter);
            s.count += value;
            (Try::Ready(Ok(())), s.rwait.take_key(sysabi::epoll::IN))
        })
    }

    pub(crate) fn poll(&self, waiter: Option<&Arc<Parker>>) -> PollEvents {
        let mut s = self.st.lock();
        if let Some(w) = waiter {
            s.rwait.register(w);
            s.wwait.register(w);
        }
        let mut ready = PollEvents::empty();
        if s.count > 0 {
            ready |= PollEvents::IN;
        }
        if u64::MAX - s.count > 1 {
            ready |= PollEvents::OUT;
        }
        if s.count == u64::MAX {
            ready |= PollEvents::ERR;
        }
        ready
    }

    pub(crate) fn unregister(&self, waiter: &Arc<Parker>) {
        let mut s = self.st.lock();
        s.rwait.unregister(waiter);
        s.wwait.unregister(waiter);
    }

    fn fdinfo_lines(&self) -> String {
        format!(
            "eventfd-count: {:16x}\neventfd-id: {}\neventfd-semaphore: {}\n",
            self.st.lock().count,
            self.id,
            u8::from(self.semaphore)
        )
    }
}

// ---- timerfd ----

struct TimerState {
    /// Quando vence o próximo disparo (`None` desarmado).
    deadline: Option<Instant>,
    interval: Duration,
    /// Vencimentos ainda não lidos.
    ticks: u64,
    /// As flags do último `timerfd_settime` (o `fdinfo` as mostra).
    settime_flags: u32,
    wait: WaitList,
    /// A thread do host que acorda os observadores no prazo; acordá-la a manda embora.
    ticker: Option<Arc<Parker>>,
}

impl TimerState {
    /// Conta os vencimentos até `now`: um se o timer é de um disparo só, vários se o intervalo já passou.
    fn expire(&mut self, now: Instant) {
        let Some(deadline) = self.deadline else { return };
        if now < deadline {
            return;
        }
        if self.interval.is_zero() {
            self.ticks += 1;
            self.deadline = None;
        } else {
            let missed = (now - deadline).as_nanos() / self.interval.as_nanos();
            self.ticks += 1 + missed as u64;
            self.deadline = Some(deadline + self.interval * (1 + missed as u32));
        }
    }

    fn remaining(&self, now: Instant) -> Duration {
        self.deadline.map_or(Duration::ZERO, |d| d.saturating_duration_since(now))
    }
}

pub(crate) struct Timerfd {
    clock: i32,
    st: Mutex<TimerState>,
}

impl Drop for Timerfd {
    fn drop(&mut self) {
        if let Some(ticker) = self.st.get_mut().ticker.take() {
            ticker.unpark();
        }
    }
}

impl Timerfd {
    pub(crate) fn new(clock: i32) -> Arc<Timerfd> {
        let st = TimerState { deadline: None, interval: Duration::ZERO, ticks: 0, settime_flags: 0, wait: WaitList::default(), ticker: None };
        Arc::new(Timerfd { clock, st: Mutex::new(st) })
    }

    pub(crate) fn clock(&self) -> i32 {
        self.clock
    }

    /// O prazo do próximo vencimento, para o `read` bloqueante dormir até ele.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.st.lock().deadline
    }

    /// `timerfd_read`: os vencimentos desde a última leitura.
    pub(crate) fn try_read(&self, nonblock: bool, waiter: &Arc<Parker>) -> Try<Result<u64, Errno>> {
        let mut s = self.st.lock();
        s.expire(Instant::now());
        if s.ticks > 0 {
            s.wait.unregister(waiter);
            return Try::Ready(Ok(std::mem::take(&mut s.ticks)));
        }
        if nonblock {
            return Try::Ready(Err(Errno::EAGAIN));
        }
        s.wait.register(waiter);
        Try::Pending
    }

    pub(crate) fn poll(&self, waiter: Option<&Arc<Parker>>) -> PollEvents {
        let mut s = self.st.lock();
        s.expire(Instant::now());
        if let Some(w) = waiter {
            s.wait.register(w);
        }
        if s.ticks > 0 { PollEvents::IN } else { PollEvents::empty() }
    }

    pub(crate) fn unregister(&self, waiter: &Arc<Parker>) {
        self.st.lock().wait.unregister(waiter);
    }

    /// `timerfd_gettime`: o que falta e o intervalo, em nanossegundos.
    pub(crate) fn gettime(&self) -> (u64, u64) {
        let mut s = self.st.lock();
        s.expire(Instant::now());
        (s.remaining(Instant::now()).as_nanos() as u64, s.interval.as_nanos() as u64)
    }

    /// `timerfd_settime`: rearma com o prazo `value` a partir de agora (o chamador já converteu o tempo
    /// absoluto) e o intervalo; `value` zero desarma. Devolve o que o timer antigo tinha. Os vencimentos não
    /// lidos zeram.
    pub(crate) fn settime(self: &Arc<Self>, flags: u32, value: Duration, interval: Duration) -> (u64, u64) {
        let now = Instant::now();
        let mut s = self.st.lock();
        s.expire(now);
        let old = (s.remaining(now).as_nanos() as u64, s.interval.as_nanos() as u64);
        if let Some(ticker) = s.ticker.take() {
            ticker.unpark();
        }
        s.ticks = 0;
        s.settime_flags = flags;
        s.interval = interval;
        if value.is_zero() {
            s.deadline = None;
        } else {
            let deadline = now + value;
            s.deadline = Some(deadline);
            let ticker = Parker::new();
            s.ticker = Some(ticker.clone());
            Self::spawn_ticker(Arc::downgrade(self), ticker, deadline);
        }
        old
    }

    /// A thread que dorme até o prazo e acorda quem observa o timerfd; repete a cada intervalo e acaba quando é
    /// acordada (rearme ou fechamento) ou quando o timer não tem mais prazo.
    fn spawn_ticker(timer: std::sync::Weak<Timerfd>, ticker: Arc<Parker>, mut deadline: Instant) {
        std::thread::spawn(move || {
            while !ticker.park_until(deadline) {
                let Some(timer) = timer.upgrade() else { return };
                let (wake, next) = {
                    let mut s = timer.st.lock();
                    s.expire(Instant::now());
                    (s.wait.take(), s.deadline)
                };
                wake.run();
                match next {
                    Some(next) => deadline = next,
                    None => return,
                }
            }
        });
    }

    fn fdinfo_lines(&self) -> String {
        let mut s = self.st.lock();
        s.expire(Instant::now());
        let remaining = s.remaining(Instant::now());
        format!(
            "clockid: {}\nticks: {}\nsettime flags: 0{:o}\nit_value: ({}, {})\nit_interval: ({}, {})\n",
            self.clock,
            s.ticks,
            s.settime_flags,
            remaining.as_secs(),
            remaining.subsec_nanos(),
            s.interval.as_secs(),
            s.interval.subsec_nanos()
        )
    }
}
