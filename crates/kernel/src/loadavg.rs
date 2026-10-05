//! Média de carga do sandbox: o `calc_load` do kernel (`kernel/sched/loadavg.c`) sobre as tarefas
//! rodando dele. Quem chama a amostra é a thread de tick, de 5 em 5 segundos (`LOAD_FREQ`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};

use crate::proc::INIT_PID;
use crate::sandbox::SbInner;

/// Ponto fixo das médias (`FSHIFT`).
const FSHIFT: u32 = 11;
const FIXED_1: u64 = 1 << FSHIFT;
/// `1/exp(5s/1min)`, `1/exp(5s/5min)` e `1/exp(5s/15min)` em ponto fixo.
const EXP_1: u64 = 1884;
const EXP_5: u64 = 2014;
const EXP_15: u64 = 2037;

/// `calc_load`: média móvel exponencial, arredondando pra cima quando a carga sobe.
fn calc_load(load: u64, exp: u64, active: u64) -> u64 {
    let mut newload = load * exp + active * (FIXED_1 - exp);
    if active >= load {
        newload += FIXED_1 - 1;
    }
    newload / FIXED_1
}

/// As médias de carga de um sandbox.
pub(crate) struct LoadAvg {
    avg: [AtomicU64; 3],
    sb: OnceLock<Weak<SbInner>>,
}

impl LoadAvg {
    pub(crate) fn new() -> Arc<LoadAvg> {
        Arc::new(LoadAvg { avg: [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)], sb: OnceLock::new() })
    }

    /// Liga ao sandbox cujas tarefas são contadas.
    pub(crate) fn bind(&self, sb: &Arc<SbInner>) {
        let _ = self.sb.set(Arc::downgrade(sb));
    }

    /// `avenrun` em ponto fixo (`FSHIFT` = 11).
    pub(crate) fn get(&self) -> [u64; 3] {
        [self.avg[0].load(Ordering::Relaxed), self.avg[1].load(Ordering::Relaxed), self.avg[2].load(Ordering::Relaxed)]
    }

    /// Uma rodada de `calc_global_load`.
    pub(crate) fn sample(&self) {
        let Some(sb) = self.sb.get().and_then(Weak::upgrade) else { return };
        let (running, _) = count_tasks(&sb);
        let active = u64::from(running) * FIXED_1;
        for (slot, exp) in self.avg.iter().zip([EXP_1, EXP_5, EXP_15]) {
            let old = slot.load(Ordering::Relaxed);
            slot.store(calc_load(old, exp, active), Ordering::Relaxed);
        }
    }
}

/// Tarefas prontas ou rodando (não dormindo numa espera, de processo não parado) e tarefas existentes
/// (threads vivas, o init e os zumbis ainda não colhidos).
pub(crate) fn count_tasks(sb: &SbInner) -> (u32, u32) {
    let procs: Vec<(Arc<crate::proc::Proc>, bool)> = {
        let t = sb.table.lock();
        t.map.values().map(|e| (e.proc.clone(), e.rel.zombie.is_some())).collect()
    };
    let (mut running, mut total) = (0u32, 0u32);
    for (p, zombie) in procs {
        if p.pid == INIT_PID || zombie {
            total += 1;
            continue;
        }
        let stopped = p.stopped.load(Ordering::Relaxed);
        let th = p.threads.lock();
        total += th.live.len().max(1) as u32;
        if !stopped {
            running += th.live.values().filter(|t| !t.blocked.load(Ordering::Relaxed)).count() as u32;
        }
    }
    (running, total)
}
