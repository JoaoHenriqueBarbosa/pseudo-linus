//! Leituras do processo host em `/proc` (só pra métricas da bancada).

/// Um campo em kB de `/proc/self/status` (ex.: "VmHWM", "VmRSS").
pub fn status_kib(field: &str) -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    text.lines().find_map(|l| {
        let rest = l.strip_prefix(field)?.strip_prefix(':')?;
        rest.trim().trim_end_matches("kB").trim().parse().ok()
    })
}

/// Pico de RSS do processo (VmHWM), em KiB.
pub fn peak_rss_kib() -> Option<u64> {
    status_kib("VmHWM")
}

/// Tempo de CPU da thread corrente em ns (`clock_gettime(CLOCK_THREAD_CPUTIME_ID)`, via rustix).
///
/// Diferente do tempo de parede, não conta a espera na fila do escalonador: numa máquina compartilhada
/// e carregada, é a medida que separa o custo do allocator do ruído dos vizinhos. Não aloca. (O
/// `/proc/thread-self/schedstat` não serve: só atualiza no tick do escalonador, 4 ms aqui.)
pub fn thread_cpu_ns() -> Option<u64> {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    Some(t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64)
}

/// `vm.overcommit_memory` do host.
pub fn overcommit_mode() -> Option<u32> {
    std::fs::read_to_string("/proc/sys/vm/overcommit_memory").ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_peak_rss() {
        let v = super::peak_rss_kib().expect("VmHWM");
        assert!(v > 0);
    }

    #[test]
    fn thread_cpu_time_advances_with_work() {
        let a = super::thread_cpu_ns().expect("relógio da thread");
        let mut x = 0u64;
        for i in 0..1_000_000u64 {
            x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(i));
        }
        let b = super::thread_cpu_ns().expect("relógio da thread");
        assert!(b > a, "{a} {b} {x}");
    }
}
