//! Contadores por grupo alimentados pelo `tracking-allocator` (metrics-rs/tobz).
//!
//! O `tracking-allocator` guarda, num cabeçalho de 8 bytes antes de cada alocação, o id do grupo que
//! estava ativo na thread quando ela aconteceu, e no `dealloc` entrega ao nosso `AllocationTracker` o
//! grupo de origem (`source_group_id`) e o grupo corrente. Com isso a tabela abaixo debita o grupo que
//! alocou, mesmo quando outra thread libera: é exatamente a semântica de "memória do processo A".
//!
//! A tabela é um vetor estático de slots alinhados a 64 bytes (um por linha de cache, sem falso
//! compartilhamento entre processos), indexado por `id & (SLOTS - 1)`. Os ids do `tracking-allocator`
//! são monotônicos e nunca reaproveitados; o kernel real recicla o slot quando o processo morre (o
//! `reset` aqui), e a colisão só acontece entre processos separados por 65536 criações.
//!
//! Todo o código daqui é seguro: o `unsafe` está dentro da crate (o `GlobalAlloc` e o cabeçalho).

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

use tracking_allocator::{AllocationGroupId, AllocationGroupToken, AllocationRegistry, AllocationTracker};

use crate::accounting::{Accounting, Capabilities, Pid};

/// Quantidade de slots (potência de 2).
pub const SLOTS: usize = 1 << 16;

/// Contadores de um processo. Tudo zero no início: assim a tabela estática vai pro `.bss` e só ocupa
/// memória física nas páginas tocadas.
///
/// Os bytes vivos são `owned - foreign_freed`. `owned` só é escrito pela thread que está dentro do grupo
/// (o `AllocationGroupToken` só pode estar ativo numa thread por vez), então ela atualiza com load e
/// store relaxados, sem instrução `lock`; liberações feitas por outra thread vão pra `foreign_freed`
/// com `fetch_add`. O grupo raiz (threads fora de processo, todas ao mesmo tempo) usa sempre `fetch_add`.
#[repr(align(64))]
#[derive(Debug)]
pub struct Slot {
    /// Bytes alocados menos bytes liberados pela própria thread do grupo.
    owned: AtomicI64,
    /// Bytes do grupo liberados por outras threads.
    foreign_freed: AtomicI64,
    /// Teto em bytes; 0 = sem teto.
    limit: AtomicI64,
    /// O allocator viu o processo passar do teto.
    over: AtomicBool,
    /// Quantas vezes `over` foi marcada (só a primeira importa; o resto é contagem barata).
    over_marks: AtomicU64,
}

/// Id do grupo raiz do `tracking-allocator` (alocações fora de qualquer grupo).
const ROOT: u64 = 1;

impl Slot {
    const fn new() -> Slot {
        Slot {
            owned: AtomicI64::new(0),
            foreign_freed: AtomicI64::new(0),
            limit: AtomicI64::new(0),
            over: AtomicBool::new(false),
            over_marks: AtomicU64::new(0),
        }
    }

    #[inline]
    fn live(&self) -> i64 {
        self.owned.load(Ordering::Relaxed) - self.foreign_freed.load(Ordering::Relaxed)
    }

