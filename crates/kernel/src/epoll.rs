//! `epoll(7)`: o objeto atrás de um fd `anon_inode:[eventpoll]`.
//!
//! A lista de interesse guarda `(fd, descrição)` como o `ep_find` do kernel: o mesmo arquivo pode entrar
//! com dois fds, e a entrada some sozinha quando a última referência à descrição fecha (a referência aqui é
//! fraca). A prontidão vem do mesmo `poll` que o `poll(2)` usa ([`Task::poll_ofd`]); `EPOLLET` e
//! `EPOLLONESHOT` guardam o estado por entrada. O `EPOLLET` segue o `ep_poll_callback`: a entrada tem um
//! parker próprio nas filas do arquivo, e cada despertar dele (dado novo, mesmo com o fd já pronto) a
//! reentrega, desde que a chave do despertar cruze a máscara da entrada (a escrita no outro sentido não
//! reentrega um `EPOLLIN`); sem despertar só vale a transição de não pronto para pronto.

use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use sysabi::epoll as ev;
use sysabi::{EpollEvent, Errno, Fd, PollEvents, SysResult};

use crate::fd::{FileObj, Ofd};
use crate::park::Parker;
use crate::proc::Task;
use crate::sys::unregister_ofd;

/// `EP_MAX_EVENTS`: `INT_MAX / sizeof(struct epoll_event)` (12 bytes, estrutura empacotada).
pub(crate) const EP_MAX_EVENTS: usize = (i32::MAX as usize) / 12;

/// Profundidade máxima de epolls aninhados (`EP_MAX_NESTS`).
const MAX_NESTS: usize = 4;

/// `st_dev`, `st_ino` e `mnt_id` do inode único do anon_inodefs (medidos no Debian 13).
pub(crate) const ANON_DEV: u64 = 16;
pub(crate) const ANON_INO: u64 = 58;
pub(crate) const ANON_MNT_ID: u32 = 17;

/// O `dev_t` do kernel (`major << 20 | minor`) a partir do `st_dev` da codificação do espaço de usuário
/// (`new_encode_dev`), como o `sdev:` do `ep_show_fdinfo` o imprime.
fn kernel_dev(st_dev: u64) -> u32 {
    let major = (st_dev >> 8) & 0xfff;
    let minor = (st_dev & 0xff) | ((st_dev >> 12) & 0xfff00);
    ((major << 20) | minor) as u32
}

struct Item {
    fd: Fd,
    id: u64,
    file: Weak<Ofd>,
    events: u32,
    data: u64,
    /// `EPOLLET`: os bits que estavam prontos na última passada.
    seen: u32,
    /// `EPOLLET`: despertado pelo arquivo quando chega algo (o `ep_poll_callback`).
    wake: Arc<Parker>,
    /// `EPOLLONESHOT` desarma a entrada depois do primeiro evento entregue; `EPOLL_CTL_MOD` rearma.
    armed: bool,
}

#[derive(Default)]
pub(crate) struct Epoll {
    items: Mutex<Vec<Item>>,
}

/// Os bits do `poll(2)` que a máscara de interesse pede (`EPOLLRDNORM` vale `POLLIN`, `EPOLLWRNORM`, `POLLOUT`).
fn interest(mask: u32) -> PollEvents {
    let mut bits = (mask & 0xffff) as u16 & !PollEvents::NVAL.bits();
    if mask & ev::RDNORM != 0 {
        bits |= PollEvents::IN.bits();
    }
    if mask & ev::WRNORM != 0 {
        bits |= PollEvents::OUT.bits();
    }
    PollEvents::from_bits_truncate(bits)
}

/// A prontidão do `poll(2)` no vocabulário do epoll: sockets e pipes reportam `RDNORM` junto de `IN`.
fn from_poll(p: PollEvents) -> u32 {
    let mut e = u32::from(p.bits());
    if p.contains(PollEvents::IN) {
        e |= ev::RDNORM;
    }
    if p.contains(PollEvents::OUT) {
        e |= ev::WRNORM;
    }
    e
}

/// O filtro do despertar de uma entrada: o `ep_poll_callback` descarta a chave que não cruza
/// `epi->event.events` (que o `ep_insert` já amplia com `ERR` e `HUP`). Um epoll aninhado é acordado com a
/// chave `EPOLLIN` pelo `ep_poll_safewake` seja qual for o evento do arquivo de dentro, então fica sem filtro.
fn wake_filter(events: u32, file: &Ofd) -> u32 {
    if matches!(file.obj, FileObj::Epoll(_)) { 0 } else { events | ev::ERR | ev::HUP }
}

