//! `require("string_decoder")` e `require("stream")` do bun 1.4.2, escritos em JavaScript (`node_string_decoder.js` e
//! `node_stream_modules.js`, este gerado por `scripts/gen-node-stream.js` a partir dos fontes do stream do bun).

use crate::api::node_js_host::{module_host, run_factory};
use crate::runtime::host_call::HostResult;
use crate::runtime::js_global_object::JSGlobalObject;

const STRING_DECODER: &str = include_str!("node_string_decoder.js");
const STREAM: &str = include_str!("node_stream_modules.js");

/// Instalador de `node:string_decoder`.
pub(crate) fn install_string_decoder_module(global_object: &JSGlobalObject) -> HostResult {
    let host = module_host(global_object, &[])?;
    Ok(run_factory(global_object, STRING_DECODER, "string_decoder", &host))
}

/// Instalador de `node:stream`.
pub(crate) fn install_stream_module(global_object: &JSGlobalObject) -> HostResult {
    let host = module_host(global_object, &[])?;
    Ok(run_factory(global_object, STREAM, "stream", &host))
}
