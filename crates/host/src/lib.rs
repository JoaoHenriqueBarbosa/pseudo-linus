//! host: o lado do pseudo-linus que fala com o mundo.
//!
//! - [`auth`]: usuários e chaves de API (hash SHA-256, expiração, revogação), num arquivo JSON com
//!   escrita atômica e trava entre processos.
//! - [`config`]: configuração do daemon (TOML com sobrescrita por variáveis de ambiente).
//! - [`timeutil`]: datas UTC e durações (`30d`, `12h`) sem depender do fuso do host.
//! - [`rpc`]: envelope e erros do JSON-RPC 2.0; [`api`]: parâmetros e resultados dos métodos.

//! - [`backend`]: as primitivas que o worker usa do kernel; [`exec`]: execução com timeout, limite
//!   de saída e cancelamento em cima delas.
//! - `fake` (feature `fake-backend` e testes): dublê do kernel pra testar o host sozinho.

pub mod api;
pub mod auth;
pub mod backend;
pub mod client;
pub mod config;
pub mod exec;
#[cfg(any(test, feature = "fake-backend"))]
pub mod fake;
pub mod fsops;
pub mod ids;
pub mod ipc;
pub mod isolation;
#[cfg(feature = "kernel")]
pub mod kernel_backend;
pub mod logging;
pub mod methods;
pub mod rpc;
pub mod server;
pub mod session;
pub mod supervisor;
pub mod tarball;
#[cfg(feature = "test-programs")]
pub mod testprogs;
pub mod timeutil;
pub mod worker;