impl Epoll {
    pub(crate) fn new() -> Epoll {
        Epoll::default()
    }

    pub(crate) fn add(self: &Arc<Self>, fd: Fd, file: &Arc<Ofd>, event: EpollEvent) -> SysResult<()> {
        let mut items = self.items.lock();
        if items.iter().any(|i| i.fd == fd && i.id == file.id) {
            return Err(Errno::EEXIST);
        }
        let wake = Parker::new();
        wake.set_filter(wake_filter(event.events, file));
        items.push(Item {
            fd,
            id: file.id,
            file: Arc::downgrade(file),
            events: event.events,
            data: event.data,
            seen: 0,
            wake,
            armed: true,
        });
        let mut watchers = file.watchers.lock();
        watchers.retain(|w| w.strong_count() > 0);
        if !watchers.iter().any(|w| std::ptr::eq(w.as_ptr(), Arc::as_ptr(self))) {
            watchers.push(Arc::downgrade(self));
        }
        Ok(())
    }

    pub(crate) fn modify(&self, fd: Fd, file: &Arc<Ofd>, event: EpollEvent) -> SysResult<()> {
        let mut items = self.items.lock();
        let item = items.iter_mut().find(|i| i.fd == fd && i.id == file.id).ok_or(Errno::ENOENT)?;
        item.events = event.events;
        item.data = event.data;
        item.seen = 0;
        item.wake.set_filter(wake_filter(event.events, file));
        item.wake.take_notified();
        item.armed = true;
        Ok(())
    }

    pub(crate) fn delete(&self, fd: Fd, file: &Arc<Ofd>) -> SysResult<()> {
        // `ep_remove`: o parker da entrada sai das filas do alvo sob a trava da lista, então um despertar
        // concorrente ou entrou antes (e a entrada some depois) ou não encontra mais a entrada.
        let mut items = self.items.lock();
        let at = items.iter().position(|i| i.fd == fd && i.id == file.id).ok_or(Errno::ENOENT)?;
        let item = items.remove(at);
        unregister_ofd(file, &item.wake);
        if !items.iter().any(|i| i.id == file.id) {
            file.watchers.lock().retain(|w| w.strong_count() > 0 && !std::ptr::eq(w.as_ptr(), self));
        }
        Ok(())
    }

    /// A máscara guardada de uma entrada (o `EPOLLEXCLUSIVE` decide o `EPOLL_CTL_MOD`).
    pub(crate) fn events_of(&self, fd: Fd, file: &Arc<Ofd>) -> Option<u32> {
        self.items.lock().iter().find(|i| i.fd == fd && i.id == file.id).map(|i| i.events)
    }

    /// O epoll de descrição `id` está na lista deste, direta ou por epolls aninhados (laço do `EPOLL_CTL_ADD`).
    pub(crate) fn reaches(&self, id: u64) -> bool {
        let files: Vec<Arc<Ofd>> = self.items.lock().iter().filter_map(|i| i.file.upgrade()).collect();
        files.iter().any(|f| f.id == id || matches!(&f.obj, FileObj::Epoll(e) if e.reaches(id)))
    }

    /// Altura da cadeia de epolls aninhados abaixo deste, contando ele (`ep_nested_calls`).
    pub(crate) fn depth(&self) -> usize {
        let files: Vec<Arc<Ofd>> = self.items.lock().iter().filter_map(|i| i.file.upgrade()).collect();
        1 + files.iter().filter_map(|f| if let FileObj::Epoll(e) = &f.obj { Some(e.depth()) } else { None }).max().unwrap_or(0)
    }

    pub(crate) fn too_deep(&self, extra: usize) -> bool {
        self.depth() + extra > MAX_NESTS
    }

