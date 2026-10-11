//! Os módulos do bun 1.4.2 que só dependem do registro de `internal/*` (`api/node_internals.js`): `node:diagnostics_channel`,
//! `node:http`, `node:https`, `node:child_process`, `node:zlib` e os `node:_http_*`, gerados por `scripts/gen-node-modules.js` (um arquivo `api/node_<nome>.js` por
//! módulo). O servidor (`Bun.serve` com `onNodeHTTPRequest`) e o analisador HTTP nativo ainda não existem e lançam "not
//! implemented" no ponto de uso.

use crate::runtime::host_call::HostResult;
use crate::runtime::js_global_object::JSGlobalObject;

/// Monta a fábrica `source` (o módulo `label`) com o `host` dos módulos gerados.
fn install(global_object: &JSGlobalObject, source: &str, label: &str) -> HostResult {
    let host_object = crate::api::node_js_host::module_host(
        global_object,
        &[
            ("internalModule", 1, crate::runtime::node_internals::internal_module_function),
            ("getAsyncContext", 0, crate::api::node_async_hooks::get_async_context),
            ("setAsyncContext", 1, crate::api::node_async_hooks::set_async_context),
        ],
    )?;
    Ok(crate::api::node_js_host::run_factory(global_object, source, label, &host_object))
}

macro_rules! installers {
    ($($name:ident => ($source:literal, $label:literal)),* $(,)?) => {
        $(
            #[doc = concat!("Instalador de `node:", $label, "`.")]
            pub(crate) fn $name(global_object: &JSGlobalObject) -> HostResult {
                install(global_object, include_str!($source), $label)
            }
        )*
    };
}

installers! {
    install_diagnostics_channel_module => ("../api/node_diagnostics_channel.js", "diagnostics_channel"),
    install_http_module => ("../api/node_http.js", "http"),
    install_https_module => ("../api/node_https.js", "https"),
    install_child_process_module => ("../api/node_child_process.js", "child_process"),
    install_zlib_module => ("../api/node_zlib.js", "zlib"),
    install_http_agent_module => ("../api/node__http_agent.js", "_http_agent"),
    install_http_client_module => ("../api/node__http_client.js", "_http_client"),
    install_http_common_module => ("../api/node__http_common.js", "_http_common"),
    install_http_incoming_module => ("../api/node__http_incoming.js", "_http_incoming"),
    install_http_outgoing_module => ("../api/node__http_outgoing.js", "_http_outgoing"),
    install_http_server_module => ("../api/node__http_server.js", "_http_server"),
}
