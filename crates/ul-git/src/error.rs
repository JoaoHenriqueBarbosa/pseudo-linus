//! Erros no estilo do git: `die()` vira [`Fail::Fatal`] (exit 128), `usage` vira 129, e o resto é um
//! código de saída já com as mensagens impressas.

use crate::os;

/// Como um comando termina antes da hora.
#[derive(Debug)]
pub enum Fail {
    /// `fatal: <msg>` no stderr e exit 128, como o `die()` do git.
    Fatal(String),
    /// Mensagens já impressas; só falta sair com este código.
    Exit(i32),
}

pub type R<T> = Result<T, Fail>;

impl Fail {
    /// Imprime o que faltar e devolve o código de saída.
    pub fn report(self) -> i32 {
        match self {
            Fail::Fatal(msg) => {
                os::err_line("fatal: ", &msg);
                128
            }
            Fail::Exit(code) => code,
        }
    }
}

/// `return Err(Fail::Fatal(format!(...)))`.
#[macro_export]
macro_rules! die {
    ($($arg:tt)*) => {
        return Err($crate::error::Fail::Fatal(format!($($arg)*)))
    };
}

/// `Fail::Fatal(format!(...))`, pra usar com `ok_or_else` e `map_err`.
#[macro_export]
macro_rules! fatal {
    ($($arg:tt)*) => {
        $crate::error::Fail::Fatal(format!($($arg)*))
    };
}

/// `error: <msg>` no stderr (sem sair).
pub fn error(msg: &str) {
    os::err_line("error: ", msg);
}

/// `warning: <msg>` no stderr.
pub fn warning(msg: &str) {
    os::err_line("warning: ", msg);
}

/// Cada linha de `msg` com o prefixo `hint: ` (linha vazia vira `hint:`), como o `advise()` do git.
pub fn hint(msg: &str) {
    let mut out = String::new();
    for line in msg.split('\n') {
        if line.is_empty() {
            out.push_str("hint:\n");
        } else {
            out.push_str("hint: ");
            out.push_str(line);
            out.push('\n');
        }
    }
    os::err_bytes(out.as_bytes());
}

/// `error: <msg>` e o código dado.
pub fn error_exit<T>(msg: &str, code: i32) -> R<T> {
    error(msg);
    Err(Fail::Exit(code))
}