    /// Alocação feita pela thread do grupo. `shared` = o grupo pode estar ativo em várias threads.
    #[inline]
    fn charge(&self, bytes: i64, shared: bool) {
        let owned = if shared {
            self.owned.fetch_add(bytes, Ordering::Relaxed) + bytes
        } else {
            let v = self.owned.load(Ordering::Relaxed) + bytes;
            self.owned.store(v, Ordering::Relaxed);
            v
        };
        let limit = self.limit.load(Ordering::Relaxed);
        if limit != 0 && owned - self.foreign_freed.load(Ordering::Relaxed) > limit && !self.over.load(Ordering::Relaxed) {
            self.over.store(true, Ordering::Release);
            self.over_marks.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Liberação pela própria thread do grupo.
    #[inline]
    fn uncharge_own(&self, bytes: i64, shared: bool) {
        if shared {
            self.owned.fetch_sub(bytes, Ordering::Relaxed);
        } else {
            self.owned.store(self.owned.load(Ordering::Relaxed) - bytes, Ordering::Relaxed);
        }
    }

    /// Liberação feita por outra thread.
    #[inline]
    fn uncharge_foreign(&self, bytes: i64) {
        self.foreign_freed.fetch_add(bytes, Ordering::Relaxed);
    }
}

/// A tabela de todos os processos.
#[derive(Debug)]
pub struct GroupTable {
    slots: [Slot; SLOTS],
}

impl GroupTable {
    const fn new() -> GroupTable {
        GroupTable { slots: [const { Slot::new() }; SLOTS] }
    }

    #[inline]
    pub fn slot(&self, id: u64) -> &Slot {
        &self.slots[(id as usize) & (SLOTS - 1)]
    }

    /// Prepara o slot de um processo novo.
    pub fn reset(&self, id: u64, limit: Option<i64>) {
        let s = self.slot(id);
        s.owned.store(0, Ordering::Relaxed);
        s.foreign_freed.store(0, Ordering::Relaxed);
        s.over.store(false, Ordering::Relaxed);
        s.over_marks.store(0, Ordering::Relaxed);
        s.limit.store(limit.unwrap_or(0).max(0), Ordering::Release);
    }

    pub fn live(&self, id: u64) -> i64 {
        self.slot(id).live()
    }

    pub fn over(&self, id: u64) -> bool {
        self.slot(id).over.load(Ordering::Acquire)
    }

    /// Aplica uma alocação feita pela thread corrente no grupo `id` (o tracker e os testes chamam).
    #[inline]
    pub fn on_alloc(&self, id: u64, bytes: usize) {
        self.slot(id).charge(bytes as i64, id == ROOT);
    }

    /// Aplica uma liberação ao grupo de origem; `current_id` é o grupo ativo na thread que libera.
    #[inline]
    pub fn on_dealloc(&self, source_id: u64, current_id: u64, bytes: usize) {
        let s = self.slot(source_id);
        if source_id == current_id {
            s.uncharge_own(bytes as i64, source_id == ROOT);
        } else {
            s.uncharge_foreign(bytes as i64);
        }
    }
}

/// A tabela global usada pelo tracker instalado.
pub static TABLE: GroupTable = GroupTable::new();

/// O `AllocationTracker` que alimenta [`TABLE`]. Conta `object_size` (o pedido), não o tamanho com
/// cabeçalho, pra que "bytes vivos" signifique o mesmo em todos os candidatos.
#[derive(Debug)]
pub struct GroupTracker;

impl AllocationTracker for GroupTracker {
    #[inline]
    fn allocated(&self, _addr: usize, object_size: usize, _wrapped_size: usize, group_id: AllocationGroupId) {
        TABLE.on_alloc(group_id.as_usize().get() as u64, object_size);
    }

    #[inline]
    fn deallocated(
        &self,
        _addr: usize,
        object_size: usize,
        _wrapped_size: usize,
        source_group_id: AllocationGroupId,
        current_group_id: AllocationGroupId,
    ) {
        TABLE.on_dealloc(
            source_group_id.as_usize().get() as u64,
            current_group_id.as_usize().get() as u64,
            object_size,
        );
    }
}

thread_local! {
    /// Grupo ativo nesta thread (0 = nenhum). `const` pra não alocar no acesso.
    static CURRENT: Cell<u64> = const { Cell::new(0) };
    /// Bytes vivos do último processo desta thread, lidos ao fim dele.
    static LAST_NET: Cell<Option<i64>> = const { Cell::new(None) };
}

/// Adaptador do `tracking-allocator` para a bancada. Serve pra qualquer allocator interno (System ou
/// mimalloc): o binário declara o `#[global_allocator]` e chama [`TrackingAdapter::install`].
#[derive(Debug)]
pub struct TrackingAdapter {
    pub name: &'static str,
}

impl TrackingAdapter {
    /// Instala o tracker e liga a contabilidade. Chamar uma vez, no começo do `main`.
    pub fn install() {
        AllocationRegistry::set_global_tracker(GroupTracker).expect("tracker global já instalado");
        AllocationRegistry::enable_tracking();
    }
}

impl Accounting for TrackingAdapter {
    fn name(&self) -> &'static str {
        self.name
    }

    fn caps(&self) -> Capabilities {
        Capabilities {
            per_group: true,
            self_read: true,
            remote_read: true,
            kernel_scope: true,
            limit_flag: true,
            hard_limit: false,
            global_only: false,
        }
    }

    fn run_process<R>(&self, limit: Option<i64>, body: impl FnOnce(Pid) -> R) -> R {
        let mut token = AllocationGroupToken::register().expect("ids de grupo esgotados");
        let id = token.id().as_usize().get() as u64;
        TABLE.reset(id, limit);
        let previous = CURRENT.with(|c| c.replace(id));
        let out = {
            let _guard = token.enter();
            body(Pid(id))
        };
        LAST_NET.with(|c| c.set(Some(TABLE.live(id))));
        CURRENT.with(|c| c.set(previous));
        out
    }

    fn self_live(&self) -> Option<i64> {
        let id = CURRENT.with(Cell::get);
        (id != 0).then(|| TABLE.live(id))
    }

    fn remote_live(&self, pid: Pid) -> Option<i64> {
        Some(TABLE.live(pid.0))
    }

    fn last_process_net(&self) -> Option<i64> {
        LAST_NET.with(Cell::get)
    }

    fn kernel_scope<R>(&self, f: impl FnOnce() -> R) -> R {
        AllocationRegistry::untracked(f)
    }

    fn over_limit(&self, pid: Pid) -> Option<bool> {
        Some(TABLE.over(pid.0))
    }
}

#[cfg(test)]
mod tests {
    // O binário de teste usa o System allocator e não instala o tracker, então a TABLE global só muda
    // por estes testes. Cada teste usa ids próprios porque eles rodam em paralelo.
    use super::*;

