//! Sonda do E06 (H19): cada módulo toca o host de um jeito diferente, direto no código do crate de
//! userland. O E06 roda o `cargo clippy` uma vez e atribui cada diagnóstico ao módulo pelo arquivo.
//! Os módulos `direct_*` e `fn_pointer` são o controle (o lint precisa pegar); `handle_from_dep` é o
//! I/O feito com um handle do host que chegou por uma dependência.

pub mod direct_env;
pub mod direct_fs;
pub mod direct_net;
pub mod direct_print_macro;
pub mod direct_process;
pub mod direct_stdout;
pub mod file_type_path;
pub mod fn_pointer;
pub mod handle_from_dep;
