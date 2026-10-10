//! `require("string_decoder")`, `require("stream")`, `require("querystring")`, `require("timers")` e
//! `require("timers/promises")` do bun 1.4.2, escritos em JavaScript (`node_string_decoder.js`, `node_stream_modules.js`,
//! gerado por `scripts/gen-node-stream.js` a partir dos fontes do stream do bun, `node_querystring.js`, o
//! `querystring.ts` do bun, e `node_timers.js` e `node_timers_promises.js`, os `timers*.ts` do bun).

use crate::api::node_js_host::install_js_module;
use crate::runtime::host_call::HostResult;
use crate::runtime::js_global_object::JSGlobalObject;

const STRING_DECODER: &str = include_str!("node_string_decoder.js");
const STREAM: &str = include_str!("node_stream_modules.js");
const QUERYSTRING: &str = include_str!("node_querystring.js");
const TIMERS: &str = include_str!("node_timers.js");
const TIMERS_PROMISES: &str = include_str!("node_timers_promises.js");

/// Instalador de `node:string_decoder`.
pub(crate) fn install_string_decoder_module(global_object: &JSGlobalObject) -> HostResult {
    install_js_module(global_object, STRING_DECODER, "string_decoder")
}

/// Instalador de `node:stream`.
pub(crate) fn install_stream_module(global_object: &JSGlobalObject) -> HostResult {
    install_js_module(global_object, STREAM, "stream")
}

/// Instalador de `node:querystring`.
pub(crate) fn install_querystring_module(global_object: &JSGlobalObject) -> HostResult {
    install_js_module(global_object, QUERYSTRING, "querystring")
}

/// Instalador de `node:timers`.
pub(crate) fn install_timers_module(global_object: &JSGlobalObject) -> HostResult {
    install_js_module(global_object, TIMERS, "timers")
}

/// Instalador de `node:timers/promises`.
pub(crate) fn install_timers_promises_module(global_object: &JSGlobalObject) -> HostResult {
    install_js_module(global_object, TIMERS_PROMISES, "timers/promises")
}
