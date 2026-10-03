//! Medições de memória do E03, num processo separado com allocator contador (`stats_alloc`).
//!
//! O binário principal roda este e lê o JSON da saída padrão. Fica separado porque o contador
//! global (contadores atômicos compartilhados) distorceria os tempos, principalmente o teste com
//! 16 threads.

use std::alloc::System;

use e03_tmpfs_persistent::memory::{Counts, content_memory, structure_memory};
use e03_tmpfs_persistent::{for_each_content, for_each_final, for_each_structure};
use serde_json::{Map, Value, json};
use stats_alloc::{INSTRUMENTED_SYSTEM, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

fn counts() -> Counts {
    let s = GLOBAL.stats();
    Counts {
        bytes_allocated: s.bytes_allocated as u64,
        bytes_deallocated: s.bytes_deallocated as u64,
        allocations: s.allocations as u64,
        deallocations: s.deallocations as u64,
    }
}

fn main() {
    let mut out: Map<String, Value> = Map::new();
    for_each_structure!(|meta, F| {
        eprintln!("memória: {}", meta.key);
        out.insert(meta.key.to_string(), json!({"structure": structure_memory::<F>(&counts)}));
    });
    for_each_content!(|meta, F| {
        eprintln!("memória: {}", meta.key);
        out.insert(meta.key.to_string(), json!({"content": content_memory::<F>(&counts)}));
    });
    for_each_final!(|meta, F| {
        eprintln!("memória: {}", meta.key);
        out.insert(
            meta.key.to_string(),
            json!({"structure": structure_memory::<F>(&counts), "content": content_memory::<F>(&counts)}),
        );
    });
    println!("{}", Value::Object(out));
}
