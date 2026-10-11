//! `StackCheck` de `src/bun_core/util.rs` do upstream: diz se ainda há pilha para recursão.
//!
//! No upstream o limite da pilha vem do WebKit (`Bun__StackCheck__initialize` lê os limites da thread com
//! `pthread_getattr_np`) e a posição atual vem do registrador de pilha por `asm!`. Sem `unsafe`, o porte mede a
//! posição pelo endereço de uma variável local (só o número, sem desreferência) e o limite pelo tamanho da pilha da
//! thread: o `RLIMIT_STACK` corrente de `/proc/self/limits` para a thread principal (`configure_thread`), ou o
//! tamanho com que a thread foi criada (`configure_thread_with_size`). O fim da pilha é a posição na configuração
//! menos esse tamanho; como a configuração acontece perto do topo da pilha, o erro fica em poucos quadros, muito
//! abaixo da folga de 128 KiB que o teste exige. O limiar é o do upstream fora do Windows.

use core::cell::Cell;

std::thread_local! {
    static STACK_END: Cell<usize> = const { Cell::new(0) };
}

/// O tamanho da pilha que o Linux dá à thread principal quando o limite não pode ser lido.
const DEFAULT_MAIN_STACK: usize = 8 * 1024 * 1024;

const THRESHOLD: usize = 128 * 1024;

#[derive(Clone, Copy, Default)]
pub struct StackCheck {
    cached_stack_end: usize,
}

#[inline(always)]
fn frame_address() -> usize {
    let probe = 0u8;
    core::ptr::from_ref(&probe).addr()
}

/// O limite flexível de pilha (`Max stack size`, primeira coluna) de `/proc/self/limits`.
fn rlimit_stack() -> Option<usize> {
    let limits = std::fs::read_to_string("/proc/self/limits").ok()?;
    let line = limits.lines().find(|l| l.starts_with("Max stack size"))?;
    line["Max stack size".len()..].split_whitespace().next()?.parse().ok()
}

impl StackCheck {
    /// Para a thread principal: o tamanho é o `RLIMIT_STACK` corrente.
    pub fn configure_thread() {
        Self::configure_thread_with_size(rlimit_stack().unwrap_or(DEFAULT_MAIN_STACK));
    }

    /// Para uma thread criada com pilha de `size` bytes.
    pub fn configure_thread_with_size(size: usize) {
        STACK_END.set(frame_address().saturating_sub(size));
    }

    /// Sem `configure_thread*` na thread, o fim fica em `0` e `is_safe_to_recurse()` sempre diz que sim, como o
    /// `Default` do upstream.
    pub fn init() -> Self {
        Self { cached_stack_end: STACK_END.get() }
    }

    pub fn update(&mut self) {
        self.cached_stack_end = STACK_END.get();
    }

    /// Is there enough stack space to safely recurse?
    pub fn is_safe_to_recurse(self) -> bool {
        let remaining = frame_address().saturating_sub(self.cached_stack_end);
        remaining > THRESHOLD
    }

    /// Like [`Self::is_safe_to_recurse`] but reserves `extra` bytes of additional headroom on top of the threshold.
    pub fn is_safe_to_recurse_with_extra(self, extra: usize) -> bool {
        let remaining = frame_address().saturating_sub(self.cached_stack_end);
        remaining > THRESHOLD.saturating_add(extra)
    }
}

#[cfg(test)]
mod tests {
    use super::StackCheck;

    fn depth_until_unsafe(check: StackCheck, depth: usize) -> usize {
        let pad = [depth as u8; 512];
        if !check.is_safe_to_recurse() {
            return depth;
        }
        depth_until_unsafe(check, depth + 1) + usize::from(core::hint::black_box(pad)[0] == 0xFF)
    }

    #[test]
    fn stops_before_the_end_of_the_stack() {
        assert!(StackCheck::init().is_safe_to_recurse());
        let handle = std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(|| {
                StackCheck::configure_thread_with_size(1024 * 1024);
                let check = StackCheck::init();
                assert!(check.is_safe_to_recurse());
                assert!(!check.is_safe_to_recurse_with_extra(2 * 1024 * 1024));
                depth_until_unsafe(check, 0)
            })
            .expect("spawn");
        let depth = handle.join().expect("a recursão parou antes de estourar a pilha");
        assert!(depth > 100);
    }
}