    #[test]
    fn charge_and_cross_thread_uncharge_hit_the_source_slot() {
        let t = &TABLE;
        t.reset(7, None);
        t.reset(8, None);
        t.on_alloc(7, 1000);
        t.on_alloc(8, 10);
        // A thread do grupo 8 libera o que o 7 alocou: o tracker passa a origem (7) e o grupo corrente (8).
        t.on_dealloc(7, 8, 1000);
        assert_eq!(t.live(7), 0);
        assert_eq!(t.live(8), 10);
        // A própria thread do 8 libera o que é dela.
        t.on_dealloc(8, 8, 10);
        assert_eq!(t.live(8), 0);
    }

    #[test]
    fn owner_and_foreign_paths_race_without_losing_updates() {
        // O dono escreve com load+store enquanto outra thread debita pelo caminho atômico: nada se perde.
        let t = &TABLE;
        t.reset(11, None);
        let n = 200_000usize;
        t.on_alloc(11, n * 8);
        std::thread::scope(|s| {
            s.spawn(|| {
                for _ in 0..n {
                    t.on_alloc(11, 3);
                    t.on_dealloc(11, 11, 3);
                }
            });
            s.spawn(|| {
                for _ in 0..n {
                    t.on_dealloc(11, 12, 8);
                }
            });
        });
        assert_eq!(t.live(11), 0);
    }

    #[test]
    fn root_group_is_shared_by_many_threads() {
        let t = &TABLE;
        let before = t.live(ROOT);
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    for _ in 0..50_000 {
                        t.on_alloc(ROOT, 16);
                        t.on_dealloc(ROOT, ROOT, 16);
                    }
                });
            }
        });
        assert_eq!(t.live(ROOT), before);
    }

    #[test]
    fn limit_flag_marks_once_over() {
        let t = &TABLE;
        t.reset(3, Some(100));
        t.on_alloc(3, 100);
        assert!(!t.over(3));
        t.on_alloc(3, 1);
        assert!(t.over(3));
        t.on_dealloc(3, 3, 50);
        assert!(t.over(3), "a flag fica marcada até o kernel agir");
        t.reset(3, Some(100));
        t.on_alloc(3, 90);
        t.on_dealloc(3, 4, 50);
        t.on_alloc(3, 50);
        assert!(!t.over(3), "o que outra thread liberou desconta do teto");
        t.reset(3, None);
        assert!(!t.over(3));
        t.on_alloc(3, 1 << 40);
        assert!(!t.over(3), "sem teto, nunca marca");
    }

    #[test]
    fn slots_wrap_by_mask() {
        let t = &TABLE;
        t.reset(5, None);
        t.on_alloc(5 + SLOTS as u64, 42);
        assert_eq!(t.live(5), 42);
    }

    #[test]
    fn slots_are_cache_line_sized() {
        assert_eq!(std::mem::size_of::<Slot>(), 64);
        assert_eq!(std::mem::align_of::<Slot>(), 64);
    }
}
