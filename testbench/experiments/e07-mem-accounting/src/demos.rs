//! Demonstrações que abortam o processo host. Rodam sempre em subprocesso (o orquestrador e os testes
//! olham o sinal de término e o stderr).
//!
//! Todas seguem o mesmo roteiro: três threads "vizinhas" (outros pseudo-processos) ficam trabalhando,
//! o processo guloso faz o pedido que o allocator recusa, e o resultado mostra que a recusa dentro do
//! allocator não mata só o guloso: `handle_alloc_error` aborta o host inteiro, vizinhos inclusive, e
//! `catch_unwind` não segura porque não há unwind, há `abort`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

/// Roda `greedy` numa thread própria enquanto três vizinhas batem o coração. Se o processo sobreviver,
/// devolve o resultado e as batidas; se abortar, o stderr mostra as batidas de antes do pedido.
pub fn with_neighbors(greedy: impl FnOnce() -> Result<String, String> + Send) -> Value {
    let stop = AtomicBool::new(false);
    let beats = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
    std::thread::scope(|s| {
        for b in &beats {
            let stop = &stop;
            s.spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    b.fetch_add(1, Ordering::Relaxed);
                    std::thread::sleep(Duration::from_micros(200));
                }
            });
        }
        std::thread::sleep(Duration::from_millis(50));
        let before: Vec<u64> = beats.iter().map(|b| b.load(Ordering::Relaxed)).collect();
        eprintln!("e07-demo: vizinhos vivos antes do pedido, batidas {before:?}");
        let outcome = s
            .spawn(move || catch_unwind(AssertUnwindSafe(greedy)))
            .join()
            .map_err(|_| "a thread gulosa morreu sem abortar o host".to_string());
        std::thread::sleep(Duration::from_millis(20));
        stop.store(true, Ordering::Relaxed);
        let after: Vec<u64> = beats.iter().map(|b| b.load(Ordering::Relaxed)).collect();
        let outcome = match outcome {
            Ok(Ok(Ok(v))) => json!({"ok": v}),
            Ok(Ok(Err(e))) => json!({"err": e}),
            Ok(Err(_)) => json!({"panicked": true}),
            Err(e) => json!({"thread_lost": e}),
        };
        json!({"survived": true, "outcome": outcome, "beats_before": before, "beats_after": after})
    })
}

/// Tamanho de um pedido gigante: 32 TiB, acima de RAM + swap de qualquer host da bancada.
pub const HUGE: usize = 1 << 45;

/// Pedido gigante sem limite nenhum: com `vm.overcommit_memory = 0` o kernel recusa o `mmap` e o
/// `Vec::with_capacity` aborta o host. `fallible = true` usa `try_reserve_exact` e sobrevive.
pub fn huge_alloc(fallible: bool) -> Value {
    with_neighbors(move || {
        if fallible {
            let mut v: Vec<u8> = Vec::new();
            match v.try_reserve_exact(HUGE) {
                Ok(()) => Ok(format!("reservou {} bytes", v.capacity())),
                Err(e) => Err(format!("try_reserve_exact recusou: {e}")),
            }
        } else {
            // `black_box`: sem ele o LLVM elimina a alocação que ninguém usa e o pedido nunca chega ao
            // malloc.
            let v: Vec<u8> = Vec::with_capacity(HUGE);
            std::hint::black_box(&v);
            Ok(format!("reservou {} bytes", v.capacity()))
        }
    })
}

/// Limite duro armado no allocator: `arm(teto)` define o teto global e `allocated()` diz quanto já está
/// alocado. O guloso pede 64 MiB com 32 MiB de folga.
pub fn hard_limit(arm: impl Fn(usize) + Sync, allocated: impl Fn() -> usize + Sync, fallible: bool) -> Value {
    const ROOM: usize = 32 << 20;
    const ASK: usize = 64 << 20;
    let armed = AtomicBool::new(false);
    let v = with_neighbors(|| {
        arm(allocated() + ROOM);
        armed.store(true, Ordering::SeqCst);
        eprintln!("e07-demo: teto armado com 32 MiB de folga; o guloso pede 64 MiB");
        let r = if fallible {
            let mut v: Vec<u8> = Vec::new();
            match v.try_reserve_exact(ASK) {
                Ok(()) => Ok(format!("reservou {} bytes", v.capacity())),
                Err(e) => Err(format!("try_reserve_exact recusou: {e}")),
            }
        } else {
            let v = vec![0u8; ASK];
            std::hint::black_box(&v);
            Ok(format!("alocou {} bytes", v.len()))
        };
        arm(usize::MAX);
        r
    });
    arm(usize::MAX);
    let mut v = v;
    v["armed"] = json!(armed.load(Ordering::SeqCst));
    v
}