    /// Os eventos prontos, no máximo `max`. `consume` aplica o efeito de entregar: a borda do `EPOLLET`,
    /// o desarme do `EPOLLONESHOT` e o giro das entradas de nível entregues para o fim da fila (a lista de
    /// prontos do kernel as recoloca no fim). Sem `consume` só se olha (o `poll` do próprio fd de epoll).
    /// `waiter` é registrado nas filas dos arquivos para acordar quem espera.
    pub(crate) fn scan(&self, task: &Task, max: usize, consume: bool, waiter: Option<&Arc<Parker>>) -> Vec<EpollEvent> {
        // As referências que o `upgrade` toma ficam vivas até depois de soltar a trava: se uma for a última, o
        // `Drop` da descrição chama `release`, que trava `items` de novo. A entrada de uma descrição morta
        // fica para o `release` tirar (junto com o parker dela nas filas), então aqui só é pulada.
        let mut hold = Vec::new();
        let mut items = self.items.lock();
        let mut out = Vec::new();
        let mut rotated = Vec::new();
        for (at, item) in items.iter_mut().enumerate() {
            if out.len() >= max {
                break;
            }
            if !item.armed {
                continue;
            }
            let Some(file) = item.file.upgrade() else { continue };
            hold.push(file);
            let file = hold.last().expect("just pushed");
            let edge = item.events & ev::ET != 0;
            // O `ep_poll_callback`: todo despertar do arquivo recoloca a entrada na lista de prontos, mesmo
            // que ela já estivesse pronta. O parker próprio da entrada fica nas filas do arquivo entre uma
            // espera e outra e guarda que houve chegada. Registra antes de olhar o estado (sob a trava do
            // objeto), para que chegada alguma se perca entre os dois.
            if edge && consume {
                task.poll_ofd(&file, interest(item.events), Some(&item.wake));
            }
            let polled = task.poll_ofd(&file, interest(item.events), waiter);
            let ready = from_poll(polled) & (item.events | ev::ERR | ev::HUP);
            let arrived = edge && if consume { item.wake.take_notified() } else { item.wake.is_notified() };
            let fresh = if edge { ready != 0 && (arrived || ready & !item.seen != 0) } else { ready != 0 };
            if consume && edge {
                item.seen = ready;
            }
            if !fresh {
                continue;
            }
            out.push(EpollEvent { events: ready, data: item.data });
            if consume {
                if item.events & ev::ONESHOT != 0 {
                    item.armed = false;
                } else if !edge {
                    rotated.push(at);
                }
            }
        }
        let mut moved: Vec<Item> = rotated.into_iter().rev().map(|at| items.remove(at)).collect();
        moved.reverse();
        items.extend(moved);
        out
    }

    /// As linhas `tfd:` do `ep_show_fdinfo`, na ordem da árvore do kernel (aqui, descrição e depois fd).
    /// `identify` dá o `st_dev` e o `st_ino` do alvo.
    pub(crate) fn fdinfo_lines(&self, identify: impl Fn(&Ofd) -> (u64, u64)) -> String {
        let mut live: Vec<(u64, Fd, Arc<Ofd>, u32, u64)> =
            self.items.lock().iter().filter_map(|i| i.file.upgrade().map(|f| (i.id, i.fd, f, i.events, i.data))).collect();
        live.sort_by_key(|(id, fd, ..)| (*id, fd.0));
        let mut text = String::new();
        for (_, fd, file, events, data) in live {
            let (dev, ino) = identify(&file);
            let pos = file.st.lock().pos as i64;
            text.push_str(&format!("tfd: {:8} events: {:8x} data: {:16x}  pos:{} ino:{:x} sdev:{:x}\n", fd.0, events, data, pos, ino, kernel_dev(dev)));
        }
        text
    }

    /// Solta o registro de `waiter` nos objetos observados.
    pub(crate) fn unregister(&self, waiter: &Arc<Parker>) {
        let files: Vec<Arc<Ofd>> = self.items.lock().iter().filter_map(|i| i.file.upgrade()).collect();
        for f in files {
            unregister_ofd(&f, waiter);
        }
    }

    /// `eventpoll_release`: a descrição `file` está sendo liberada; tira a entrada dela daqui e o parker da
    /// entrada das filas dela. Roda no `Drop` da descrição, então só olha `file` por referência.
    pub(crate) fn release(&self, file: &Ofd) {
        let gone = self.take_items(|i| i.id == file.id);
        for item in &gone {
            unregister_ofd(file, &item.wake);
        }
    }

    /// Tira as entradas que `pick` escolhe, na ordem em que estavam, sob a trava.
    fn take_items(&self, pick: impl Fn(&Item) -> bool) -> Vec<Item> {
        let mut items = self.items.lock();
        let (gone, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut *items).into_iter().partition(|i| pick(i));
        *items = keep;
        gone
    }
}

/// `ep_free`: o epoll fecha e cada entrada solta o parker dela das filas do alvo (`ep_remove`).
impl Drop for Epoll {
    fn drop(&mut self) {
        for item in std::mem::take(self.items.get_mut()) {
            if let Some(file) = item.file.upgrade() {
                unregister_ofd(&file, &item.wake);
                file.watchers.lock().retain(|w| w.strong_count() > 0);
            }
        }
    }
}
