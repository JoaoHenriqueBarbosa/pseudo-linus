//! O subconjunto de `src/bun_core/output.rs` do upstream que o parser usa: o `panic` e o escritor de erro.
//!
//! No upstream o `error_writer` devolve o fluxo de erro do processo guardado num thread-local que o `main` da CLI
//! inicializa; o porte não tem essa CLI, então é o `stderr` do processo direto.

use core::fmt;

pub fn panic(args: fmt::Arguments<'_>) -> ! {
    core::panic!("{args}");
}

pub fn error_writer() -> std::io::Stderr {
    std::io::stderr()
}

/// `todo_panic!`: o `TODO: ... (arquivo:linha)` do upstream. O contador `Global::features::TODO_PANIC` dele só
/// alimenta o relatório de falha da CLI, que o porte não tem.
#[macro_export]
macro_rules! todo_panic {
    ($($arg:tt)*) => {{
        $crate::output::panic(::core::format_args!(
            "TODO: {} ({}:{})",
            ::core::format_args!($($arg)*),
            ::core::file!(),
            ::core::line!(),
        ))
    }};
}

#[cfg(test)]
mod tests {
    #[test]
    #[should_panic(expected = "TODO: registerServerReference (")]
    fn todo_panic_message() {
        crate::todo_panic!("registerServerReference");
    }
}
