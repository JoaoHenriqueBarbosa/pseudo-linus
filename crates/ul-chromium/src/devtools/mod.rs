//! O DevTools remoto do Chromium (`--remote-debugging-port`): o servidor HTTP dos comandos `/json/*`, o WebSocket
//! e o protocolo CDP sobre ele.

pub mod http;
pub mod json;
pub mod websocket;
